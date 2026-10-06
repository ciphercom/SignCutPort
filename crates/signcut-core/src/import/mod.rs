//! File importers. Every importer produces an [`ImportedDesign`] whose paths
//! are SVG path data in millimetres, normalised so the bounding box starts at
//! (0,0) (SignCut's "optimized" bounding box).

pub mod dxf;
pub mod hpgl;
pub mod svg;
pub mod text;

use crate::fonts::{FontIssue, FontLibrary};
use crate::geometry::{parse_d, segs_bbox, segs_to_d, transform_segs, Affine, BBox};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportOptions {
    /// Override the SVG user-unit resolution (96 = Inkscape/browsers, 72 = Illustrator).
    pub dpi: Option<f64>,
    /// Missing family name -> installed family to use instead.
    pub font_substitutions: HashMap<String, String>,
    /// Directory for resolving relative references.
    pub resources_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPath {
    /// SVG path data, absolute, millimetres.
    pub d: String,
    /// "#rrggbb"
    pub color: String,
    pub filled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedDesign {
    pub name: String,
    pub paths: Vec<ImportedPath>,
    pub width_mm: f64,
    pub height_mm: f64,
    /// Offset of the design's bounding box inside the source document (mm).
    pub offset_x_mm: f64,
    pub offset_y_mm: f64,
    pub doc_width_mm: f64,
    pub doc_height_mm: f64,
    pub text_objects: usize,
    pub font_issues: Vec<FontIssue>,
    pub warnings: Vec<String>,
}

impl ImportedDesign {
    /// Build a design and normalise its paths to start at (0,0).
    pub fn new(name: &str, mut paths: Vec<ImportedPath>) -> Self {
        let mut bb = BBox::EMPTY;
        let parsed: Vec<_> = paths.iter().map(|p| parse_d(&p.d)).collect();
        for segs in &parsed {
            bb.union(&segs_bbox(segs));
        }
        let r = bb.rect();
        let shift = Affine::translate(-r.x, -r.y);
        for (p, segs) in paths.iter_mut().zip(parsed) {
            p.d = segs_to_d(&transform_segs(&segs, &shift));
        }
        paths.retain(|p| !p.d.is_empty());
        ImportedDesign {
            name: name.to_string(),
            paths,
            width_mm: r.w,
            height_mm: r.h,
            offset_x_mm: r.x,
            offset_y_mm: r.y,
            doc_width_mm: r.w,
            doc_height_mm: r.h,
            text_objects: 0,
            font_issues: vec![],
            warnings: vec![],
        }
    }
}

pub fn import_file(path: &Path, fonts: &FontLibrary, opts: &ImportOptions) -> Result<ImportedDesign, String> {
    let data = std::fs::read(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported")
        .to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut opts = opts.clone();
    if opts.resources_dir.is_none() {
        opts.resources_dir = path.parent().map(|p| p.display().to_string());
    }
    import_bytes(&data, &ext, &name, fonts, &opts)
}

pub fn import_bytes(
    data: &[u8],
    ext: &str,
    name: &str,
    fonts: &FontLibrary,
    opts: &ImportOptions,
) -> Result<ImportedDesign, String> {
    let design = match ext {
        "svg" => svg::import_svg(data, name, fonts, opts)?,
        "plt" | "hpgl" | "hpg" | "hgl" => hpgl::import_hpgl(data, name)?,
        "dxf" => dxf::import_dxf(data, name)?,
        "pdf" | "ai" | "eps" | "ps" => {
            return Err(format!(
                ".{ext} files are not supported directly yet. Export as SVG from your design \
                 program (Illustrator: File › Save As › SVG, “Fonts: Convert to outline” is not \
                 needed — SignCut Port keeps your installed fonts)."
            ))
        }
        _ => {
            // Sniff content.
            let head = String::from_utf8_lossy(&data[..data.len().min(512)]);
            if head.contains("<svg") || head.contains("<?xml") {
                svg::import_svg(data, name, fonts, opts)?
            } else if head.contains("SECTION") {
                dxf::import_dxf(data, name)?
            } else if head.contains("IN;") || head.contains("PU") || head.contains(";:") {
                hpgl::import_hpgl(data, name)?
            } else {
                return Err(format!("Unsupported file type: .{ext}"));
            }
        }
    };
    if design.paths.is_empty() {
        let mut msg = "No cuttable vector paths found in the file.".to_string();
        if !design.warnings.is_empty() {
            msg.push(' ');
            msg.push_str(&design.warnings.join(" "));
        }
        return Err(msg);
    }
    Ok(design)
}

pub const SUPPORTED_EXTENSIONS: &[&str] = &["svg", "plt", "hpgl", "hpg", "dxf"];
