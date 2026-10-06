//! SVG import via usvg. Text is converted to outlines with *our* font
//! resolver, so installed custom fonts are honoured and misses are reported.

use super::{ImportOptions, ImportedDesign, ImportedPath};
use crate::fonts::{FontIssue, FontLibrary};
use crate::geometry::{segs_to_d, Affine, Pt, Seg};
use std::sync::{Arc, Mutex};

pub fn import_svg(
    data: &[u8],
    name: &str,
    fonts: &FontLibrary,
    opts: &ImportOptions,
) -> Result<ImportedDesign, String> {
    let text = decode_text(data)?;
    let mut fonts = fonts.clone();
    let mut warnings = Vec::new();

    // Fonts embedded with @font-face data URIs.
    for font in embedded_fonts(&text) {
        let fams = fonts.add_font_data(font);
        if !fams.is_empty() {
            warnings.push(format!("Using embedded font(s): {}", fams.join(", ")));
        }
    }

    // Illustrator historically writes 72 user units per inch.
    let is_illustrator = text.contains("Adobe Illustrator") || text.contains("Generator: Adobe");
    let dpi = opts.dpi.unwrap_or(if is_illustrator { 72.0 } else { 96.0 });

    let issues: Arc<Mutex<Vec<FontIssue>>> = Arc::new(Mutex::new(Vec::new()));
    let mut uopt = usvg::Options::default();
    uopt.dpi = dpi as f32;
    uopt.fontdb = fonts.db().clone();
    uopt.font_family = "sans-serif".into();
    uopt.font_resolver = fonts.usvg_resolver(opts.font_substitutions.clone(), issues.clone());
    if let Some(dir) = &opts.resources_dir {
        uopt.resources_dir = Some(dir.into());
    }
    let tree = usvg::Tree::from_str(&text, &uopt).map_err(|e| format!("Invalid SVG: {e}"))?;

    // usvg user units are CSS px at `dpi`; mm = px * 25.4 / dpi.
    // Absolute units (mm, in, pt) in width/height have already been
    // converted by usvg using the same dpi, so this is consistent.
    let k = 25.4 / dpi;
    let to_mm = Affine::scale(k, k);

    let mut paths = Vec::new();
    let mut stats = Stats::default();
    walk_group(tree.root(), &to_mm, &mut paths, &mut stats);

    if stats.images > 0 {
        warnings.push(format!(
            "{} embedded bitmap image(s) ignored (only vector outlines can be cut)",
            stats.images
        ));
    }
    if stats.gradients > 0 {
        warnings.push("Gradient/pattern fills are cut as plain outlines".into());
    }
    let size = tree.size();
    let mut design = ImportedDesign::new(name, paths);
    design.doc_width_mm = (size.width() as f64) * k;
    design.doc_height_mm = (size.height() as f64) * k;
    design.text_objects = stats.texts;
    design.font_issues = std::mem::take(&mut *issues.lock().unwrap());
    design.warnings = warnings;
    Ok(design)
}

#[derive(Default)]
struct Stats {
    images: usize,
    texts: usize,
    gradients: usize,
}

fn decode_text(data: &[u8]) -> Result<String, String> {
    if data.starts_with(&[0x1f, 0x8b]) {
        return Err("Compressed SVG (.svgz) is not supported; please save as plain SVG".into());
    }
    let s = String::from_utf8_lossy(data);
    Ok(s.trim_start_matches('\u{feff}').to_string())
}

fn color_hex(paint: &usvg::Paint, stats: &mut Stats) -> Option<String> {
    match paint {
        usvg::Paint::Color(c) => Some(format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue)),
        _ => {
            stats.gradients += 1;
            None
        }
    }
}

fn walk_group(g: &usvg::Group, to_mm: &Affine, out: &mut Vec<ImportedPath>, stats: &mut Stats) {
    for node in g.children() {
        match node {
            usvg::Node::Group(g) => walk_group(g, to_mm, out, stats),
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let color = p
                    .fill()
                    .and_then(|f| color_hex(f.paint(), stats))
                    .or_else(|| p.stroke().and_then(|s| color_hex(s.paint(), stats)))
                    .unwrap_or_else(|| "#000000".into());
                let t = p.abs_transform();
                let m = Affine([
                    t.sx as f64,
                    t.ky as f64,
                    t.kx as f64,
                    t.sy as f64,
                    t.tx as f64,
                    t.ty as f64,
                ])
                .then(to_mm);
                let segs = convert_path(p.data(), &m);
                if segs.len() < 2 {
                    continue;
                }
                out.push(ImportedPath {
                    d: segs_to_d(&segs),
                    color,
                    filled: p.fill().is_some(),
                });
            }
            usvg::Node::Text(t) => {
                stats.texts += 1;
                walk_group(t.flattened(), to_mm, out, stats);
            }
            usvg::Node::Image(_) => stats.images += 1,
        }
    }
}

fn convert_path(path: &usvg::tiny_skia_path::Path, m: &Affine) -> Vec<Seg> {
    use usvg::tiny_skia_path::PathSegment as PS;
    let mut segs = Vec::new();
    let mut cur = Pt::default();
    let p = |x: f32, y: f32| m.apply(Pt::new(x as f64, y as f64));
    for s in path.segments() {
        match s {
            PS::MoveTo(a) => {
                cur = p(a.x, a.y);
                segs.push(Seg::M(cur));
            }
            PS::LineTo(a) => {
                cur = p(a.x, a.y);
                segs.push(Seg::L(cur));
            }
            PS::QuadTo(q, a) => {
                let q = p(q.x, q.y);
                let e = p(a.x, a.y);
                let c1 = cur.add(q.sub(cur).scale(2.0 / 3.0));
                let c2 = e.add(q.sub(e).scale(2.0 / 3.0));
                segs.push(Seg::C(c1, c2, e));
                cur = e;
            }
            PS::CubicTo(a, b, e) => {
                cur = p(e.x, e.y);
                segs.push(Seg::C(p(a.x, a.y), p(b.x, b.y), cur));
            }
            PS::Close => segs.push(Seg::Z),
        }
    }
    segs
}

/// Extract `@font-face { src: url(data:...;base64,...) }` fonts.
fn embedded_fonts(text: &str) -> Vec<Vec<u8>> {
    use base64::Engine;
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("@font-face") {
        rest = &rest[pos + 10..];
        let end = rest.find('}').unwrap_or(rest.len());
        let block = &rest[..end];
        let mut b = block;
        while let Some(u) = b.find("url(") {
            b = &b[u + 4..];
            let close = b.find(')').unwrap_or(b.len());
            let url = b[..close].trim().trim_matches(|c| c == '"' || c == '\'');
            if let Some(idx) = url.find("base64,") {
                if url.starts_with("data:") {
                    let payload: String = url[idx + 7..].chars().filter(|c| !c.is_whitespace()).collect();
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload) {
                        // WOFF/WOFF2 are not supported by the parser; only raw sfnt.
                        if bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"OTTO") || bytes.starts_with(b"true") || bytes.starts_with(b"ttcf") {
                            out.push(bytes);
                        }
                    }
                }
            }
            b = &b[close.min(b.len())..];
        }
        rest = &rest[end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_shapes_in_mm() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">
            <rect x="10" y="10" width="20" height="10" fill="red"/>
            <circle cx="70" cy="25" r="10" fill="none" stroke="blue"/>
        </svg>"#;
        let d = import_svg(svg, "t", &FontLibrary::empty(), &ImportOptions::default()).unwrap();
        assert_eq!(d.paths.len(), 2);
        assert_eq!(d.paths[0].color, "#ff0000");
        assert_eq!(d.paths[1].color, "#0000ff");
        assert!((d.doc_width_mm - 100.0).abs() < 1e-3);
        // rect bbox
        let b = crate::geometry::segs_bbox(&crate::geometry::parse_d(&d.paths[0].d)).rect();
        assert!(b.x.abs() < 1e-3 && (b.w - 20.0).abs() < 1e-3, "{b:?}");
        assert!((d.offset_x_mm - 10.0).abs() < 1e-3);
    }

    #[test]
    fn px_units_at_96dpi() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="48"/></svg>"#;
        let d = import_svg(svg, "t", &FontLibrary::empty(), &ImportOptions::default()).unwrap();
        assert!((d.width_mm - 25.4).abs() < 1e-3, "{}", d.width_mm);
        assert!((d.height_mm - 12.7).abs() < 1e-3);
    }

    #[test]
    fn reports_missing_font() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><text x="10" y="50" font-family="Definitely Not Installed" font-size="20">Hi</text></svg>"#;
        let d = import_svg(svg, "t", &FontLibrary::empty(), &ImportOptions::default()).unwrap();
        assert_eq!(d.font_issues.len(), 1);
        assert_eq!(d.font_issues[0].requested, "Definitely Not Installed");
        assert_eq!(d.font_issues[0].kind, "missing-font");
    }
}
