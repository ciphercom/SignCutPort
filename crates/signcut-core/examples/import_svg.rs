//! Debug helper: import a file and print what happened.
//! cargo run -p signcut-core --example import_svg -- file.svg
use signcut_core::fonts::FontLibrary;
use signcut_core::import::{import_file, ImportOptions};

fn main() {
    let path = std::env::args().nth(1).expect("usage: import_svg <file>");
    let lib = FontLibrary::system();
    match import_file(std::path::Path::new(&path), &lib, &ImportOptions::default()) {
        Ok(d) => {
            println!("{}: {} paths, {:.1} x {:.1} mm (document {:.1} x {:.1} mm), {} text objects",
                d.name, d.paths.len(), d.width_mm, d.height_mm, d.doc_width_mm, d.doc_height_mm, d.text_objects);
            for i in &d.font_issues {
                println!("font issue: {:?}", i);
            }
            for w in &d.warnings {
                println!("warning: {w}");
            }
        }
        Err(e) => println!("error: {e}"),
    }
}
