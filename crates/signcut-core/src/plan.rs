//! Cut planning: turn placed designs into an ordered list of knife moves in
//! cutter coordinates (millimetres).
//!
//! Coordinate conventions:
//! * Sheet (what the UI shows): x to the right along the material length,
//!   y downward across the material width, origin at the top-left corner.
//!   The bottom edge of the sheet is the cutter's origin side (front-right
//!   corner as you face a roll cutter).
//! * Cutter: X along the feed direction, Y across the carriage, origin at the
//!   cutter's home position. `X = sheet_x`, `Y = material_width - sheet_y`.
//!   This is a right-handed mapping, so the result is not mirrored.

use crate::geometry::{flatten, parse_d, Affine, BBox, Polyline, Pt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPath {
    pub d: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobObject {
    pub paths: Vec<JobPath>,
    /// Local (mm) -> sheet (mm).
    pub transform: Affine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SortMode {
    /// Keep document order.
    None,
    /// Nearest neighbour (shortest travel).
    #[default]
    Nearest,
    /// Work in bands along the material length (less back-and-forth feeding
    /// on roll cutters, which reduces tracking drift on long jobs).
    Bands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Placement {
    /// Shapes cut exactly where they are on the sheet ("Whole area").
    #[default]
    AsPlaced,
    /// Job bounding box moved to the origin ("Optimized").
    Origin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AfterCut {
    /// Return the head to the origin.
    #[default]
    ReturnToOrigin,
    /// Advance the material past the end of the job.
    FeedPastJob,
    /// Leave the head where it finished.
    Stay,
}

/// Per-colour overrides ("Set Attributes" in SignCut).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LayerSettings {
    pub enabled: bool,
    pub passes: Option<u32>,
    pub speed: Option<f64>,
    pub force: Option<f64>,
    pub overcut: Option<f64>,
    pub blade_offset: Option<f64>,
    pub tool: Option<u32>,
    /// Stop and wait for the user before this colour (tool / material change).
    pub pause_before: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CutSettings {
    /// Material (roll) width in mm = sheet height in the UI.
    pub material_width: f64,
    pub blade_offset: f64,
    pub use_blade_offset: bool,
    /// Turn the blade into the cut direction before lowering it fully.
    pub tangential_emulation: bool,
    pub overcut: f64,
    pub passes: u32,
    pub speed: Option<f64>,
    pub force: Option<f64>,
    pub tool: u32,
    pub sort: SortMode,
    pub band_width: f64,
    pub inside_first: bool,
    pub mirror: bool,
    pub placement: Placement,
    /// Distance from the origin when `placement == Origin`.
    pub margin: f64,
    /// Rectangle around the job, at this distance (mm). None = off.
    pub weed_border: Option<f64>,
    pub copies: u32,
    pub copy_gap: f64,
    /// Fill copies across the material width before advancing along the length.
    pub stack_copies: bool,
    pub after_cut: AfterCut,
    pub feed_extra: f64,
    /// Max chord deviation when flattening curves (mm).
    pub curve_tolerance: f64,
    /// Colour order and overrides. Colours not listed are cut last with defaults.
    pub layers: Vec<(String, LayerSettings)>,
}

impl Default for CutSettings {
    fn default() -> Self {
        Self {
            material_width: 600.0,
            blade_offset: 0.25,
            use_blade_offset: true,
            tangential_emulation: true,
            overcut: 1.0,
            passes: 1,
            speed: None,
            force: None,
            tool: 1,
            sort: SortMode::Nearest,
            band_width: 200.0,
            inside_first: true,
            mirror: false,
            placement: Placement::AsPlaced,
            margin: 0.0,
            weed_border: None,
            copies: 1,
            copy_gap: 5.0,
            stack_copies: true,
            // SignCut defaults: "End after job" with no extra feed.
            after_cut: AfterCut::FeedPastJob,
            feed_extra: 0.0,
            curve_tolerance: 0.05,
            layers: vec![],
        }
    }
}

/// One machine operation in cutter millimetres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "op")]
pub enum Op {
    /// Tool/speed/force for the following cuts.
    Tool {
        tool: u32,
        speed: Option<f64>,
        force: Option<f64>,
    },
    /// Wait for the user (tool change).
    Pause { message: String },
    /// Lift, travel to pts[0], lower, cut through all points.
    Cut { pts: Vec<Pt>, color: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStats {
    pub paths: usize,
    pub cut_length_mm: f64,
    pub travel_length_mm: f64,
    /// Job extent in cutter coordinates.
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub ops: Vec<Op>,
    pub stats: PlanStats,
    /// Where the job ends (for feed-after-cut).
    pub end_x: f64,
}

struct Item {
    pl: Polyline,
    color: String,
    area: f64,
    bbox: BBox,
}

/// Build the cut plan.
pub fn plan(objects: &[JobObject], s: &CutSettings, max_width: Option<f64>) -> Plan {
    let mut warnings = Vec::new();
    let w = s.material_width;
    let enabled = |c: &str| {
        s.layers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(c))
            .map(|(_, l)| l.enabled)
            .unwrap_or(true)
    };
    // 1. Flatten into cutter coordinates.
    let to_cutter = Affine([1.0, 0.0, 0.0, -1.0, 0.0, w]);
    let mut items: Vec<Item> = Vec::new();
    for obj in objects {
        let m = obj.transform.then(&to_cutter);
        for p in &obj.paths {
            if !enabled(&p.color) {
                continue;
            }
            let segs = crate::geometry::transform_segs(&parse_d(&p.d), &m);
            for mut pl in flatten(&segs, s.curve_tolerance) {
                pl.dedup(0.005);
                if pl.pts.len() < 2 {
                    continue;
                }
                if pl.closed && pl.pts.len() < 3 {
                    pl.closed = false;
                }
                items.push(Item {
                    area: pl.signed_area().abs(),
                    bbox: pl.bbox(),
                    color: p.color.to_ascii_lowercase(),
                    pl,
                });
            }
        }
    }
    if items.is_empty() {
        return Plan {
            stats: PlanStats {
                warnings: vec!["Nothing to cut".into()],
                ..Default::default()
            },
            ..Default::default()
        };
    }
    let mut job = BBox::EMPTY;
    for it in &items {
        job.union(&it.bbox);
    }

    // 2. Mirror (heat-transfer vinyl is cut mirrored).
    if s.mirror {
        let cx = job.min.x + job.max.x;
        for it in &mut items {
            for p in &mut it.pl.pts {
                p.x = cx - p.x;
            }
            it.bbox = it.pl.bbox();
        }
    }

    // 3. Weed border.
    if let Some(d) = s.weed_border.filter(|d| *d >= 0.0) {
        let r = job.rect();
        let (x0, y0, x1, y1) = (r.x - d, r.y - d, r.max_x() + d, r.max_y() + d);
        let pl = Polyline {
            pts: vec![Pt::new(x0, y0), Pt::new(x1, y0), Pt::new(x1, y1), Pt::new(x0, y1)],
            closed: true,
        };
        job.add(Pt::new(x0, y0));
        job.add(Pt::new(x1, y1));
        items.push(Item {
            area: pl.signed_area().abs(),
            bbox: pl.bbox(),
            color: "weed-border".into(),
            pl,
        });
    }

    // 4. Placement.
    // Like SignCut, keep the blade offset clear of the origin so the
    // compensated tool path never goes negative.
    let blade_clear = if s.use_blade_offset { s.blade_offset.max(0.0) } else { 0.0 };
    let shift = match s.placement {
        Placement::Origin => Pt::new(
            s.margin + blade_clear - job.min.x,
            s.margin + blade_clear - job.min.y,
        ),
        Placement::AsPlaced => Pt::default(),
    };
    if shift != Pt::default() {
        for it in &mut items {
            for p in &mut it.pl.pts {
                *p = p.add(shift);
            }
            it.bbox = it.pl.bbox();
        }
        job = BBox {
            min: job.min.add(shift),
            max: job.max.add(shift),
        };
    }

    // 5. Copies.
    let copies = s.copies.max(1) as usize;
    if copies > 1 {
        let r = job.rect();
        let avail = max_width.unwrap_or(w).min(w);
        let per_col = if s.stack_copies {
            (((avail - r.y).max(r.h) + s.copy_gap) / (r.h + s.copy_gap)).floor().max(1.0) as usize
        } else {
            1
        };
        let base: Vec<Item> = items.drain(..).collect();
        for k in 0..copies {
            let col = k / per_col;
            let row = k % per_col;
            let off = Pt::new(col as f64 * (r.w + s.copy_gap), row as f64 * (r.h + s.copy_gap));
            for it in &base {
                let pts: Vec<Pt> = it.pl.pts.iter().map(|p| p.add(off)).collect();
                let pl = Polyline { pts, closed: it.pl.closed };
                items.push(Item {
                    bbox: pl.bbox(),
                    area: it.area,
                    color: it.color.clone(),
                    pl,
                });
            }
        }
    }

    // 6. Bounds checks.
    let mut ext = BBox::EMPTY;
    for it in &items {
        ext.union(&it.bbox);
    }
    if ext.min.x < -0.01 || ext.min.y < -0.01 {
        warnings.push("Part of the job lies outside the material (before the origin); it will be clipped by the cutter.".into());
    }
    if ext.max.y > w + 0.01 {
        warnings.push(format!(
            "Job is {:.1} mm wide but the material is only {:.1} mm.",
            ext.max.y, w
        ));
    }
    if let Some(mw) = max_width {
        if ext.max.y > mw + 0.01 {
            warnings.push(format!(
                "Job exceeds the cutter's maximum cutting width of {mw:.0} mm."
            ));
        }
    }

    // 7. Group by colour order.
    let mut order: Vec<String> = s.layers.iter().map(|(c, _)| c.to_ascii_lowercase()).collect();
    for it in &items {
        if !order.contains(&it.color) {
            order.push(it.color.clone());
        }
    }
    // The weed border always goes last.
    if let Some(pos) = order.iter().position(|c| c == "weed-border") {
        let c = order.remove(pos);
        order.push(c);
    }
    let layer_of = |c: &str| -> LayerSettings {
        s.layers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(c))
            .map(|(_, l)| l.clone())
            .unwrap_or(LayerSettings {
                enabled: true,
                ..Default::default()
            })
    };

    let mut ops = Vec::new();
    let mut pos = Pt::new(0.0, 0.0);
    let mut blade_dir: Option<Pt> = Some(Pt::new(1.0, 0.0));
    let mut cut_len = 0.0;
    let mut travel = 0.0;
    let mut npaths = 0;
    let mut last_tool: Option<(u32, Option<f64>, Option<f64>)> = None;
    let mut first_layer = true;
    let mut by_color: HashMap<String, Vec<Item>> = HashMap::new();
    for it in items {
        by_color.entry(it.color.clone()).or_default().push(it);
    }
    for color in order {
        let Some(group) = by_color.remove(&color) else { continue };
        let layer = layer_of(&color);
        if layer.pause_before && !first_layer {
            ops.push(Op::Pause {
                message: format!("Change tool or material for colour {color}"),
            });
        }
        first_layer = false;
        let tool = (
            layer.tool.unwrap_or(s.tool),
            layer.speed.or(s.speed),
            layer.force.or(s.force),
        );
        if last_tool != Some(tool) {
            ops.push(Op::Tool {
                tool: tool.0,
                speed: tool.1,
                force: tool.2,
            });
            last_tool = Some(tool);
        }
        let passes = layer.passes.unwrap_or(s.passes).max(1);
        let overcut = layer.overcut.unwrap_or(s.overcut).max(0.0);
        let offset = if s.use_blade_offset {
            layer.blade_offset.unwrap_or(s.blade_offset).max(0.0)
        } else {
            0.0
        };
        let ordered = order_items(group, s, pos);
        for (pl, _) in ordered {
            let mut pts = if pl.closed {
                with_overcut(&pl.pts, overcut)
            } else {
                pl.pts.clone()
            };
            if pts.len() < 2 {
                continue;
            }
            let single_len = Polyline { pts: pts.clone(), closed: false }.length();
            if passes > 1 {
                let base = pts.clone();
                for k in 1..passes {
                    if pl.closed {
                        // Continue around the loop without lifting.
                        let loop_pts: Vec<Pt> = pl.pts.iter().copied().chain(std::iter::once(pl.pts[0])).collect();
                        pts.extend(loop_pts.into_iter().skip(1));
                    } else if k % 2 == 1 {
                        pts.extend(base.iter().rev().skip(1));
                    } else {
                        pts.extend(base.iter().skip(1));
                    }
                }
                if pl.closed && overcut > 0.0 {
                    // Re-apply overcut at the very end.
                    let tail = with_overcut(&pl.pts, overcut);
                    let extra: Vec<Pt> = tail.iter().skip(pl.pts.len() + 1).copied().collect();
                    pts.extend(extra);
                }
            }
            let final_pts = if offset > 0.0 {
                let (c, dir) = compensate(&pts, offset, if s.tangential_emulation { blade_dir } else { None });
                blade_dir = dir;
                c
            } else {
                pts
            };
            travel += pos.dist(final_pts[0]);
            cut_len += single_len * passes as f64;
            pos = *final_pts.last().unwrap();
            npaths += 1;
            ops.push(Op::Cut {
                pts: final_pts,
                color: color.clone(),
            });
        }
    }

    let mut ext2 = BBox::EMPTY;
    for op in &ops {
        if let Op::Cut { pts, .. } = op {
            for p in pts {
                ext2.add(*p);
            }
        }
    }
    Plan {
        end_x: ext2.max.x,
        ops,
        stats: PlanStats {
            paths: npaths,
            cut_length_mm: cut_len,
            travel_length_mm: travel,
            min_x: ext2.min.x,
            min_y: ext2.min.y,
            max_x: ext2.max.x,
            max_y: ext2.max.y,
            warnings,
        },
    }
}

/// Order paths: inner shapes before the shapes containing them, then by the
/// chosen sort mode. Closed paths get their start point rotated to the vertex
/// closest to the previous end; open paths may be reversed.
fn order_items(items: Vec<Item>, s: &CutSettings, start: Pt) -> Vec<(Polyline, String)> {
    let n = items.len();
    // children_left[i] = number of not-yet-cut paths contained in i.
    let mut parents: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut children_left = vec![0usize; n];
    if s.inside_first {
        // Sort indices by area so containment checks only go small -> large.
        let mut idx: Vec<usize> = (0..n).collect();
        idx.sort_by(|a, b| items[*a].area.partial_cmp(&items[*b].area).unwrap());
        for (k, &i) in idx.iter().enumerate() {
            let a = &items[i];
            for &j in &idx[k + 1..] {
                let b = &items[j];
                if !b.pl.closed || b.area <= a.area {
                    continue;
                }
                let (bb, ab) = (b.bbox.rect(), a.bbox.rect());
                if bb.contains_rect(&ab) && b.pl.contains_point(a.pl.pts[0]) {
                    parents[i].push(j);
                    children_left[j] += 1;
                }
            }
        }
    }
    let mut done = vec![false; n];
    let mut out = Vec::with_capacity(n);
    let mut pos = start;
    let band = |x: f64| -> i64 {
        if s.sort == SortMode::Bands && s.band_width > 1.0 {
            (x / s.band_width).floor() as i64
        } else {
            0
        }
    };
    for step in 0..n {
        let mut best: Option<(usize, f64, usize, bool)> = None; // idx, cost, start vertex, reversed
        for i in 0..n {
            if done[i] || children_left[i] > 0 {
                continue;
            }
            if s.sort == SortMode::None {
                best = Some((i, 0.0, 0, false));
                break;
            }
            let it = &items[i];
            let (d, v, rev) = if it.pl.closed {
                let (v, d) = it
                    .pl
                    .pts
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (k, p.dist(pos)))
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
                    .unwrap();
                (d, v, false)
            } else {
                let d0 = it.pl.pts[0].dist(pos);
                let d1 = it.pl.pts.last().unwrap().dist(pos);
                if d1 < d0 {
                    (d1, 0, true)
                } else {
                    (d0, 0, false)
                }
            };
            let cost = band(it.bbox.min.x) as f64 * 1e9 + d;
            if best.map_or(true, |b| cost < b.1) {
                best = Some((i, cost, v, rev));
            }
        }
        let Some((i, _, v, rev)) = best else {
            // Cycle guard (should not happen): take any remaining.
            let i = (0..n).find(|i| !done[*i]).unwrap();
            children_left[i] = 0;
            let _ = step;
            continue;
        };
        done[i] = true;
        for &p in &parents[i] {
            children_left[p] = children_left[p].saturating_sub(1);
        }
        let it = &items[i];
        let mut pl = it.pl.clone();
        if pl.closed && s.sort != SortMode::None {
            pl.pts.rotate_left(v);
        }
        if rev {
            pl.pts.reverse();
        }
        pos = if pl.closed { pl.pts[0] } else { *pl.pts.last().unwrap() };
        out.push((pl, it.color.clone()));
    }
    // Any leftovers from the cycle guard.
    for i in 0..n {
        if !done[i] {
            out.push((items[i].pl.clone(), items[i].color.clone()));
        }
    }
    out
}

/// Closed loop as an open point list returning to the start, plus `overcut`
/// mm continuing along the path.
pub fn with_overcut(loop_pts: &[Pt], overcut: f64) -> Vec<Pt> {
    let mut v: Vec<Pt> = loop_pts.to_vec();
    if v.is_empty() {
        return v;
    }
    v.push(loop_pts[0]);
    let mut left = overcut;
    let n = loop_pts.len();
    let mut i = 0;
    while left > 1e-9 && i < n * 4 {
        let a = loop_pts[i % n];
        let b = loop_pts[(i + 1) % n];
        let l = a.dist(b);
        if l >= left {
            v.push(a.lerp(b, left / l));
            break;
        }
        v.push(b);
        left -= l;
        i += 1;
    }
    v
}

/// Drag-knife (blade offset) compensation.
///
/// The blade tip trails the tool axis by `r`. To make the tip follow `pts`,
/// the axis is driven `r` ahead along the direction of travel, and swivelled
/// around each corner point on an arc of radius `r`.
///
/// `prev_dir` is the direction the blade was pointing at the end of the
/// previous cut; when given, the first move swivels the blade from that
/// direction into the new one ("tangential emulation").
pub fn compensate(pts: &[Pt], r: f64, prev_dir: Option<Pt>) -> (Vec<Pt>, Option<Pt>) {
    // Drop zero-length segments.
    let mut p: Vec<Pt> = Vec::with_capacity(pts.len());
    for q in pts {
        if p.last().map_or(true, |l: &Pt| l.dist(*q) > 1e-6) {
            p.push(*q);
        }
    }
    if p.len() < 2 || r <= 0.0 {
        return (p, prev_dir);
    }
    let dirs: Vec<Pt> = p.windows(2).map(|w| w[1].sub(w[0]).normalized()).collect();
    let mut out = Vec::with_capacity(p.len() * 2);
    let d0 = dirs[0];
    match prev_dir {
        Some(pd) if angle_between(pd, d0).abs() > 0.05 => {
            out.push(p[0].add(pd.scale(r)));
            arc(&mut out, p[0], pd, d0, r);
        }
        _ => out.push(p[0].add(d0.scale(r))),
    }
    for i in 1..p.len() - 1 {
        let (a, b) = (dirs[i - 1], dirs[i]);
        push_unique(&mut out, p[i].add(a.scale(r)));
        arc(&mut out, p[i], a, b, r);
    }
    let last = *dirs.last().unwrap();
    push_unique(&mut out, p[p.len() - 1].add(last.scale(r)));
    (out, Some(last))
}

fn push_unique(v: &mut Vec<Pt>, p: Pt) {
    if v.last().map_or(true, |l| l.dist(p) > 1e-6) {
        v.push(p);
    }
}

fn angle_between(a: Pt, b: Pt) -> f64 {
    a.cross(b).atan2(a.dot(b))
}

/// Arc around `c` from direction `a` to `b` (shortest way), radius `r`,
/// appending intermediate points and the end point.
fn arc(out: &mut Vec<Pt>, c: Pt, a: Pt, b: Pt, r: f64) {
    let total = angle_between(a, b);
    let step = 10f64.to_radians();
    let n = (total.abs() / step).ceil() as usize;
    let a0 = a.y.atan2(a.x);
    for k in 1..n {
        let t = a0 + total * k as f64 / n as f64;
        push_unique(out, Pt::new(c.x + r * t.cos(), c.y + r * t.sin()));
    }
    push_unique(out, c.add(b.scale(r)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64, color: &str) -> JobPath {
        JobPath {
            d: format!("M{x} {y}h{s}v{s}h-{s}z"),
            color: color.into(),
        }
    }

    fn obj(paths: Vec<JobPath>) -> JobObject {
        JobObject {
            paths,
            transform: Affine::IDENTITY,
        }
    }

    fn cuts(p: &Plan) -> Vec<&Vec<Pt>> {
        p.ops
            .iter()
            .filter_map(|o| match o {
                Op::Cut { pts, .. } => Some(pts),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sheet_to_cutter_mapping() {
        let s = CutSettings {
            material_width: 100.0,
            use_blade_offset: false,
            overcut: 0.0,
            ..Default::default()
        };
        // Square near the bottom-left of the sheet -> near the cutter origin.
        let p = plan(&[obj(vec![square(10.0, 80.0, 10.0, "#000")])], &s, None);
        assert_eq!(p.stats.paths, 1);
        assert!((p.stats.min_x - 10.0).abs() < 1e-9);
        assert!((p.stats.min_y - 10.0).abs() < 1e-9);
        assert!((p.stats.max_y - 20.0).abs() < 1e-9);
    }

    #[test]
    fn inner_shapes_first() {
        let s = CutSettings {
            material_width: 100.0,
            use_blade_offset: false,
            overcut: 0.0,
            ..Default::default()
        };
        let p = plan(
            &[obj(vec![square(0.0, 0.0, 50.0, "#000"), square(10.0, 10.0, 5.0, "#000")])],
            &s,
            None,
        );
        let c = cuts(&p);
        assert_eq!(c.len(), 2);
        let first = Polyline { pts: c[0].clone(), closed: false }.bbox().rect();
        assert!(first.w < 6.0, "inner square must be cut first");
    }

    #[test]
    fn overcut_extends_loop() {
        let pts = vec![Pt::new(0.0, 0.0), Pt::new(10.0, 0.0), Pt::new(10.0, 10.0), Pt::new(0.0, 10.0)];
        let v = with_overcut(&pts, 2.0);
        assert_eq!(v.len(), 6);
        assert_eq!(v[4], Pt::new(0.0, 0.0));
        assert!((v[5].x - 2.0).abs() < 1e-9 && v[5].y.abs() < 1e-9);
    }

    #[test]
    fn blade_compensation_square_corner() {
        let pts = vec![Pt::new(0.0, 0.0), Pt::new(10.0, 0.0), Pt::new(10.0, 10.0)];
        let (c, dir) = compensate(&pts, 0.5, None);
        // Starts 0.5 ahead.
        assert_eq!(c[0], Pt::new(0.5, 0.0));
        // Reaches past the corner, then swivels around it.
        assert!(c.contains(&Pt::new(10.5, 0.0)));
        // Every arc point is 0.5 from the corner.
        let arc_pts: Vec<_> = c.iter().filter(|p| p.x > 10.0 && p.y > 0.0 && p.y < 0.5).collect();
        assert!(!arc_pts.is_empty());
        for p in arc_pts {
            assert!((p.dist(Pt::new(10.0, 0.0)) - 0.5).abs() < 1e-9);
        }
        assert_eq!(*c.last().unwrap(), Pt::new(10.0, 10.5));
        assert_eq!(dir, Some(Pt::new(0.0, 1.0)));
    }

    #[test]
    fn mirror_and_origin_placement() {
        let s = CutSettings {
            material_width: 100.0,
            use_blade_offset: false,
            overcut: 0.0,
            mirror: true,
            placement: Placement::Origin,
            margin: 5.0,
            ..Default::default()
        };
        let p = plan(
            &[obj(vec![JobPath { d: "M50 50L70 50L70 60".into(), color: "#000".into() }])],
            &s,
            None,
        );
        assert!((p.stats.min_x - 5.0).abs() < 1e-9 && (p.stats.min_y - 5.0).abs() < 1e-9);
        // The vertical leg is at the left after mirroring.
        let c = cuts(&p);
        let vert_x = c[0].iter().filter(|q| (q.y - 5.0).abs() < 1e-9 || (q.y - 15.0).abs() < 1e-9).map(|q| q.x).fold(f64::INFINITY, f64::min);
        assert!((vert_x - 5.0).abs() < 1e-9);
    }

    #[test]
    fn copies_and_weed_border() {
        let s = CutSettings {
            material_width: 100.0,
            use_blade_offset: false,
            overcut: 0.0,
            placement: Placement::Origin,
            copies: 4,
            copy_gap: 5.0,
            weed_border: Some(2.0),
            ..Default::default()
        };
        let p = plan(&[obj(vec![square(0.0, 0.0, 20.0, "#000")])], &s, None);
        // 4 squares + 4 borders.
        assert_eq!(p.stats.paths, 8);
        // Borders are 24 mm; 3 stacked across 100 mm, 4th in the next column.
        assert!((p.stats.max_y - (3.0 * 24.0 + 2.0 * 5.0)).abs() < 1e-6, "{}", p.stats.max_y);
        assert!(p.stats.max_x > 40.0);
    }

    #[test]
    fn disabled_layer_is_skipped() {
        let mut s = CutSettings {
            material_width: 100.0,
            ..Default::default()
        };
        s.layers.push(("#ff0000".into(), LayerSettings { enabled: false, ..Default::default() }));
        let p = plan(
            &[obj(vec![square(0.0, 0.0, 10.0, "#FF0000"), square(20.0, 0.0, 10.0, "#000000")])],
            &s,
            None,
        );
        assert_eq!(p.stats.paths, 1);
    }
}
