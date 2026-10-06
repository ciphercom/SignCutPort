//! Text tool: turn a string + installed font into cuttable outlines.

use super::{ImportOptions, ImportedDesign};
use crate::fonts::FontLibrary;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextRequest {
    pub text: String,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    /// Font size (em) in millimetres.
    pub size_mm: f64,
    /// Extra letter spacing in millimetres.
    pub letter_spacing_mm: f64,
    /// Line height as a multiple of the font size.
    pub line_height: f64,
    /// "start" | "middle" | "end"
    pub align: String,
}

impl Default for TextRequest {
    fn default() -> Self {
        Self {
            text: String::new(),
            family: "Helvetica".into(),
            weight: 400,
            italic: false,
            size_mm: 50.0,
            letter_spacing_mm: 0.0,
            line_height: 1.2,
            align: "start".into(),
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn render_text(req: &TextRequest, fonts: &FontLibrary) -> Result<ImportedDesign, String> {
    if req.text.trim().is_empty() {
        return Err("Text is empty".into());
    }
    let size = req.size_mm.max(0.1);
    let anchor = match req.align.as_str() {
        "middle" | "center" => "middle",
        "end" | "right" => "end",
        _ => "start",
    };
    let lines: Vec<&str> = req.text.lines().collect();
    let width = 10_000.0;
    let x = match anchor {
        "middle" => width / 2.0,
        "end" => width,
        _ => 0.0,
    };
    let mut body = String::new();
    for (i, line) in lines.iter().enumerate() {
        let y = size * (1.0 + i as f64 * req.line_height);
        body.push_str(&format!(
            r#"<text x="{x}" y="{y}" xml:space="preserve">{}</text>"#,
            esc(line)
        ));
    }
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">
<g font-family="{fam}" font-size="{size}" font-weight="{wt}" font-style="{st}" letter-spacing="{ls}" text-anchor="{anchor}" fill="#000">{body}</g></svg>"##,
        w = width,
        h = size * (lines.len() as f64 * req.line_height + 1.0),
        fam = esc(&format!("'{}'", req.family.replace('\'', ""))),
        wt = req.weight,
        st = if req.italic { "italic" } else { "normal" },
        ls = req.letter_spacing_mm,
    );
    // The SVG uses mm as user units: dpi 25.4 makes 1 user unit = 1 mm.
    let opts = ImportOptions {
        dpi: Some(25.4),
        ..Default::default()
    };
    let mut d = super::svg::import_svg(svg.as_bytes(), &req.text, fonts, &opts)?;
    d.name = req.text.lines().next().unwrap_or("Text").chars().take(40).collect();
    d.text_objects = lines.len();
    Ok(d)
}
