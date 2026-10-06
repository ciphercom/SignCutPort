//! Cutter ("plotter") definitions.
//!
//! The format mirrors SignCut's driver XML files (`drivers.pak` is a zip of
//! them): a manufacturer with global `<Config>`/`<Commands>` plus a list of
//! `<Plotter>` models that can override any value. A model value of `CLEAR`
//! removes the inherited value.
//!
//! A catalog converted from those files ships embedded in the app; users can
//! additionally load a `drivers.pak` or individual XML files at runtime.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ModelDef {
    #[serde(rename = "n")]
    pub name: String,
    /// Max cutting width in mm.
    #[serde(rename = "w")]
    pub max_width: f64,
    #[serde(rename = "l")]
    pub max_length: f64,
    #[serde(rename = "c", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, String>,
    #[serde(rename = "k", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub commands: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DriverFile {
    #[serde(rename = "m")]
    pub manufacturer: String,
    #[serde(rename = "c", default)]
    pub config: BTreeMap<String, String>,
    #[serde(rename = "k", default)]
    pub commands: BTreeMap<String, String>,
    #[serde(rename = "p")]
    pub models: Vec<ModelDef>,
}

/// Command strings resolved for one model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Commands {
    pub initialise: String,
    pub start: String,
    pub end: String,
    pub tool_up: String,
    pub tool_down: String,
    pub page_feed: String,
    pub after_cut: String,
    pub delimiter: String,
    pub terminator: String,
    pub velocity: String,
    pub force: String,
    pub select_pen: String,
    pub select_tool: String,
    pub cut_absolute: String,
    pub cut_relative: String,
    pub cutoff: String,
    pub up_speed: String,
}

/// Fully merged description of one cutter model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MachineProfile {
    pub manufacturer: String,
    pub model: String,
    pub max_width_mm: f64,
    pub max_length_mm: f64,
    /// HPGL2 | HPGL | DMPL | MHGL | GPGL | GCODE | ...
    pub language: String,
    pub relative_coordinates: bool,
    /// Millimetres per device unit.
    pub x_resolution: f64,
    pub y_resolution: f64,
    pub use_knife_compensation: bool,
    pub default_blade_offset: f64,
    pub default_baud: u32,
    pub rts: bool,
    pub cts: bool,
    pub dtr: bool,
    pub dsr: bool,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub in_pipe: Option<u8>,
    pub out_pipe: Option<u8>,
    pub act_as_printer_driver: bool,
    pub swap_axis: bool,
    pub rotate90: bool,
    pub pens: u32,
    pub tool_names: Vec<String>,
    pub min_speed: Option<f64>,
    pub max_speed: Option<f64>,
    pub min_force: Option<f64>,
    pub max_force: Option<f64>,
    pub forward_after_cut: Option<f64>,
    pub supports_contour_cut: bool,
    pub commands: Commands,
    /// All merged config values, for anything not modelled above.
    pub raw: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManufacturerSummary {
    pub manufacturer: String,
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub name: String,
    pub max_width_mm: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub drivers: Vec<DriverFile>,
}

/// Configuration keys we drop when converting (links to vendor web pages).
const DROP_KEYS: &[&str] = &["Information", "Image", "Manual"];

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn text_of(n: roxmltree::Node) -> String {
    n.text().unwrap_or("").trim().to_string()
}

fn read_map(node: Option<roxmltree::Node>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Some(n) = node {
        for c in n.children().filter(|c| c.is_element()) {
            let k = c.tag_name().name();
            if DROP_KEYS.contains(&k) || c.children().any(|x| x.is_element()) {
                continue;
            }
            let k = match k {
                // Normalise spelling variants seen in the wild.
                "Initialize" => "Initialise",
                "Select_Pen" => "SelectPen",
                "inPipe" => "InPipe",
                "outPipe" => "OutPipe",
                other => other,
            };
            m.insert(k.to_string(), text_of(c));
        }
    }
    m
}

impl DriverFile {
    pub fn from_xml(xml: &str) -> Result<Self, String> {
        // Some files carry a BOM or leading junk.
        let xml = xml.trim_start_matches('\u{feff}');
        let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        if root.tag_name().name() != "Driver" {
            return Err("not a <Driver> file".into());
        }
        let manufacturer = child(root, "Manufacturer").map(text_of).unwrap_or_default();
        let config = read_map(child(root, "Config"));
        let commands = read_map(child(root, "Commands"));
        let mut models = Vec::new();
        if let Some(ms) = child(root, "Models") {
            for p in ms
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() == "Plotter")
            {
                let mut cfg = read_map(child(p, "Config"));
                let name = child(p, "Name")
                    .map(text_of)
                    .or_else(|| cfg.remove("Name"))
                    .unwrap_or_default();
                let num = |s: Option<String>| s.and_then(|v| v.parse::<f64>().ok());
                let max_width = num(child(p, "MaxWidth").map(text_of))
                    .or_else(|| num(cfg.remove("MaxWidth")))
                    .unwrap_or(600.0);
                let max_length = num(child(p, "MaxLength").map(text_of))
                    .or_else(|| num(cfg.remove("MaxLength")))
                    .unwrap_or(50_000.0);
                if name.is_empty() {
                    continue;
                }
                models.push(ModelDef {
                    name,
                    max_width,
                    max_length,
                    config: cfg,
                    commands: read_map(child(p, "Commands")),
                });
            }
        }
        Ok(DriverFile {
            manufacturer,
            config,
            commands,
            models,
        })
    }

    pub fn profile(&self, model: &ModelDef) -> MachineProfile {
        let merge = |base: &BTreeMap<String, String>, over: &BTreeMap<String, String>| {
            let mut m = base.clone();
            for (k, v) in over {
                if v == "CLEAR" {
                    m.remove(k);
                } else {
                    m.insert(k.clone(), v.clone());
                }
            }
            m
        };
        let cfg = merge(&self.config, &model.config);
        let cmd = merge(&self.commands, &model.commands);
        let s = |k: &str| cfg.get(k).cloned().unwrap_or_default();
        let b = |k: &str, d: bool| cfg.get(k).map(|v| v.trim() == "1").unwrap_or(d);
        let f = |k: &str| cfg.get(k).and_then(|v| v.trim().parse::<f64>().ok());
        let c = |k: &str| cmd.get(k).cloned().unwrap_or_default();
        let mut tool_names = Vec::new();
        for i in 1..=4 {
            if let Some(n) = cfg.get(&format!("Tool{i}Name")) {
                tool_names.push(n.clone());
            }
        }
        let language = {
            let l = s("Language").to_ascii_uppercase();
            if l.is_empty() {
                "HPGL".into()
            } else {
                l
            }
        };
        let special = s("Special").to_ascii_lowercase();
        MachineProfile {
            manufacturer: self.manufacturer.clone(),
            model: model.name.clone(),
            max_width_mm: model.max_width,
            max_length_mm: model.max_length,
            language,
            relative_coordinates: s("CoordinateMode").eq_ignore_ascii_case("REL"),
            x_resolution: f("XResolution").filter(|v| *v > 0.0).unwrap_or(0.025),
            y_resolution: f("YResolution").filter(|v| *v > 0.0).unwrap_or(0.025),
            use_knife_compensation: b("UseKnifeCompensation", true),
            default_blade_offset: f("DefaultBladeOffset").unwrap_or(0.25),
            default_baud: f("DefaultBaud").map(|v| v as u32).unwrap_or(9600),
            rts: b("RTS", false),
            cts: b("CTS", false),
            dtr: b("DTR", false),
            dsr: b("DSR", false),
            vendor_id: f("VendorID").map(|v| v as u16),
            product_id: f("ProductID").map(|v| v as u16),
            in_pipe: f("InPipe").map(|v| v as u8),
            out_pipe: f("OutPipe").map(|v| v as u8),
            act_as_printer_driver: b("ActAsPrinterDriver", false),
            swap_axis: b("SwapAxis", false),
            rotate90: b("Rotate90", false),
            pens: f("Pens").map(|v| v as u32).unwrap_or(1).max(1),
            tool_names,
            min_speed: f("MinSpeed"),
            max_speed: f("MaxSpeed"),
            min_force: f("MinForce"),
            max_force: f("MaxForce"),
            forward_after_cut: f("ForwardAfterCut"),
            supports_contour_cut: cfg.contains_key("ARMS_Name") || special.contains("contourcut"),
            commands: Commands {
                initialise: c("Initialise"),
                start: c("StartCmd"),
                end: c("EndCmd"),
                tool_up: c("Tool_Up"),
                tool_down: c("Tool_Down"),
                page_feed: c("PageFeed"),
                after_cut: c("AfterCutCmd"),
                delimiter: {
                    let d = c("Delimiter");
                    if d.is_empty() {
                        ",".into()
                    } else {
                        d
                    }
                },
                terminator: c("Terminator"),
                velocity: c("Velocity"),
                force: c("Force"),
                select_pen: c("SelectPen"),
                select_tool: c("Select_Tool"),
                cut_absolute: c("Cut_Absolute"),
                cut_relative: c("Cut_Relative"),
                cutoff: c("Cutoff_Tool"),
                up_speed: c("UpSpeed"),
            },
            raw: cfg,
        }
    }
}

impl Catalog {
    /// The catalog embedded in the application.
    pub fn builtin() -> Self {
        let json = include_str!("../data/machines.json");
        let drivers: Vec<DriverFile> = serde_json::from_str(json).unwrap_or_default();
        let mut c = Catalog { drivers };
        c.drivers.push(generic_driver());
        c.sort();
        c
    }

    pub fn sort(&mut self) {
        self.drivers
            .sort_by(|a, b| a.manufacturer.to_lowercase().cmp(&b.manufacturer.to_lowercase()));
    }

    /// Merge drivers into the catalog; same manufacturer name replaces.
    pub fn merge(&mut self, drivers: Vec<DriverFile>) -> usize {
        let mut n = 0;
        for d in drivers {
            n += d.models.len();
            if let Some(existing) = self
                .drivers
                .iter_mut()
                .find(|x| x.manufacturer.eq_ignore_ascii_case(&d.manufacturer))
            {
                *existing = d;
            } else {
                self.drivers.push(d);
            }
        }
        self.sort();
        n
    }

    pub fn summaries(&self) -> Vec<ManufacturerSummary> {
        self.drivers
            .iter()
            .map(|d| ManufacturerSummary {
                manufacturer: d.manufacturer.clone(),
                models: d
                    .models
                    .iter()
                    .map(|m| ModelSummary {
                        name: m.name.clone(),
                        max_width_mm: m.max_width,
                    })
                    .collect(),
            })
            .collect()
    }

    pub fn profile(&self, manufacturer: &str, model: &str) -> Option<MachineProfile> {
        let d = self
            .drivers
            .iter()
            .find(|d| d.manufacturer.eq_ignore_ascii_case(manufacturer))?;
        let m = d.models.iter().find(|m| m.name == model)?;
        Some(d.profile(m))
    }

    /// Find models matching a USB vendor/product id (for auto-detection).
    pub fn find_by_usb(&self, vid: u16, pid: u16) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for d in &self.drivers {
            for m in &d.models {
                let p = d.profile(m);
                if p.vendor_id == Some(vid) && p.product_id == Some(pid) {
                    out.push((d.manufacturer.clone(), m.name.clone()));
                }
            }
        }
        out
    }
}

/// Load driver definitions from a `drivers.pak` (zip of XML) or a single XML file.
pub fn load_driver_source(data: &[u8]) -> Result<Vec<DriverFile>, String> {
    if data.starts_with(b"PK") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
            if !f.name().to_ascii_lowercase().ends_with(".xml") {
                continue;
            }
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            if let Ok(d) = DriverFile::from_xml(&String::from_utf8_lossy(&buf)) {
                if !d.models.is_empty() {
                    out.push(d);
                }
            }
        }
        if out.is_empty() {
            return Err("No driver definitions found in archive".into());
        }
        Ok(out)
    } else {
        Ok(vec![DriverFile::from_xml(&String::from_utf8_lossy(data))?])
    }
}

/// A generic profile for cutters not in the catalog.
fn generic_driver() -> DriverFile {
    let mut config = BTreeMap::new();
    for (k, v) in [
        ("Language", "HPGL"),
        ("CoordinateMode", "ABS"),
        ("XResolution", "0.025"),
        ("YResolution", "0.025"),
        ("UseKnifeCompensation", "1"),
        ("DefaultBladeOffset", "0.25"),
        ("DefaultBaud", "9600"),
    ] {
        config.insert(k.into(), v.into());
    }
    let mut commands = BTreeMap::new();
    for (k, v) in [
        ("Initialise", "IN"),
        ("Tool_Up", "PU"),
        ("Tool_Down", "PD"),
        ("Delimiter", ","),
        ("Terminator", ";"),
        ("Velocity", "VS"),
        ("Force", "FS"),
        ("SelectPen", "SP"),
        ("PageFeed", "PG"),
    ] {
        commands.insert(k.into(), v.into());
    }
    let mut dmpl_cfg = BTreeMap::new();
    dmpl_cfg.insert("Language".to_string(), "DMPL".to_string());
    let mut dmpl_cmd = BTreeMap::new();
    for (k, v) in [
        ("Initialise", ";:H A L0 ECN U"),
        ("Tool_Up", "U"),
        ("Tool_Down", "D"),
        ("Terminator", "0x20"),
        ("PageFeed", "U F @"),
        ("Velocity", "CLEAR"),
        ("Force", "CLEAR"),
        ("SelectPen", "CLEAR"),
    ] {
        dmpl_cmd.insert(k.into(), v.into());
    }
    DriverFile {
        manufacturer: "Generic".into(),
        config,
        commands,
        models: vec![
            ModelDef {
                name: "Generic HPGL cutter".into(),
                max_width: 1200.0,
                max_length: 50_000.0,
                ..Default::default()
            },
            ModelDef {
                name: "Generic DMPL cutter".into(),
                max_width: 1200.0,
                max_length: 50_000.0,
                config: dmpl_cfg,
                commands: dmpl_cmd,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Driver>
  <Manufacturer>VEVOR</Manufacturer>
  <Config>
    <Information>https://example.com</Information>
    <Language>HPGL2</Language>
    <XResolution>0.025</XResolution>
    <YResolution>0.025</YResolution>
    <DefaultBladeOffset>0.36</DefaultBladeOffset>
    <DefaultBaud>9600</DefaultBaud>
  </Config>
  <Commands>
    <Initialise>;:H A L0 ECN U</Initialise>
    <Tool_Up>U</Tool_Up>
    <Tool_Down>D</Tool_Down>
    <Delimiter>,</Delimiter>
    <Terminator>0x20</Terminator>
    <Velocity>VS</Velocity>
  </Commands>
  <Models>
    <Plotter><Name>KH-720</Name><MaxWidth>630</MaxWidth><MaxLength>50000</MaxLength></Plotter>
    <Plotter>
      <Config><DefaultBaud>38400</DefaultBaud></Config>
      <Commands><Initialise>IN</Initialise><Tool_Up>PU</Tool_Up><Tool_Down>PD</Tool_Down><Terminator>;</Terminator><Velocity>CLEAR</Velocity></Commands>
      <Name>KH-720A</Name><MaxWidth>630</MaxWidth><MaxLength>50000</MaxLength>
    </Plotter>
  </Models>
</Driver>"#;

    #[test]
    fn parse_and_merge() {
        let d = DriverFile::from_xml(XML).unwrap();
        assert_eq!(d.models.len(), 2);
        assert!(!d.config.contains_key("Information"));
        let p = d.profile(&d.models[0]);
        assert_eq!(p.commands.initialise, ";:H A L0 ECN U");
        assert_eq!(p.default_baud, 9600);
        assert_eq!(p.commands.velocity, "VS");
        let p = d.profile(&d.models[1]);
        assert_eq!(p.commands.initialise, "IN");
        assert_eq!(p.commands.terminator, ";");
        assert_eq!(p.default_baud, 38400);
        assert_eq!(p.commands.velocity, "", "CLEAR removes inherited value");
        assert_eq!(p.max_width_mm, 630.0);
    }

    #[test]
    fn builtin_catalog_loads() {
        let c = Catalog::builtin();
        assert!(c.drivers.iter().any(|d| d.manufacturer == "Generic"));
    }
}
