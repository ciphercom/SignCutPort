//! Convert SignCut driver XML files (a directory or a drivers.pak) into the
//! compact JSON catalog embedded in the app.
//!
//! cargo run -p signcut-core --example convert_drivers -- <dir|drivers.pak> > data/machines.json
use signcut_core::drivers::{load_driver_source, DriverFile};

fn main() {
    let src = std::env::args().nth(1).expect("usage: convert_drivers <dir|drivers.pak>");
    let path = std::path::Path::new(&src);
    let mut drivers: Vec<DriverFile> = Vec::new();
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path).unwrap().flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.extension().map(|e| e.eq_ignore_ascii_case("xml")) != Some(true) {
                continue;
            }
            match load_driver_source(&std::fs::read(&p).unwrap()) {
                Ok(mut d) => drivers.append(&mut d),
                Err(e) => eprintln!("skip {}: {e}", p.display()),
            }
        }
    } else {
        drivers = load_driver_source(&std::fs::read(path).unwrap()).unwrap();
    }
    drivers.retain(|d| !d.models.is_empty());
    drivers.sort_by(|a, b| a.manufacturer.to_lowercase().cmp(&b.manufacturer.to_lowercase()));
    let n: usize = drivers.iter().map(|d| d.models.len()).sum();
    eprintln!("{} manufacturers, {} models", drivers.len(), n);
    println!("{}", serde_json::to_string(&drivers).unwrap());
}
