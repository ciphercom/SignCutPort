//! Basic 2D geometry: points, affine transforms, path segments, flattening.
//!
//! All coordinates in the core are millimetres unless stated otherwise.

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
    pub fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
    pub fn scale(self, s: f64) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn dist(self, o: Pt) -> f64 {
        self.sub(o).len()
    }
    pub fn dot(self, o: Pt) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: Pt) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn normalized(self) -> Pt {
        let l = self.len();
        if l < 1e-12 {
            Pt::new(1.0, 0.0)
        } else {
            self.scale(1.0 / l)
        }
    }
    pub fn lerp(self, o: Pt, t: f64) -> Pt {
        Pt::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
}

/// Affine matrix `[a b c d e f]` mapping (x,y) -> (a*x + c*y + e, b*x + d*y + f),
/// identical to the SVG `matrix()` convention.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affine(pub [f64; 6]);

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn translate(x: f64, y: f64) -> Self {
        Affine([1.0, 0.0, 0.0, 1.0, x, y])
    }
    pub fn scale(sx: f64, sy: f64) -> Self {
        Affine([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }
    pub fn rotate_deg(deg: f64) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Affine([c, s, -s, c, 0.0, 0.0])
    }
    /// `self * other`: apply `other` first, then `self`.
    pub fn then_apply_after(&self, other: &Affine) -> Affine {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = other.0;
        Affine([
            a1 * a2 + c1 * b2,
            b1 * a2 + d1 * b2,
            a1 * c2 + c1 * d2,
            b1 * c2 + d1 * d2,
            a1 * e2 + c1 * f2 + e1,
            b1 * e2 + d1 * f2 + f1,
        ])
    }
    /// Returns the transform that applies `self` first and then `next`.
    pub fn then(&self, next: &Affine) -> Affine {
        next.then_apply_after(self)
    }
    pub fn apply(&self, p: Pt) -> Pt {
        let [a, b, c, d, e, f] = self.0;
        Pt::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }
    pub fn is_mirroring(&self) -> bool {
        let [a, b, c, d, _, _] = self.0;
        a * d - b * c < 0.0
    }
}

/// One drawing command of a path. Coordinates are absolute.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Seg {
    M(Pt),
    L(Pt),
    C(Pt, Pt, Pt),
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x - 1e-9
            && o.y >= self.y - 1e-9
            && o.max_x() <= self.max_x() + 1e-9
            && o.max_y() <= self.max_y() + 1e-9
    }
}

/// Bounding box accumulator.
#[derive(Debug, Clone, Copy)]
pub struct BBox {
    pub min: Pt,
    pub max: Pt,
}

impl Default for BBox {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl BBox {
    pub const EMPTY: BBox = BBox {
        min: Pt::new(f64::INFINITY, f64::INFINITY),
        max: Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    pub fn add(&mut self, p: Pt) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }
    pub fn union(&mut self, o: &BBox) {
        if !o.is_empty() {
            self.add(o.min);
            self.add(o.max);
        }
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn rect(&self) -> Rect {
        if self.is_empty() {
            return Rect::default();
        }
        Rect {
            x: self.min.x,
            y: self.min.y,
            w: self.max.x - self.min.x,
            h: self.max.y - self.min.y,
        }
    }
}

/// A polyline produced by flattening. `closed` means the last point connects
/// back to the first (the first point is NOT repeated at the end).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polyline {
    pub pts: Vec<Pt>,
    pub closed: bool,
}

impl Polyline {
    pub fn bbox(&self) -> BBox {
        let mut b = BBox::EMPTY;
        for p in &self.pts {
            b.add(*p);
        }
        b
    }
    pub fn length(&self) -> f64 {
        let mut l: f64 = self.pts.windows(2).map(|w| w[0].dist(w[1])).sum();
        if self.closed && self.pts.len() > 1 {
            l += self.pts[self.pts.len() - 1].dist(self.pts[0]);
        }
        l
    }
    /// Signed area (positive = counter-clockwise in a y-up system).
    pub fn signed_area(&self) -> f64 {
        let n = self.pts.len();
        if n < 3 {
            return 0.0;
        }
        let mut a = 0.0;
        for i in 0..n {
            let p = self.pts[i];
            let q = self.pts[(i + 1) % n];
            a += p.cross(q);
        }
        a * 0.5
    }
    /// Even-odd point in polygon test (treats the polyline as closed).
    pub fn contains_point(&self, p: Pt) -> bool {
        let n = self.pts.len();
        if n < 3 {
            return false;
        }
        let mut inside = false;
        let mut j = n - 1;
        for i in 0..n {
            let a = self.pts[i];
            let b = self.pts[j];
            if (a.y > p.y) != (b.y > p.y) {
                let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
                if p.x < x {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }
    /// Points as a list where closed polylines repeat the first point at the end.
    pub fn as_open_points(&self) -> Vec<Pt> {
        let mut v = self.pts.clone();
        if self.closed && !v.is_empty() {
            v.push(v[0]);
        }
        v
    }
    /// Remove consecutive points closer than `eps`.
    pub fn dedup(&mut self, eps: f64) {
        let mut out: Vec<Pt> = Vec::with_capacity(self.pts.len());
        for p in &self.pts {
            if out.last().map_or(true, |l: &Pt| l.dist(*p) > eps) {
                out.push(*p);
            }
        }
        if self.closed && out.len() > 1 && out[0].dist(*out.last().unwrap()) <= eps {
            out.pop();
        }
        self.pts = out;
    }
}

/// Parse a restricted SVG path data string (as produced by [`segs_to_d`], but
/// also accepting the full SVG grammar including relative commands, H/V, S/T,
/// Q and A) into absolute segments.
pub fn parse_d(d: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let tokens = tokenize(d);
    let mut i = 0;
    let mut cmd = 'M';
    let mut cur = Pt::default();
    let mut start = Pt::default();
    let mut last_ctrl: Option<Pt> = None; // for S
    let mut last_qctrl: Option<Pt> = None; // for T
    let num = |i: &mut usize| -> Option<f64> {
        match tokens.get(*i) {
            Some(Tok::Num(n)) => {
                *i += 1;
                Some(*n)
            }
            _ => None,
        }
    };
    while i < tokens.len() {
        if let Tok::Cmd(c) = tokens[i] {
            cmd = c;
            i += 1;
            if c == 'Z' || c == 'z' {
                out.push(Seg::Z);
                cur = start;
                last_ctrl = None;
                last_qctrl = None;
                continue;
            }
        } else if cmd == 'Z' || cmd == 'z' {
            // Numbers after Z without a command: treat as error, skip.
            i += 1;
            continue;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { Pt::default() };
        macro_rules! pt {
            () => {{
                let x = match num(&mut i) {
                    Some(v) => v,
                    None => break,
                };
                let y = match num(&mut i) {
                    Some(v) => v,
                    None => break,
                };
                Pt::new(base.x + x, base.y + y)
            }};
        }
        match cmd.to_ascii_uppercase() {
            'M' => {
                let p = pt!();
                out.push(Seg::M(p));
                cur = p;
                start = p;
                // Subsequent pairs are implicit LineTo.
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
                last_qctrl = None;
            }
            'L' => {
                let p = pt!();
                out.push(Seg::L(p));
                cur = p;
                last_ctrl = None;
                last_qctrl = None;
            }
            'H' => {
                let x = match num(&mut i) {
                    Some(v) => v,
                    None => break,
                };
                let p = Pt::new(if rel { cur.x + x } else { x }, cur.y);
                out.push(Seg::L(p));
                cur = p;
                last_ctrl = None;
                last_qctrl = None;
            }
            'V' => {
                let y = match num(&mut i) {
                    Some(v) => v,
                    None => break,
                };
                let p = Pt::new(cur.x, if rel { cur.y + y } else { y });
                out.push(Seg::L(p));
                cur = p;
                last_ctrl = None;
                last_qctrl = None;
            }
            'C' => {
                let c1 = pt!();
                let c2 = pt!();
                let p = pt!();
                out.push(Seg::C(c1, c2, p));
                cur = p;
                last_ctrl = Some(c2);
                last_qctrl = None;
            }
            'S' => {
                let c1 = match last_ctrl {
                    Some(c) => cur.scale(2.0).sub(c),
                    None => cur,
                };
                let c2 = pt!();
                let p = pt!();
                out.push(Seg::C(c1, c2, p));
                cur = p;
                last_ctrl = Some(c2);
                last_qctrl = None;
            }
            'Q' => {
                let q = pt!();
                let p = pt!();
                out.push(quad_to_cubic(cur, q, p));
                cur = p;
                last_qctrl = Some(q);
                last_ctrl = None;
            }
            'T' => {
                let q = match last_qctrl {
                    Some(c) => cur.scale(2.0).sub(c),
                    None => cur,
                };
                let p = pt!();
                out.push(quad_to_cubic(cur, q, p));
                cur = p;
                last_qctrl = Some(q);
                last_ctrl = None;
            }
            'A' => {
                let rx = num(&mut i);
                let ry = num(&mut i);
                let rot = num(&mut i);
                let large = num(&mut i);
                let sweep = num(&mut i);
                let (Some(rx), Some(ry), Some(rot), Some(large), Some(sweep)) =
                    (rx, ry, rot, large, sweep)
                else {
                    break;
                };
                let p = pt!();
                arc_to_cubics(cur, rx, ry, rot, large != 0.0, sweep != 0.0, p, &mut out);
                cur = p;
                last_ctrl = None;
                last_qctrl = None;
            }
            _ => {
                i += 1;
            }
        }
    }
    out
}

fn quad_to_cubic(p0: Pt, q: Pt, p: Pt) -> Seg {
    let c1 = p0.add(q.sub(p0).scale(2.0 / 3.0));
    let c2 = p.add(q.sub(p).scale(2.0 / 3.0));
    Seg::C(c1, c2, p)
}

#[allow(clippy::too_many_arguments)]
fn arc_to_cubics(
    p0: Pt,
    mut rx: f64,
    mut ry: f64,
    x_rot_deg: f64,
    large: bool,
    sweep: bool,
    p: Pt,
    out: &mut Vec<Seg>,
) {
    if p0.dist(p) < 1e-12 {
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    if rx < 1e-12 || ry < 1e-12 {
        out.push(Seg::L(p));
        return;
    }
    let phi = x_rot_deg.to_radians();
    let (sphi, cphi) = phi.sin_cos();
    let dx = (p0.x - p.x) / 2.0;
    let dy = (p0.y - p.y) / 2.0;
    let x1 = cphi * dx + sphi * dy;
    let y1 = -sphi * dx + cphi * dy;
    let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = (num / den).max(0.0).sqrt();
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let cx = cphi * cx1 - sphi * cy1 + (p0.x + p.x) / 2.0;
    let cy = sphi * cx1 + cphi * cy1 + (p0.y + p.y) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| -> f64 {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let th1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dth = ang(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    if !sweep && dth > 0.0 {
        dth -= std::f64::consts::TAU;
    } else if sweep && dth < 0.0 {
        dth += std::f64::consts::TAU;
    }
    let n = (dth.abs() / (std::f64::consts::FRAC_PI_2)).ceil().max(1.0) as usize;
    let step = dth / n as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let pt_at = |t: f64| -> (Pt, Pt) {
        let (st, ct) = t.sin_cos();
        let x = rx * ct;
        let y = ry * st;
        let dx = -rx * st;
        let dy = ry * ct;
        (
            Pt::new(cphi * x - sphi * y + cx, sphi * x + cphi * y + cy),
            Pt::new(cphi * dx - sphi * dy, sphi * dx + cphi * dy),
        )
    };
    let mut t = th1;
    for idx in 0..n {
        let (a, da) = pt_at(t);
        let (b, db) = pt_at(t + step);
        let c1 = a.add(da.scale(k));
        let c2 = b.sub(db.scale(k));
        let end = if idx == n - 1 { p } else { b };
        out.push(Seg::C(c1, c2, end));
        t += step;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Cmd(char),
    Num(f64),
}

fn tokenize(d: &str) -> Vec<Tok> {
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            out.push(Tok::Cmd(c));
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let s = i;
            i += 1;
            let mut seen_dot = c == '.';
            let mut seen_e = false;
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !seen_dot && !seen_e {
                    seen_dot = true;
                    i += 1;
                } else if (ch == 'e' || ch == 'E') && !seen_e {
                    seen_e = true;
                    i += 1;
                    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = d[s..i].parse::<f64>() {
                out.push(Tok::Num(v));
            }
        } else {
            i += 1;
        }
    }
    out
}

fn fmt_num(s: &mut String, v: f64) {
    // 4 decimals in mm = 0.1 µm, plenty.
    let r = (v * 10000.0).round() / 10000.0;
    let r = if r == 0.0 { 0.0 } else { r };
    let _ = write!(s, "{}", r);
}

/// Serialize segments to compact absolute SVG path data.
pub fn segs_to_d(segs: &[Seg]) -> String {
    let mut s = String::new();
    for seg in segs {
        match seg {
            Seg::M(p) => {
                s.push('M');
                fmt_num(&mut s, p.x);
                s.push(' ');
                fmt_num(&mut s, p.y);
            }
            Seg::L(p) => {
                s.push('L');
                fmt_num(&mut s, p.x);
                s.push(' ');
                fmt_num(&mut s, p.y);
            }
            Seg::C(a, b, p) => {
                s.push('C');
                for (k, q) in [a, b, p].iter().enumerate() {
                    if k > 0 {
                        s.push(' ');
                    }
                    fmt_num(&mut s, q.x);
                    s.push(' ');
                    fmt_num(&mut s, q.y);
                }
            }
            Seg::Z => s.push('Z'),
        }
    }
    s
}

pub fn transform_segs(segs: &[Seg], m: &Affine) -> Vec<Seg> {
    segs.iter()
        .map(|s| match *s {
            Seg::M(p) => Seg::M(m.apply(p)),
            Seg::L(p) => Seg::L(m.apply(p)),
            Seg::C(a, b, p) => Seg::C(m.apply(a), m.apply(b), m.apply(p)),
            Seg::Z => Seg::Z,
        })
        .collect()
}

/// Bounding box of segments (control-point hull; exact enough for layout).
pub fn segs_bbox(segs: &[Seg]) -> BBox {
    let mut b = BBox::EMPTY;
    for p in flatten(segs, 0.05).iter().flat_map(|pl| pl.pts.iter()) {
        b.add(*p);
    }
    b
}

/// Flatten segments into polylines with maximum chord deviation `tol` (mm).
pub fn flatten(segs: &[Seg], tol: f64) -> Vec<Polyline> {
    let tol = tol.max(1e-4);
    let mut out = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    let mut pos = Pt::default();
    let mut start = Pt::default();
    let finish = |cur: &mut Vec<Pt>, closed: bool, out: &mut Vec<Polyline>| {
        if cur.len() >= 2 {
            let mut pl = Polyline {
                pts: std::mem::take(cur),
                closed,
            };
            if closed && pl.pts.len() > 2 && pl.pts[0].dist(*pl.pts.last().unwrap()) < 1e-9 {
                pl.pts.pop();
            }
            // A "closed" path with only two distinct points is really a line.
            if pl.closed && pl.pts.len() < 3 {
                pl.closed = false;
            }
            out.push(pl);
        } else {
            cur.clear();
        }
    };
    for seg in segs {
        match *seg {
            Seg::M(p) => {
                finish(&mut cur, false, &mut out);
                cur.push(p);
                pos = p;
                start = p;
            }
            Seg::L(p) => {
                if cur.is_empty() {
                    cur.push(pos);
                }
                cur.push(p);
                pos = p;
            }
            Seg::C(a, b, p) => {
                if cur.is_empty() {
                    cur.push(pos);
                }
                flatten_cubic(pos, a, b, p, tol, &mut cur);
                pos = p;
            }
            Seg::Z => {
                if cur.is_empty() {
                    continue;
                }
                finish(&mut cur, true, &mut out);
                pos = start;
            }
        }
    }
    finish(&mut cur, false, &mut out);
    // Open paths whose ends meet are treated as closed.
    for pl in &mut out {
        if !pl.closed && pl.pts.len() > 3 && pl.pts[0].dist(*pl.pts.last().unwrap()) < 1e-6 {
            pl.pts.pop();
            pl.closed = true;
        }
    }
    out
}

fn flatten_cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, tol: f64, out: &mut Vec<Pt>) {
    // Number of segments from the second-difference bound (Wang's formula).
    let dd1 = p0.sub(p1.scale(2.0)).add(p2);
    let dd2 = p1.sub(p2.scale(2.0)).add(p3);
    let l = dd1.len().max(dd2.len());
    let n = ((6.0 * l / (8.0 * tol)).sqrt().ceil() as usize).clamp(1, 1000);
    for i in 1..=n {
        let t = i as f64 / n as f64;
        let mt = 1.0 - t;
        let a = mt * mt * mt;
        let b = 3.0 * mt * mt * t;
        let c = 3.0 * mt * t * t;
        let d = t * t * t;
        out.push(Pt::new(
            a * p0.x + b * p1.x + c * p2.x + d * p3.x,
            a * p0.y + b * p1.y + c * p2.y + d * p3.y,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        let segs = parse_d("M0 0 L10 0 L10 10 Z M 1,1 c 1,0 2,1 2,2");
        assert_eq!(segs.len(), 6);
        let d = segs_to_d(&segs);
        assert_eq!(parse_d(&d), segs);
    }

    #[test]
    fn relative_and_hv() {
        let segs = parse_d("m10 10 h5 v5 h-5 z");
        assert_eq!(
            segs,
            vec![
                Seg::M(Pt::new(10.0, 10.0)),
                Seg::L(Pt::new(15.0, 10.0)),
                Seg::L(Pt::new(15.0, 15.0)),
                Seg::L(Pt::new(10.0, 15.0)),
                Seg::Z
            ]
        );
    }

    #[test]
    fn compact_numbers() {
        let segs = parse_d("M1-2L.5.5L1e1,2");
        assert_eq!(segs[0], Seg::M(Pt::new(1.0, -2.0)));
        assert_eq!(segs[1], Seg::L(Pt::new(0.5, 0.5)));
        assert_eq!(segs[2], Seg::L(Pt::new(10.0, 2.0)));
    }

    #[test]
    fn flatten_circle_arc() {
        let segs = parse_d("M0 0 A10 10 0 1 1 0 20 A10 10 0 1 1 0 0 Z");
        let pls = flatten(&segs, 0.01);
        assert_eq!(pls.len(), 1);
        assert!(pls[0].closed);
        let len = pls[0].length();
        assert!((len - std::f64::consts::PI * 20.0).abs() < 0.05, "{len}");
        let b = pls[0].bbox().rect();
        assert!((b.w - 20.0).abs() < 0.01 && (b.h - 20.0).abs() < 0.01);
    }

    #[test]
    fn affine_compose() {
        let t = Affine::translate(5.0, 0.0);
        let r = Affine::rotate_deg(90.0);
        let m = r.then(&t); // rotate then translate
        let p = m.apply(Pt::new(1.0, 0.0));
        assert!((p.x - 5.0).abs() < 1e-9 && (p.y - 1.0).abs() < 1e-9);
    }
}
