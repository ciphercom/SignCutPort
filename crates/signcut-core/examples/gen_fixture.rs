//! Generate the UI mock fixture (src/mock/fixture.json) from real core output.
use signcut_core::drivers::Catalog;
use signcut_core::fonts::FontLibrary;
use signcut_core::import::{svg::import_svg, text::{render_text, TextRequest}, ImportOptions};

fn main() {
    let fonts = FontLibrary::system();
    let text = render_text(
        &TextRequest { text: "Hello Vinyl".into(), family: "DejaVu Sans".into(), weight: 700, size_mm: 60.0, ..Default::default() },
        &fonts,
    )
    .unwrap();
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="120mm" height="110mm" viewBox="0 0 120 110">
      <path fill="#e8455f" d="M60 105 C20 75 0 55 0 30 C0 12 14 0 30 0 C44 0 54 8 60 18 C66 8 76 0 90 0 C106 0 120 12 120 30 C120 55 100 75 60 105Z"/>
      <path fill="#1f77b4" d="M40 30 L80 30 L80 50 L40 50 Z"/>
      <text x="60" y="80" font-family="'MyCustomFont-Bold', 'Brush Script'" font-size="16" text-anchor="middle" fill="#222">Love</text>
    </svg>"##;
    let heart = import_svg(svg, "heart-logo", &fonts, &ImportOptions::default()).unwrap();
    let catalog = Catalog::builtin();
    let profile = catalog.profile("VEVOR", "VEVOR KH-720").unwrap();
    let families: Vec<_> = fonts.families().into_iter().take(60).collect();
    let out = serde_json::json!({
        "text": text,
        "heart": heart,
        "machines": catalog.summaries(),
        "profile": profile,
        "fonts": families,
    });
    println!("{}", serde_json::to_string(&out).unwrap());
}
