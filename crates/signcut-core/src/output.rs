//! Sending plot data to cutters.
//!
//! Supported connections:
//! * **USB** (direct, via IOKit/libusb-free `nusb`): cutters that enumerate
//!   as USB printer-class or vendor-specific devices, matched by VID/PID.
//!   On Windows SignCut sends to these through the printer spooler
//!   (`ActAsPrinterDriver`); on macOS we talk to the bulk OUT endpoint
//!   directly, so no printer driver is needed.
//! * **Serial**: USB-serial adapters (CH340, PL2303, FTDI, CP210x) appear as
//!   `/dev/cu.*` on macOS.
//! * **TCP/IP**: raw socket (port 9100 by default).
//! * **macOS printer queue** (CUPS, raw): if the cutter has been added as a
//!   printer in System Settings.
//! * **File**: write a `.plt` file.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum Port {
    Serial {
        path: String,
        baud: u32,
        /// "none" | "hardware" | "software"
        flow: String,
        /// Assert DTR (most cutters: on; VEVOR profiles: off).
        #[serde(default)]
        dtr: bool,
    },
    Usb {
        vendor_id: u16,
        product_id: u16,
        serial: Option<String>,
    },
    Tcp {
        host: String,
        port: u16,
    },
    Printer {
        name: String,
    },
    File {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub port: Port,
    pub label: String,
    pub detail: String,
    /// True when this looks like a cutter (known VID/PID or printer class).
    pub likely_cutter: bool,
}

/// Well-known USB-serial bridge vendor IDs.
fn serial_bridge_name(vid: u16) -> Option<&'static str> {
    match vid {
        0x1a86 => Some("WCH CH340/CH341"),
        0x067b => Some("Prolific PL2303"),
        0x0403 => Some("FTDI"),
        0x10c4 => Some("Silicon Labs CP210x"),
        _ => None,
    }
}

/// Enumerate serial ports, USB devices and printer queues.
pub fn list_ports(known_usb: &dyn Fn(u16, u16) -> bool) -> Vec<PortInfo> {
    let mut out = Vec::new();
    if let Ok(ports) = serialport::available_ports() {
        for p in ports {
            // On macOS every device has a tty.* and a cu.* node; cu.* is the
            // one to use for outgoing connections.
            if cfg!(target_os = "macos") && p.port_name.starts_with("/dev/tty.") {
                continue;
            }
            if p.port_name.contains("Bluetooth") || p.port_name.contains("debug-console") {
                continue;
            }
            let (detail, likely) = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => {
                    let name = u
                        .product
                        .clone()
                        .or_else(|| serial_bridge_name(u.vid).map(String::from))
                        .unwrap_or_else(|| "USB serial".into());
                    (
                        format!("{name} ({:04x}:{:04x})", u.vid, u.pid),
                        serial_bridge_name(u.vid).is_some() || known_usb(u.vid, u.pid),
                    )
                }
                _ => ("Serial port".into(), false),
            };
            out.push(PortInfo {
                label: p.port_name.clone(),
                port: Port::Serial {
                    path: p.port_name,
                    baud: 9600,
                    flow: "hardware".into(),
                    dtr: false,
                },
                detail,
                likely_cutter: likely,
            });
        }
    }
    if let Ok(devs) = nusb::list_devices().wait_compat() {
        for d in devs {
            // Skip hubs and devices already exposed as serial ports.
            if d.class() == 9 || serial_bridge_name(d.vendor_id()).is_some() {
                continue;
            }
            let printer = d.interfaces().any(|i| i.class() == 7);
            let known = known_usb(d.vendor_id(), d.product_id());
            let vendor_class = d.interfaces().any(|i| i.class() == 0xff);
            if !(printer || known || vendor_class) {
                continue;
            }
            let name = d
                .product_string()
                .map(String::from)
                .unwrap_or_else(|| "USB device".into());
            out.push(PortInfo {
                label: format!(
                    "USB: {}{}",
                    d.manufacturer_string().map(|m| format!("{m} ")).unwrap_or_default(),
                    name
                ),
                detail: format!(
                    "{:04x}:{:04x}{}",
                    d.vendor_id(),
                    d.product_id(),
                    if printer { " · printer class" } else { "" }
                ),
                likely_cutter: printer || known,
                port: Port::Usb {
                    vendor_id: d.vendor_id(),
                    product_id: d.product_id(),
                    serial: d.serial_number().map(String::from),
                },
            });
        }
    }
    for name in list_printers() {
        out.push(PortInfo {
            label: format!("Printer queue: {name}"),
            detail: "macOS printer (sent raw)".into(),
            likely_cutter: false,
            port: Port::Printer { name },
        });
    }
    out
}

trait WaitCompat {
    type Out;
    fn wait_compat(self) -> Self::Out;
}
impl<F: nusb::MaybeFuture> WaitCompat for F {
    type Out = F::Output;
    fn wait_compat(self) -> F::Output {
        self.wait()
    }
}

fn list_printers() -> Vec<String> {
    let Ok(out) = std::process::Command::new("lpstat").arg("-e").output() else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

pub struct SendControl<'a> {
    pub cancel: &'a AtomicBool,
    pub progress: &'a mut dyn FnMut(usize, usize),
}

/// Send `data` to `port`. Blocks until done, cancelled or failed.
pub fn send(port: &Port, data: &[u8], ctl: &mut SendControl) -> Result<(), String> {
    match port {
        Port::File { path } => {
            std::fs::write(path, data).map_err(|e| format!("Cannot write {path}: {e}"))?;
            (ctl.progress)(data.len(), data.len());
            Ok(())
        }
        Port::Tcp { host, port } => {
            use std::net::ToSocketAddrs;
            let addr = (host.as_str(), *port)
                .to_socket_addrs()
                .map_err(|e| format!("Cannot resolve {host}: {e}"))?
                .next()
                .ok_or_else(|| format!("Cannot resolve {host}"))?;
            let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5))
                .map_err(|e| format!("Cannot connect to {host}:{port}: {e}"))?;
            s.set_write_timeout(Some(Duration::from_secs(60))).ok();
            write_chunked(&mut s, data, 4096, ctl, None)?;
            s.flush().map_err(|e| format!("Write failed: {e}"))
        }
        Port::Serial { path, baud, flow, dtr } => {
            let flow_control = match flow.as_str() {
                "hardware" => serialport::FlowControl::Hardware,
                "software" => serialport::FlowControl::Software,
                _ => serialport::FlowControl::None,
            };
            let mut sp = serialport::new(path, *baud)
                .data_bits(serialport::DataBits::Eight)
                .parity(serialport::Parity::None)
                .stop_bits(serialport::StopBits::One)
                .flow_control(flow_control)
                .timeout(Duration::from_secs(120))
                .open()
                .map_err(|e| format!("Cannot open {path}: {e}"))?;
            let _ = sp.write_data_terminal_ready(*dtr);
            if flow_control != serialport::FlowControl::Hardware {
                let _ = sp.write_request_to_send(true);
            }
            // Without flow control, pace output to the line rate so small
            // cutter buffers do not overflow.
            let pace = if flow_control == serialport::FlowControl::None {
                Some(Duration::from_secs_f64(256.0 * 10.0 / *baud as f64))
            } else {
                None
            };
            write_chunked(&mut sp, data, 256, ctl, pace)?;
            // Wait for the OS buffer to drain. Not supported by every driver
            // (e.g. ptys); closing the port drains it anyway.
            let _ = sp.flush();
            Ok(())
        }
        Port::Usb {
            vendor_id,
            product_id,
            serial,
        } => send_usb(*vendor_id, *product_id, serial.as_deref(), data, ctl),
        Port::Printer { name } => {
            let tmp = std::env::temp_dir().join(format!("signcut-port-{}.plt", std::process::id()));
            std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
            let out = std::process::Command::new("lp")
                .args(["-d", name, "-o", "raw"])
                .arg(&tmp)
                .output()
                .map_err(|e| format!("Cannot run lp: {e}"))?;
            let _ = std::fs::remove_file(&tmp);
            if !out.status.success() {
                return Err(format!(
                    "Printing to {name} failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
            (ctl.progress)(data.len(), data.len());
            Ok(())
        }
    }
}

fn write_chunked(
    w: &mut dyn Write,
    data: &[u8],
    chunk: usize,
    ctl: &mut SendControl,
    pace: Option<Duration>,
) -> Result<(), String> {
    let mut sent = 0;
    for c in data.chunks(chunk) {
        if ctl.cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        w.write_all(c).map_err(|e| format!("Write failed: {e}"))?;
        sent += c.len();
        (ctl.progress)(sent, data.len());
        if let Some(p) = pace {
            std::thread::sleep(p);
        }
    }
    Ok(())
}

fn send_usb(
    vid: u16,
    pid: u16,
    serial: Option<&str>,
    data: &[u8],
    ctl: &mut SendControl,
) -> Result<(), String> {
    use nusb::descriptors::TransferType;
    use nusb::transfer::{Bulk, Direction, Out};
    let info = nusb::list_devices()
        .wait_compat()
        .map_err(|e| e.to_string())?
        .find(|d| {
            d.vendor_id() == vid
                && d.product_id() == pid
                && (serial.is_none() || d.serial_number() == serial)
        })
        .ok_or_else(|| format!("USB cutter {vid:04x}:{pid:04x} is not connected"))?;
    let dev = info
        .open()
        .wait_compat()
        .map_err(|e| format!("Cannot open USB device: {e}"))?;
    // Find an interface with a bulk OUT endpoint; prefer printer class.
    let cfg = dev
        .active_configuration()
        .map_err(|e| format!("Cannot read USB configuration: {e}"))?;
    let mut candidates: Vec<(u8, u8, u8)> = Vec::new(); // (class, interface, endpoint)
    for intf in cfg.interfaces() {
        for alt in intf.alt_settings() {
            for ep in alt.endpoints() {
                if ep.transfer_type() == TransferType::Bulk && ep.direction() == Direction::Out {
                    candidates.push((alt.class(), intf.interface_number(), ep.address()));
                }
            }
        }
    }
    candidates.sort_by_key(|c| if c.0 == 7 { 0 } else { 1 });
    let (_, intf_no, ep_addr) = *candidates
        .first()
        .ok_or("The USB device has no bulk output endpoint")?;
    drop(cfg);
    let intf = dev
        .detach_and_claim_interface(intf_no)
        .wait_compat()
        .or_else(|_| dev.claim_interface(intf_no).wait_compat())
        .map_err(|e| {
            format!(
                "Cannot claim the USB interface ({e}). If the cutter was added as a printer in \
                 macOS, choose its printer queue instead, or remove it from System Settings › Printers."
            )
        })?;
    let ep = intf
        .endpoint::<Bulk, Out>(ep_addr)
        .map_err(|e| format!("Cannot open USB endpoint: {e}"))?;
    let mut w = ep.writer(4096).with_write_timeout(Duration::from_secs(120));
    write_chunked(&mut w, data, 4096, ctl, None)?;
    w.flush_end().map_err(|e| format!("USB write failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_json_matches_ui() {
        let p: Port = serde_json::from_str(r#"{"kind":"usb","vendorId":1155,"productId":22352,"serial":null}"#).unwrap();
        assert_eq!(p, Port::Usb { vendor_id: 1155, product_id: 22352, serial: None });
        let p: Port = serde_json::from_str(r#"{"kind":"serial","path":"/dev/cu.x","baud":9600,"flow":"hardware","dtr":false}"#).unwrap();
        assert!(matches!(p, Port::Serial { baud: 9600, .. }));
        let s = serde_json::to_string(&Port::Tcp { host: "h".into(), port: 9100 }).unwrap();
        assert_eq!(s, r#"{"kind":"tcp","host":"h","port":9100}"#);
    }

    #[test]
    fn file_port_writes() {
        let dir = std::env::temp_dir().join(format!("scp-test-{}.plt", std::process::id()));
        let cancel = AtomicBool::new(false);
        let mut last = 0;
        let mut prog = |s: usize, _t: usize| last = s;
        let mut ctl = SendControl { cancel: &cancel, progress: &mut prog };
        send(&Port::File { path: dir.display().to_string() }, b"IN;PU0,0;", &mut ctl).unwrap();
        assert_eq!(std::fs::read(&dir).unwrap(), b"IN;PU0,0;");
        assert_eq!(last, 9);
        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn tcp_port_sends() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut s, &mut buf).unwrap();
            buf
        });
        let cancel = AtomicBool::new(false);
        let mut prog = |_s: usize, _t: usize| {};
        let mut ctl = SendControl { cancel: &cancel, progress: &mut prog };
        let data = vec![b'x'; 10_000];
        send(&Port::Tcp { host: "127.0.0.1".into(), port }, &data, &mut ctl).unwrap();
        drop(ctl);
        assert_eq!(h.join().unwrap().len(), 10_000);
    }

    #[cfg(unix)]
    #[test]
    fn serial_port_sends_over_pty() {
        use serialport::SerialPort;
        use std::io::Read;
        let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
        let path = slave.name().unwrap();
        // Keep the slave open so the pty stays alive while we open it again by path.
        let reader = std::thread::spawn(move || {
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            master.set_timeout(Duration::from_millis(500)).unwrap();
            while got.len() < 3000 {
                match master.read(&mut buf) {
                    Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                    _ => break,
                }
            }
            got
        });
        let data: Vec<u8> = b"IN;PU0,0;PD400,400;".iter().cycle().take(3000).copied().collect();
        let cancel = AtomicBool::new(false);
        let mut prog = |_s: usize, _t: usize| {};
        let mut ctl = SendControl { cancel: &cancel, progress: &mut prog };
        send(&Port::Serial { path, baud: 115200, flow: "none".into(), dtr: true }, &data, &mut ctl).unwrap();
        let got = reader.join().unwrap();
        drop(slave);
        assert_eq!(got, data);
    }
}
