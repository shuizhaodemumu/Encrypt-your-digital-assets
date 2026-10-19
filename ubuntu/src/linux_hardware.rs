use crate::hardware::{Hardware, Row};
use anyhow::Result;
use std::{fs, path::Path};
fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}
fn row(values: &[(&str, String)]) -> Row {
    values
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
pub fn collect() -> Result<Hardware> {
    let dmi = Path::new("/sys/class/dmi/id");
    let uuid = crate::platform::uuid()?;
    let fallback = read(dmi.join("product_uuid")).is_empty();
    let product = read(dmi.join("product_name"));
    let mut h = Hardware {
        computer: vec![row(&[
            ("Manufacturer", read(dmi.join("sys_vendor"))),
            ("Model", product.clone()),
        ])],
        base_board: vec![row(&[
            ("Manufacturer", read(dmi.join("board_vendor"))),
            ("Product", read(dmi.join("board_name"))),
            ("SerialNumber", read(dmi.join("board_serial"))),
        ])],
        computer_product: vec![row(&[
            (
                "Name",
                if fallback {
                    format!("{product} [Linux machine-id fallback]")
                } else {
                    product
                },
            ),
            ("Vendor", read(dmi.join("sys_vendor"))),
            ("UUID", uuid),
        ])],
        disks: vec![],
        processors: vec![],
        bios: vec![row(&[("SerialNumber", String::new())])],
        memory: vec![],
        network_adapters: vec![],
    };
    let cpu = read("/proc/cpuinfo");
    let field = |name: &str| {
        cpu.lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                (k.trim() == name).then(|| v.trim().to_string())
            })
            .unwrap_or_default()
    };
    h.processors.push(row(&[
        ("Name", field("model name")),
        ("SerialNumber", field("Serial")),
        (
            "ProcessorId",
            format!(
                "{} family={} model={} stepping={}",
                field("vendor_id"),
                field("cpu family"),
                field("model"),
                field("stepping")
            ),
        ),
    ]));
    if let Ok(disks) = fs::read_dir("/sys/block") {
        for d in disks.flatten() {
            let p = d.path();
            let n = d.file_name().to_string_lossy().into_owned();
            if n.starts_with("loop")
                || n.starts_with("ram")
                || n.starts_with("dm-")
                || n.starts_with("sr")
            {
                continue;
            }
            let real = fs::canonicalize(&p).unwrap_or(p.clone());
            let usb = real.to_string_lossy().contains("/usb") || read(p.join("removable")) == "1";
            h.disks.push(row(&[
                ("Model", read(p.join("device/model"))),
                ("SerialNumber", {
                    let s = read(p.join("device/serial"));
                    if s.is_empty() {
                        read(p.join("serial"))
                    } else {
                        s
                    }
                }),
                (
                    "InterfaceType",
                    if usb { "USB".into() } else { "Internal".into() },
                ),
                ("PNPDeviceID", if usb { format!("USB/{n}") } else { n }),
                ("BusType", if usb { "7".into() } else { "0".into() }),
            ]));
        }
    }
    if let Ok(cards) = fs::read_dir("/sys/class/net") {
        for n in cards.flatten() {
            let p = n.path();
            if !p.join("device").exists() {
                continue;
            }
            h.network_adapters.push(row(&[
                ("Name", n.file_name().to_string_lossy().into_owned()),
                ("MACAddress", read(p.join("address"))),
                (
                    "NetConnectionStatus",
                    if read(p.join("operstate")) == "up" {
                        "2".into()
                    } else {
                        "7".into()
                    },
                ),
            ]));
        }
    }
    // EDAC provides slot labels on some hosts; missing serials stay empty, never fabricated.
    if let Ok(controllers) = fs::read_dir("/sys/devices/system/edac/mc") {
        for c in controllers.flatten() {
            if let Ok(slots) = fs::read_dir(c.path()) {
                for s in slots.flatten() {
                    if !s.file_name().to_string_lossy().starts_with("dimm") {
                        continue;
                    }
                    h.memory.push(row(&[
                        ("DeviceLocator", read(s.path().join("dimm_label"))),
                        ("SerialNumber", String::new()),
                    ]));
                }
            }
        }
    }
    h.validate()?;
    Ok(h)
}
