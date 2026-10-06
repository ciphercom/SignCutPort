//! Checks against the embedded machine catalog.
use signcut_core::drivers::Catalog;
use signcut_core::encode::{encode, EncodeOptions};
use signcut_core::geometry::Affine;
use signcut_core::plan::{plan, CutSettings, JobObject, JobPath, Placement};

#[test]
fn catalog_has_vevor_models() {
    let c = Catalog::builtin();
    let total: usize = c.drivers.iter().map(|d| d.models.len()).sum();
    assert!(total > 1500, "{total}");
    let p = c.profile("VEVOR", "VEVOR KH-720").expect("KH-720");
    assert_eq!(p.max_width_mm, 630.0);
    assert_eq!(p.commands.initialise, ";:H A L0 ECN U");
    assert!(p.rts && p.cts && !p.dtr);
    let a = c.profile("VEVOR", "VEVOR KH-720A").expect("KH-720A");
    assert_eq!(a.commands.initialise, "IN");
    assert_eq!(a.default_baud, 38400);
}

#[test]
fn every_model_encodes() {
    let c = Catalog::builtin();
    let job = [JobObject {
        paths: vec![JobPath { d: "M0 0h20v20h-20zM5 5h5v5h-5z".into(), color: "#000".into() }],
        transform: Affine::IDENTITY,
    }];
    let s = CutSettings { material_width: 100.0, placement: Placement::Origin, speed: Some(10.0), force: Some(50.0), ..Default::default() };
    let pl = plan(&job, &s, None);
    for d in &c.drivers {
        for m in &d.models {
            let p = d.profile(m);
            let out = encode(&pl, &p, &EncodeOptions::default());
            assert!(out.total_bytes > 20, "{} {}", d.manufacturer, m.name);
        }
    }
}
