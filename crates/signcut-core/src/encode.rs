//! Turn a [`Plan`] into the byte stream a cutter understands.
//!
//! The format is table-driven from the model's driver definition exactly like
//! SignCut does it: every statement is `command + value(s) + terminator`, and
//! each point is its own statement (`PU100,200;PD300,400;` for HPGL,
//! `U100,200 D300,400 ` for the DMPL dialect used by VEVOR's D-type boards).

use crate::drivers::{Commands, MachineProfile};
use crate::plan::{AfterCut, Op, Plan};
use crate::geometry::Pt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EncodeOptions {
    /// Send speed/force/tool commands (SignCut: "Use software force and speed").
    pub send_speed_force: bool,
    /// Override the axis swap derived from the driver (SwapAxis / Rotate90).
    pub swap_xy: Option<bool>,
    pub after_cut: AfterCut,
    /// Extra feed past the job when `after_cut == FeedPastJob` (mm).
    pub feed_extra: f64,
    /// Send the driver's page-feed command at the end of the job.
    pub send_page_feed: bool,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            send_speed_force: true,
            swap_xy: None,
            after_cut: AfterCut::ReturnToOrigin,
            feed_extra: 50.0,
            send_page_feed: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Encoded {
    /// Data to send. Separate chunks are separated by user pauses.
    pub chunks: Vec<Vec<u8>>,
    /// Messages to show before chunk i+1.
    pub pauses: Vec<String>,
    pub total_bytes: usize,
}

impl Encoded {
    pub fn concat(&self) -> Vec<u8> {
        self.chunks.concat()
    }
}

/// Resolve SignCut placeholders in terminators/delimiters/commands.
pub fn unescape(s: &str) -> String {
    let s = s.trim();
    if s == "-" {
        return String::new();
    }
    if s == "0x20" {
        return " ".into();
    }
    s.replace("%newline%", "\r\n")
        .replace("%space%", " ")
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("0x20", " ")
}

pub fn effective_swap(p: &MachineProfile, o: &EncodeOptions) -> bool {
    o.swap_xy.unwrap_or_else(|| {
        // Cutters with the carriage home on the left (Rotate 0°) use the
        // other axis order; SwapAxis toggles it again.
        let rotate90 = p
            .raw
            .get("Rotate90")
            .map(|v| v.trim() != "0")
            .unwrap_or(true);
        p.swap_axis ^ !rotate90
    })
}

struct Writer<'a> {
    out: Vec<u8>,
    c: &'a Commands,
    term: String,
    delim: String,
    lang: String,
    relative: bool,
    swap: bool,
    xres: f64,
    yres: f64,
    last: (i64, i64),
    /// Last emitted statement was a pen-up move to `last`.
    last_up: bool,
}

impl<'a> Writer<'a> {
    fn raw(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }
    /// Emit a configured command followed by the terminator.
    fn cmd(&mut self, cmd: &str) {
        let c = unescape(cmd);
        if c.is_empty() {
            return;
        }
        self.raw(&c);
        let t = self.term.clone();
        self.raw(&t);
    }
    fn cmd_value(&mut self, cmd: &str, v: i64) {
        let c = unescape(cmd);
        if c.is_empty() {
            return;
        }
        let t = self.term.clone();
        self.raw(&format!("{c}{v}{t}"));
    }
    fn units(&self, p: Pt) -> (i64, i64) {
        let x = (p.x / self.xres).round().max(0.0) as i64;
        let y = (p.y / self.yres).round().max(0.0) as i64;
        if self.swap {
            (y, x)
        } else {
            (x, y)
        }
    }
    fn mv(&mut self, down: bool, p: Pt) {
        let (x, y) = self.units(p);
        if !down && self.last_up && self.last == (x, y) {
            return;
        }
        let cmd = if down {
            unescape(&self.c.tool_down)
        } else {
            unescape(&self.c.tool_up)
        };
        let s = match self.lang.as_str() {
            "GCODE" => {
                // mm with 3 decimals; pen state via the configured commands.
                format!("{cmd} X{:.3} Y{:.3}\n", x as f64 * self.xres, y as f64 * self.yres)
            }
            "PIBOT" => format!("{cmd}{x},{y}{}", self.term),
            _ => {
                let (ox, oy) = if self.relative {
                    (x - self.last.0, y - self.last.1)
                } else {
                    (x, y)
                };
                format!("{cmd}{ox}{}{oy}{}", self.delim, self.term)
            }
        };
        self.last = (x, y);
        self.last_up = !down;
        self.raw(&s);
    }
}

pub fn encode(plan: &Plan, p: &MachineProfile, o: &EncodeOptions) -> Encoded {
    let c = &p.commands;
    let mut w = writer(p, o);
    let mut enc = Encoded::default();

    w.cmd(&c.initialise);
    w.cmd(&c.start);

    let mut last_pt = Pt::default();
    for op in &plan.ops {
        match op {
            Op::Tool { tool, speed, force } => {
                if !o.send_speed_force {
                    continue;
                }
                let foison = c.velocity.contains("FOISON") || c.force.contains("FOISON");
                if p.pens > 1 || !c.select_pen.is_empty() {
                    if foison {
                        w.raw(&format!("SP{tool};"));
                    } else {
                        w.cmd_value(&c.select_pen, *tool as i64);
                    }
                }
                if let Some(v) = speed {
                    let v = clamp_opt(*v, p.min_speed, p.max_speed).round() as i64;
                    if foison {
                        w.raw(&format!("VS{v};"));
                    } else {
                        w.cmd_value(&c.velocity, v);
                    }
                }
                if let Some(f) = force {
                    let f = clamp_opt(*f, p.min_force, p.max_force).round() as i64;
                    if foison {
                        w.raw(&format!("P1;!FS{f};"));
                    } else {
                        w.cmd_value(&c.force, f);
                    }
                }
            }
            Op::Pause { message } => {
                // Lift and finish the current chunk; the app waits for the user.
                w.mv(false, last_pt);
                enc.chunks.push(std::mem::take(&mut w.out));
                enc.pauses.push(message.clone());
            }
            Op::Cut { pts, .. } => {
                if pts.len() < 2 {
                    continue;
                }
                w.mv(false, pts[0]);
                let mut prev = w.units(pts[0]);
                for q in &pts[1..] {
                    // Skip points that round to the same device position.
                    let u = w.units(*q);
                    if u == prev {
                        continue;
                    }
                    prev = u;
                    w.mv(true, *q);
                }
                last_pt = *pts.last().unwrap();
            }
        }
    }

    // End of job.
    w.mv(false, last_pt);
    match o.after_cut {
        AfterCut::ReturnToOrigin => w.mv(false, Pt::default()),
        AfterCut::FeedPastJob => w.mv(false, Pt::new(plan.end_x + o.feed_extra.max(0.0), 0.0)),
        AfterCut::Stay => {}
    }
    w.cmd(&c.after_cut);
    if o.send_page_feed {
        w.cmd(&c.page_feed);
    }
    w.cmd(&c.end);
    // DMPL-style streams are closed with '@'.
    let init = unescape(&c.initialise);
    if init.starts_with(";:") {
        let tail = String::from_utf8_lossy(&w.out[w.out.len().saturating_sub(16)..]).to_string();
        if !tail.contains('@') {
            let t = w.term.clone();
            w.raw(&format!("@{t}"));
        }
    }
    enc.chunks.push(w.out);
    enc.total_bytes = enc.chunks.iter().map(|c| c.len()).sum();
    enc
}

fn clamp_opt(v: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let mut v = v;
    if let Some(m) = min {
        v = v.max(m);
    }
    if let Some(m) = max {
        if m > 0.0 {
            v = v.min(m);
        }
    }
    v
}

fn writer<'a>(p: &'a MachineProfile, o: &EncodeOptions) -> Writer<'a> {
    let c = &p.commands;
    let d = unescape(&c.delimiter);
    Writer {
        out: Vec::new(),
        c,
        term: unescape(&c.terminator),
        delim: if d.is_empty() { ",".into() } else { d },
        lang: p.language.clone(),
        relative: p.relative_coordinates,
        swap: effective_swap(p, o),
        xres: if p.x_resolution > 0.0 { p.x_resolution } else { 0.025 },
        yres: if p.y_resolution > 0.0 { p.y_resolution } else { 0.025 },
        last: (0, 0),
        last_up: false,
    }
}

/// Move the material forward by `mm` and back (SignCut "Testfeed").
pub fn test_feed(p: &MachineProfile, mm: f64) -> Vec<u8> {
    let o = EncodeOptions::default();
    let mut w = writer(p, &o);
    let c = &p.commands;
    w.cmd(&c.initialise);
    w.cmd(&c.start);
    w.mv(false, Pt::new(mm, 0.0));
    w.mv(false, Pt::new(0.0, 0.0));
    if unescape(&c.initialise).starts_with(";:") {
        let t = w.term.clone();
        w.raw(&format!("@{t}"));
    }
    w.out
}

/// SignCut-style test cut: a square of `size` mm with a triangle inside
/// (local coordinates; place it with the job transform).
pub fn test_cut_paths(size: f64) -> Vec<crate::plan::JobPath> {
    let s = size.max(5.0);
    let m = s * 0.2;
    vec![
        crate::plan::JobPath {
            d: format!("M{m} {b}L{c} {b}L{h} {t}Z", b = s - m, c = s - m, h = s / 2.0, t = m),
            color: "#000000".into(),
        },
        crate::plan::JobPath {
            d: format!("M0 0H{s}V{s}H0Z"),
            color: "#000000".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::DriverFile;
    use crate::plan::{plan, CutSettings, JobObject, JobPath, Placement};
    use crate::geometry::Affine;

    fn vevor() -> DriverFile {
        DriverFile::from_xml(
            r#"<Driver><Manufacturer>VEVOR</Manufacturer>
            <Config><Language>HPGL2</Language><XResolution>0.025</XResolution><YResolution>0.025</YResolution></Config>
            <Commands><Initialise>;:H A L0 ECN U</Initialise><Tool_Up>U</Tool_Up><Tool_Down>D</Tool_Down>
              <PageFeed>U F @</PageFeed><Delimiter>,</Delimiter><Terminator>0x20</Terminator></Commands>
            <Models>
              <Plotter><Name>KH-720</Name><MaxWidth>630</MaxWidth><MaxLength>50000</MaxLength></Plotter>
              <Plotter><Commands><Initialise>IN</Initialise><PageFeed>PG</PageFeed><Terminator>;</Terminator>
                <Tool_Up>PU</Tool_Up><Tool_Down>PD</Tool_Down><Velocity>VS</Velocity><Force>FS</Force><SelectPen>SP</SelectPen></Commands>
                <Name>KH-720A</Name><MaxWidth>630</MaxWidth><MaxLength>50000</MaxLength></Plotter>
            </Models></Driver>"#,
        )
        .unwrap()
    }

    fn square_plan(speed: Option<f64>) -> Plan {
        let s = CutSettings {
            material_width: 100.0,
            use_blade_offset: false,
            overcut: 0.0,
            placement: Placement::Origin,
            speed,
            force: speed.map(|_| 80.0),
            ..Default::default()
        };
        plan(
            &[JobObject {
                paths: vec![JobPath { d: "M0 0h10v10h-10z".into(), color: "#000".into() }],
                transform: Affine::IDENTITY,
            }],
            &s,
            None,
        )
    }

    #[test]
    fn dmpl_dialect_for_vevor_d_board() {
        let d = vevor();
        let p = d.profile(&d.models[0]);
        let out = String::from_utf8(encode(&square_plan(None), &p, &EncodeOptions::default()).concat()).unwrap();
        assert!(out.starts_with(";:H A L0 ECN U U0,0 D0,400 D400,400 D400,0 D0,0 U0,0 @ "), "{out}");
        assert_eq!(out.matches('@').count(), 1, "{out}");
    }

    #[test]
    fn hpgl_dialect_with_speed_force() {
        let d = vevor();
        let p = d.profile(&d.models[1]);
        let out = String::from_utf8(encode(&square_plan(Some(30.0)), &p, &EncodeOptions::default()).concat()).unwrap();
        assert!(out.starts_with("IN;SP1;VS30;FS80;PU0,0;PD0,400;PD400,400;PD400,0;PD0,0;PU0,0;"), "{out}");
        assert!(!out.contains('@'));
    }

    #[test]
    fn unescape_placeholders() {
        assert_eq!(unescape("0x20"), " ");
        assert_eq!(unescape(";%newline%"), ";\r\n");
        assert_eq!(unescape("%space%"), " ");
        assert_eq!(unescape("G90\\nM6"), "G90\nM6");
    }

    #[test]
    fn swap_axes() {
        let d = vevor();
        let p = d.profile(&d.models[1]);
        let o = EncodeOptions { swap_xy: Some(true), send_speed_force: false, ..Default::default() };
        let plan = plan(
            &[JobObject {
                paths: vec![JobPath { d: "M0 90L10 90".into(), color: "#000".into() }],
                transform: Affine::IDENTITY,
            }],
            &CutSettings { material_width: 100.0, use_blade_offset: false, ..Default::default() },
            None,
        );
        let out = String::from_utf8(encode(&plan, &p, &o).concat()).unwrap();
        assert!(out.contains("PU400,0;PD400,400;"), "{out}");
    }
}
