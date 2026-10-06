//! DXF import (lines, polylines with bulges, circles, arcs, ellipses, splines, block inserts).

use super::{ImportedDesign, ImportedPath};
use crate::geometry::{segs_to_d, Affine, Pt, Seg};
use ::dxf::entities::{Entity, EntityType};
use ::dxf::enums::Units;
use ::dxf::Drawing;
use std::f64::consts::TAU;

pub fn import_dxf(data: &[u8], name: &str) -> Result<ImportedDesign, String> {
    let mut cursor = std::io::Cursor::new(data);
    let drawing = Drawing::load(&mut cursor).map_err(|e| format!("Invalid DXF: {e}"))?;
    let unit = match drawing.header.default_drawing_units {
        Units::Inches => 25.4,
        Units::Feet => 304.8,
        Units::Centimeters => 10.0,
        Units::Meters => 1000.0,
        Units::Microns => 0.001,
        _ => 1.0, // unitless / millimetres
    };
    // DXF Y is up; flip to screen coordinates.
    let base = Affine([unit, 0.0, 0.0, -unit, 0.0, 0.0]);
    let mut paths = Vec::new();
    let mut warnings = Vec::new();
    for e in drawing.entities() {
        entity_paths(&drawing, e, &base, 0, &mut paths, &mut warnings);
    }
    let mut d = ImportedDesign::new(name, paths);
    warnings.sort();
    warnings.dedup();
    d.warnings = warnings;
    Ok(d)
}

fn aci_color(c: &::dxf::Color) -> String {
    match c.index() {
        Some(1) => "#e00000".into(),
        Some(2) => "#c8b400".into(),
        Some(3) => "#00a000".into(),
        Some(4) => "#00a0a0".into(),
        Some(5) => "#0000e0".into(),
        Some(6) => "#c000c0".into(),
        _ => "#000000".into(),
    }
}

fn push(paths: &mut Vec<ImportedPath>, segs: Vec<Seg>, color: String) {
    if segs.len() >= 2 {
        paths.push(ImportedPath {
            d: segs_to_d(&segs),
            color,
            filled: false,
        });
    }
}

fn entity_paths(
    drawing: &Drawing,
    e: &Entity,
    m: &Affine,
    depth: usize,
    paths: &mut Vec<ImportedPath>,
    warnings: &mut Vec<String>,
) {
    let color = aci_color(&e.common.color);
    let p = |x: f64, y: f64| m.apply(Pt::new(x, y));
    match &e.specific {
        EntityType::Line(l) => push(
            paths,
            vec![Seg::M(p(l.p1.x, l.p1.y)), Seg::L(p(l.p2.x, l.p2.y))],
            color,
        ),
        EntityType::LwPolyline(pl) => {
            let verts: Vec<(f64, f64, f64)> = pl.vertices.iter().map(|v| (v.x, v.y, v.bulge)).collect();
            push(paths, bulge_polyline(&verts, pl.flags & 1 != 0, m), color);
        }
        EntityType::Polyline(pl) => {
            let verts: Vec<(f64, f64, f64)> = pl
                .vertices()
                .map(|v| (v.location.x, v.location.y, v.bulge))
                .collect();
            push(paths, bulge_polyline(&verts, pl.flags & 1 != 0, m), color);
        }
        EntityType::Circle(c) => {
            push(paths, arc_segs(c.center.x, c.center.y, c.radius, 0.0, TAU, true, m), color);
        }
        EntityType::Arc(a) => {
            let s = a.start_angle.to_radians();
            let mut e2 = a.end_angle.to_radians();
            if e2 <= s {
                e2 += TAU;
            }
            push(paths, arc_segs(a.center.x, a.center.y, a.radius, s, e2, false, m), color);
        }
        EntityType::Ellipse(el) => {
            let (cx, cy) = (el.center.x, el.center.y);
            let (ax, ay) = (el.major_axis.x, el.major_axis.y);
            let (bx, by) = (-ay * el.minor_axis_ratio, ax * el.minor_axis_ratio);
            let mut t0 = el.start_parameter;
            let mut t1 = el.end_parameter;
            if (t1 - t0).abs() < 1e-9 {
                t0 = 0.0;
                t1 = TAU;
            }
            if t1 < t0 {
                t1 += TAU;
            }
            let closed = (t1 - t0 - TAU).abs() < 1e-6;
            let n = (((t1 - t0) / TAU) * 128.0).ceil().max(8.0) as usize;
            let mut segs = Vec::new();
            for i in 0..=n {
                let t = t0 + (t1 - t0) * i as f64 / n as f64;
                let (s, c) = t.sin_cos();
                let pt = p(cx + ax * c + bx * s, cy + ay * c + by * s);
                segs.push(if i == 0 { Seg::M(pt) } else { Seg::L(pt) });
            }
            if closed {
                segs.pop();
                segs.push(Seg::Z);
            }
            push(paths, segs, color);
        }
        EntityType::Spline(sp) => {
            let pts = eval_spline(sp);
            if pts.len() >= 2 {
                let mut segs = vec![Seg::M(p(pts[0].x, pts[0].y))];
                segs.extend(pts[1..].iter().map(|q| Seg::L(p(q.x, q.y))));
                if sp.flags & 1 != 0 {
                    segs.push(Seg::Z);
                }
                push(paths, segs, color);
            }
        }
        EntityType::Insert(ins) => {
            if depth > 8 {
                return;
            }
            if let Some(block) = drawing.blocks().find(|b| b.name == ins.name) {
                let t = Affine::translate(-block.base_point.x, -block.base_point.y)
                    .then(&Affine::scale(ins.x_scale_factor, ins.y_scale_factor))
                    .then(&Affine::rotate_deg(ins.rotation))
                    .then(&Affine::translate(ins.location.x, ins.location.y))
                    .then(m);
                for be in &block.entities {
                    entity_paths(drawing, be, &t, depth + 1, paths, warnings);
                }
            }
        }
        EntityType::Text(_) | EntityType::MText(_) => {
            warnings.push("DXF text entities are skipped; convert text to curves in your CAD program or use the Text tool".into());
        }
        EntityType::Solid(s) => {
            let segs = vec![
                Seg::M(p(s.first_corner.x, s.first_corner.y)),
                Seg::L(p(s.second_corner.x, s.second_corner.y)),
                Seg::L(p(s.fourth_corner.x, s.fourth_corner.y)),
                Seg::L(p(s.third_corner.x, s.third_corner.y)),
                Seg::Z,
            ];
            push(paths, segs, color);
        }
        _ => {}
    }
}

fn bulge_polyline(verts: &[(f64, f64, f64)], closed: bool, m: &Affine) -> Vec<Seg> {
    let mut segs = Vec::new();
    if verts.is_empty() {
        return segs;
    }
    segs.push(Seg::M(m.apply(Pt::new(verts[0].0, verts[0].1))));
    let n = verts.len();
    let count = if closed { n } else { n - 1 };
    for i in 0..count {
        let (x0, y0, bulge) = verts[i];
        let (x1, y1, _) = verts[(i + 1) % n];
        if bulge.abs() < 1e-9 {
            segs.push(Seg::L(m.apply(Pt::new(x1, y1))));
        } else {
            // Bulge = tan(theta/4); positive = counter-clockwise.
            let theta = 4.0 * bulge.atan();
            let chord = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
            let r = chord / (2.0 * (theta / 2.0).sin());
            let mx = (x0 + x1) / 2.0;
            let my = (y0 + y1) / 2.0;
            let h = r * (theta / 2.0).cos();
            let (dx, dy) = ((x1 - x0) / chord, (y1 - y0) / chord);
            // Centre is left of the chord for positive bulge.
            let cx = mx - dy * h;
            let cy = my + dx * h;
            let a0 = (y0 - cy).atan2(x0 - cx);
            let steps = ((theta.abs() / TAU) * 96.0).ceil().max(2.0) as usize;
            for k in 1..=steps {
                let a = a0 + theta * k as f64 / steps as f64;
                let rr = r.abs();
                segs.push(Seg::L(m.apply(Pt::new(cx + rr * a.cos(), cy + rr * a.sin()))));
            }
        }
    }
    if closed {
        segs.pop();
        segs.push(Seg::Z);
    }
    segs
}

fn arc_segs(cx: f64, cy: f64, r: f64, a0: f64, a1: f64, closed: bool, m: &Affine) -> Vec<Seg> {
    let n = (((a1 - a0) / TAU) * 128.0).ceil().max(4.0) as usize;
    let mut segs = Vec::with_capacity(n + 2);
    for i in 0..=n {
        let a = a0 + (a1 - a0) * i as f64 / n as f64;
        let pt = m.apply(Pt::new(cx + r * a.cos(), cy + r * a.sin()));
        segs.push(if i == 0 { Seg::M(pt) } else { Seg::L(pt) });
    }
    if closed {
        segs.pop();
        segs.push(Seg::Z);
    }
    segs
}

/// Evaluate a (rational) B-spline with de Boor's algorithm; falls back to fit points.
fn eval_spline(sp: &::dxf::entities::Spline) -> Vec<Pt> {
    let cps: Vec<Pt> = sp.control_points.iter().map(|p| Pt::new(p.x, p.y)).collect();
    let k = sp.degree_of_curve.max(1) as usize;
    let knots = &sp.knot_values;
    if cps.len() <= k || knots.len() != cps.len() + k + 1 {
        return sp.fit_points.iter().map(|p| Pt::new(p.x, p.y)).collect();
    }
    let w: Vec<f64> = if sp.weight_values.len() == cps.len() {
        sp.weight_values.clone()
    } else {
        vec![1.0; cps.len()]
    };
    let t0 = knots[k];
    let t1 = knots[cps.len()];
    let n = (cps.len() * 16).clamp(32, 4000);
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        // find span
        let mut s = k;
        while s < cps.len() - 1 && t >= knots[s + 1] {
            s += 1;
        }
        let mut d: Vec<(f64, f64, f64)> = (0..=k)
            .map(|j| {
                let c = cps[j + s - k];
                let ww = w[j + s - k];
                (c.x * ww, c.y * ww, ww)
            })
            .collect();
        for r in 1..=k {
            for j in (r..=k).rev() {
                let i0 = j + s - k;
                let den = knots[i0 + k + 1 - r] - knots[i0];
                let a = if den.abs() < 1e-12 { 0.0 } else { (t - knots[i0]) / den };
                d[j] = (
                    (1.0 - a) * d[j - 1].0 + a * d[j].0,
                    (1.0 - a) * d[j - 1].1 + a * d[j].1,
                    (1.0 - a) * d[j - 1].2 + a * d[j].2,
                );
            }
        }
        let (x, y, ww) = d[k];
        out.push(Pt::new(x / ww, y / ww));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxf_lwpolyline_and_circle() {
        let mut d = Drawing::new();
        d.header.default_drawing_units = Units::Millimeters;
        d.header.version = ::dxf::enums::AcadVersion::R2000;
        let mut pl = ::dxf::entities::LwPolyline::default();
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)] {
            pl.vertices.push(::dxf::LwPolylineVertex { x, y, ..Default::default() });
        }
        pl.flags = 1;
        d.add_entity(Entity::new(EntityType::LwPolyline(pl)));
        let mut c = ::dxf::entities::Circle::default();
        c.center = ::dxf::Point::new(20.0, 2.5, 0.0);
        c.radius = 2.5;
        d.add_entity(Entity::new(EntityType::Circle(c)));
        let mut buf = Vec::new();
        d.save(&mut buf).unwrap();
        let imp = import_dxf(&buf, "t").unwrap();
        assert_eq!(imp.paths.len(), 2);
        assert!((imp.width_mm - 22.5).abs() < 1e-6, "{}", imp.width_mm);
        assert!((imp.height_mm - 5.0).abs() < 1e-6);
    }
}
