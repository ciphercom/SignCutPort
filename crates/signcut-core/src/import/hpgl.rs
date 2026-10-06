//! Import of HPGL / HPGL-2 (.plt) and DMPL plot files.

use super::{ImportedDesign, ImportedPath};
use crate::geometry::{segs_to_d, Pt, Seg};

/// HPGL plotter units per millimetre.
const UNITS_PER_MM: f64 = 40.0;

pub fn import_hpgl(data: &[u8], name: &str) -> Result<ImportedDesign, String> {
    let text = String::from_utf8_lossy(data);
    let is_dmpl = text.trim_start().starts_with(";:") || (!text.contains("PD") && text.contains(" D"));
    let polylines = if is_dmpl { parse_dmpl(&text) } else { parse_hpgl(&text) };
    // Plotter Y is up; screen Y is down.
    let mut paths = Vec::new();
    for (pen, pts) in polylines {
        if pts.len() < 2 {
            continue;
        }
        let mut segs = Vec::with_capacity(pts.len());
        segs.push(Seg::M(flip(pts[0])));
        for p in &pts[1..] {
            segs.push(Seg::L(flip(*p)));
        }
        if pts.len() > 3 && pts[0].dist(*pts.last().unwrap()) < 0.01 {
            segs.pop();
            segs.push(Seg::Z);
        }
        paths.push(ImportedPath {
            d: segs_to_d(&segs),
            color: pen_color(pen).into(),
            filled: false,
        });
    }
    Ok(ImportedDesign::new(name, paths))
}

fn flip(p: Pt) -> Pt {
    Pt::new(p.x, -p.y)
}

fn pen_color(pen: u32) -> &'static str {
    match pen {
        2 => "#d62728",
        3 => "#2ca02c",
        4 => "#1f77b4",
        5 => "#9467bd",
        6 => "#ff7f0e",
        _ => "#000000",
    }
}

fn parse_hpgl(text: &str) -> Vec<(u32, Vec<Pt>)> {
    let mut out: Vec<(u32, Vec<Pt>)> = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    let mut pos = Pt::default();
    let mut pen_down = false;
    let mut absolute = true;
    let mut pen = 1u32;
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    let flush = |cur: &mut Vec<Pt>, out: &mut Vec<(u32, Vec<Pt>)>, pen: u32| {
        if cur.len() > 1 {
            out.push((pen, std::mem::take(cur)));
        } else {
            cur.clear();
        }
    };
    while i < b.len() {
        let c = b[i];
        if !c.is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        if i + 1 >= b.len() {
            break;
        }
        let mnemonic: String = [c, b[i + 1]].iter().collect::<String>().to_ascii_uppercase();
        i += 2;
        // Collect parameters up to the next letter or terminator.
        let start = i;
        while i < b.len() && !b[i].is_ascii_alphabetic() && b[i] != ';' {
            i += 1;
        }
        let params: Vec<f64> = b[start..i]
            .iter()
            .collect::<String>()
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect();
        let moves = |params: &[f64], down: bool, absolute: bool, pos: &mut Pt, cur: &mut Vec<Pt>, out: &mut Vec<(u32, Vec<Pt>)>| {
            for pair in params.chunks(2) {
                if pair.len() < 2 {
                    break;
                }
                let p = Pt::new(pair[0] / UNITS_PER_MM, pair[1] / UNITS_PER_MM);
                let np = if absolute { p } else { pos.add(p) };
                if down {
                    if cur.is_empty() {
                        cur.push(*pos);
                    }
                    cur.push(np);
                } else {
                    flush(cur, out, pen);
                }
                *pos = np;
            }
        };
        match mnemonic.as_str() {
            "PU" => {
                pen_down = false;
                flush(&mut cur, &mut out, pen);
                moves(&params, false, absolute, &mut pos, &mut cur, &mut out);
            }
            "PD" => {
                pen_down = true;
                moves(&params, true, absolute, &mut pos, &mut cur, &mut out);
            }
            "PA" => {
                absolute = true;
                moves(&params, pen_down, absolute, &mut pos, &mut cur, &mut out);
            }
            "PR" => {
                absolute = false;
                moves(&params, pen_down, absolute, &mut pos, &mut cur, &mut out);
            }
            "SP" => {
                flush(&mut cur, &mut out, pen);
                pen = params.first().map(|v| *v as u32).unwrap_or(1).max(1);
            }
            "IN" => {
                flush(&mut cur, &mut out, pen);
                absolute = true;
                pen_down = false;
            }
            _ => {}
        }
        if i < b.len() && b[i] == ';' {
            i += 1;
        }
    }
    flush(&mut cur, &mut out, pen);
    out
}

fn parse_dmpl(text: &str) -> Vec<(u32, Vec<Pt>)> {
    // DMPL: U/D set pen state, followed by x,y pairs; EC sets units.
    let mut out = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    let mut pos = Pt::default();
    let mut down = false;
    let mut scale = 1.0 / 40.0; // ECN = 0.025 mm
    let mut nums: Vec<f64> = Vec::new();
    let mut tok = String::new();
    let flush_nums = |nums: &mut Vec<f64>, down: bool, pos: &mut Pt, cur: &mut Vec<Pt>, scale: f64| {
        for pair in nums.chunks(2) {
            if pair.len() == 2 {
                let p = Pt::new(pair[0] * scale, pair[1] * scale);
                if down {
                    if cur.is_empty() {
                        cur.push(*pos);
                    }
                    cur.push(p);
                }
                *pos = p;
            }
        }
        nums.clear();
    };
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() || c == '-' || c == '.' {
            tok.push(c);
            i += 1;
            continue;
        }
        if !tok.is_empty() {
            if let Ok(v) = tok.parse() {
                nums.push(v);
            }
            tok.clear();
        }
        match c {
            'E' if i + 2 < chars.len() && chars[i + 1] == 'C' => {
                scale = match chars[i + 2] {
                    'N' => 0.025,
                    'M' => 0.1,
                    '1' => 0.0254,
                    '5' => 0.127,
                    _ => scale,
                };
                i += 3;
                continue;
            }
            'U' | 'D' => {
                flush_nums(&mut nums, down, &mut pos, &mut cur, scale);
                let nd = c == 'D';
                if !nd && cur.len() > 1 {
                    out.push((1, std::mem::take(&mut cur)));
                } else if !nd {
                    cur.clear();
                }
                down = nd;
            }
            _ => {}
        }
        i += 1;
    }
    if !tok.is_empty() {
        if let Ok(v) = tok.parse() {
            nums.push(v);
        }
    }
    flush_nums(&mut nums, down, &mut pos, &mut cur, scale);
    if cur.len() > 1 {
        out.push((1, cur));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hpgl_square() {
        let d = import_hpgl(b"IN;PU0,0;PD400,0,400,400,0,400,0,0;PU;", "sq").unwrap();
        assert_eq!(d.paths.len(), 1);
        assert!((d.width_mm - 10.0).abs() < 1e-9);
        assert!(d.paths[0].d.ends_with('Z'));
    }

    #[test]
    fn dmpl_square() {
        let d = import_hpgl(b";:H A L0 ECN U 0,0 D 400,0 400,400 0,400 0,0 U @", "sq").unwrap();
        assert_eq!(d.paths.len(), 1);
        assert!((d.height_mm - 10.0).abs() < 1e-9);
    }
}
