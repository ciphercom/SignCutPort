//! Font resolution against the fonts installed on the test machine.
//! Skips silently when the expected fonts are not installed.

use signcut_core::fonts::FontLibrary;
use signcut_core::import::{svg::import_svg, text, ImportOptions};

fn lib_with(family: &str) -> Option<FontLibrary> {
    let lib = FontLibrary::system();
    lib.families()
        .iter()
        .any(|f| f.family == family)
        .then_some(lib)
}

#[test]
fn illustrator_postscript_names_resolve() {
    let Some(lib) = lib_with("Inter") else { return };
    // Illustrator writes the PostScript name as the family.
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
      <text x="0" y="50" font-family="'Inter-BoldItalic'" font-size="40">Hello</text></svg>"#;
    let d = import_svg(svg, "t", &lib, &ImportOptions::default()).unwrap();
    assert!(d.font_issues.is_empty(), "{:?}", d.font_issues);
    assert!(!d.paths.is_empty());
}

#[test]
fn inkscape_family_style_names_resolve() {
    let Some(lib) = lib_with("Inter Display") else { return };
    let id = lib.resolve("Inter Display Medium", 400, false).unwrap();
    let face = lib.db().face(id).unwrap();
    assert_eq!(face.weight.0, 500, "{}", face.post_script_name);
    // Case / spacing insensitive.
    assert!(lib.resolve("interdisplay", 400, false).is_some());
}

#[test]
fn missing_fonts_are_reported_not_hidden() {
    let Some(lib) = lib_with("Inter") else { return };
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
      <text x="0" y="50" font-family="SuperCustomFont, Inter" font-size="40">A</text>
      <text x="0" y="90" font-family="NopeFont" font-size="40">B</text></svg>"#;
    let d = import_svg(svg, "t", &lib, &ImportOptions::default()).unwrap();
    // First text falls through to Inter as the author intended: no issue.
    // Second text has no installed candidate: reported.
    assert_eq!(d.font_issues.len(), 1, "{:?}", d.font_issues);
    assert_eq!(d.font_issues[0].requested, "NopeFont");
    assert!(d.font_issues[0].substituted_with.is_some());

    // With a substitution the issue disappears.
    let mut opts = ImportOptions::default();
    opts.font_substitutions.insert("NopeFont".into(), "Inter".into());
    let d = import_svg(svg, "t", &lib, &opts).unwrap();
    assert!(d.font_issues.is_empty());
}

#[test]
fn text_tool_size_is_in_mm() {
    let Some(lib) = lib_with("DejaVu Sans") else { return };
    let d = text::render_text(
        &text::TextRequest {
            text: "H".into(),
            family: "DejaVu Sans".into(),
            size_mm: 100.0,
            ..Default::default()
        },
        &lib,
    )
    .unwrap();
    // DejaVu Sans cap height is 0.729 em.
    assert!((d.height_mm - 72.9).abs() < 1.0, "{}", d.height_mm);
    assert!(d.font_issues.is_empty());
}

/// macOS system fonts: Helvetica Neue ships as a .ttc where Bold is not the
/// first face (SignCut only indexes face 0), and Illustrator writes
/// PostScript names such as "HelveticaNeue-Bold" / "Arial-BoldMT".
#[test]
fn macos_ttc_faces_and_postscript_names() {
    let Some(lib) = lib_with("Helvetica Neue") else { return };
    let id = lib.resolve("Helvetica Neue", 700, false).expect("bold face");
    assert_eq!(lib.db().face(id).unwrap().weight.0, 700);
    let id = lib.resolve("HelveticaNeue-Bold", 400, false).expect("postscript name");
    assert_eq!(lib.db().face(id).unwrap().weight.0, 700);
    if lib.families().iter().any(|f| f.family == "Arial") {
        let id = lib.resolve("Arial-BoldMT", 400, false).expect("Arial-BoldMT");
        assert_eq!(lib.db().face(id).unwrap().weight.0, 700);
    }
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="100">
      <text x="0" y="50" font-family="'HelveticaNeue-Bold'" font-size="40">Bold</text>
      <text x="0" y="90" style="font-family:'Helvetica Neue';font-weight:bold;font-style:italic" font-size="40">Both</text></svg>"#;
    let d = import_svg(svg, "t", &lib, &ImportOptions::default()).unwrap();
    assert!(d.font_issues.is_empty(), "{:?}", d.font_issues);
}
