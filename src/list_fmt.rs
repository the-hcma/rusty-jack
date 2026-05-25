//! Format device lists for terminal or JSON output.

use crate::output_device::OutputDevice;
use crate::system_default::{DeviceList, SystemDefaultInfo};
use anyhow::Result;
use std::io::{self, IsTerminal, Write};

const NO_MONITOR: &str = "-";
pub const ANSI_GREEN: &str = "\x1b[32m";
pub const ANSI_CYAN: &str = "\x1b[36m";
pub const ANSI_DIM: &str = "\x1b[2m";
pub const ANSI_RESET: &str = "\x1b[0m";
const COL_GAP: &str = "  ";

struct TableWidths {
    cols: [usize; 7],
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn pad_cell(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        let mut out = String::with_capacity(width);
        out.push_str(s);
        out.extend(std::iter::repeat_n(' ', width - w));
        out
    }
}

fn compute_widths(devices: &[OutputDevice]) -> TableWidths {
    let mut cols = [
        display_width("IDX"),
        display_width("ACT"),
        display_width("ALIVE"),
        display_width("TRANSPORT"),
        display_width("DEVICE"),
        display_width("MONITOR"),
        display_width("UID"),
    ];

    for (i, d) in devices.iter().enumerate() {
        cols[0] = cols[0].max(display_width(&i.to_string()));
        cols[1] = cols[1].max(if d.is_active { 1 } else { 0 });
        cols[2] = cols[2].max(display_width(if d.is_alive { "yes" } else { "no" }));
        cols[3] = cols[3].max(display_width(&d.transport.to_string()));
        cols[4] = cols[4].max(display_width(&d.name));
        cols[5] = cols[5].max(display_width(
            d.monitor_name.as_deref().unwrap_or(NO_MONITOR),
        ));
        cols[6] = cols[6].max(display_width(&d.uid));
    }

    cols[0] = cols[0].max(3);
    cols[1] = cols[1].max(3);
    cols[2] = cols[2].max(5);
    cols[3] = cols[3].max(11);

    TableWidths { cols }
}

#[allow(dead_code)] // used by alignment tests
fn column_starts(widths: &TableWidths) -> Vec<usize> {
    let gap = COL_GAP.len();
    let mut starts = Vec::with_capacity(7);
    let mut pos = 0;
    for (i, &width) in widths.cols.iter().enumerate() {
        starts.push(pos);
        pos += width;
        if i + 1 < widths.cols.len() {
            pos += gap;
        }
    }
    starts
}

fn render_cells(cells: [&str; 7], widths: &TableWidths) -> String {
    let mut out = String::new();
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push_str(COL_GAP);
        }
        out.push_str(&pad_cell(cell, widths.cols[i]));
    }
    out
}

fn monitor_label(device: &OutputDevice) -> &str {
    device.monitor_name.as_deref().unwrap_or(NO_MONITOR)
}

fn active_marker(device: &OutputDevice) -> &str {
    if device.is_active { ">" } else { "" }
}

fn format_header(w: &TableWidths) -> String {
    render_cells(
        ["IDX", "ACT", "ALIVE", "TRANSPORT", "DEVICE", "MONITOR", "UID"],
        w,
    )
}

fn format_row(index: usize, device: &OutputDevice, w: &TableWidths) -> String {
    render_cells(
        [
            &index.to_string(),
            active_marker(device),
            if device.is_alive { "yes" } else { "no" },
            &device.transport.to_string(),
            &device.name,
            monitor_label(device),
            &device.uid,
        ],
        w,
    )
}

fn stdout_supports_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// True when stdout is a TTY and `NO_COLOR` is unset.
#[must_use]
pub fn terminal_supports_color() -> bool {
    stdout_supports_color()
}

fn colorize_table_row(row: &str, device: &OutputDevice, use_color: bool) -> String {
    if !use_color {
        return row.to_string();
    }
    if device.is_active {
        format!("{ANSI_GREEN}{row}{ANSI_RESET}")
    } else if !device.is_selectable() {
        format!("{ANSI_DIM}{row}{ANSI_RESET}")
    } else {
        row.to_string()
    }
}

/// Build a human-readable table (without trailing newline).
#[must_use]
pub fn format_table(devices: &[OutputDevice]) -> String {
    format_table_with_color(devices, false)
}

/// Build a table; highlight the active output device in green when `use_color` is true.
#[must_use]
pub fn format_table_with_color(devices: &[OutputDevice], use_color: bool) -> String {
    if devices.is_empty() {
        return String::new();
    }

    let w = compute_widths(devices);
    let mut lines = Vec::with_capacity(devices.len() + 1);
    lines.push(format_header(&w));

    for (index, device) in devices.iter().enumerate() {
        let row = format_row(index, device, &w);
        lines.push(colorize_table_row(&row, device, use_color));
    }

    lines.join("\n")
}

/// Print the device table only (no virtual-default footer).
pub fn print_device_table(list: &DeviceList) -> Result<()> {
    let mut out = io::stdout().lock();
    if list.devices.is_empty() {
        writeln!(out, "No output devices found.")?;
        return Ok(());
    }

    writeln!(
        out,
        "{}",
        format_table_with_color(&list.devices, stdout_supports_color())
    )?;
    Ok(())
}

/// Print virtual system-default details when the default is a router omitted from the table.
pub fn print_virtual_default_footer(list: &DeviceList) -> Result<()> {
    let Some(info) = &list.system_default else {
        return Ok(());
    };

    let mut out = io::stdout().lock();
    writeln!(out)?;
    writeln!(out, "{}", format_system_default_block(info))?;
    writeln!(out, "  (`>` marks the physical output row above)")?;
    Ok(())
}

/// Print a human-readable table to stdout (list command — table only).
pub fn print_table(list: &DeviceList, hdmi_only: bool) -> Result<()> {
    let mut out = io::stdout().lock();
    if list.devices.is_empty() {
        let scope = if hdmi_only {
            "HDMI-class output"
        } else {
            "output"
        };
        writeln!(out, "No {scope} devices found.")?;
        return Ok(());
    }

    drop(out);
    print_device_table(list)
}

/// Format `label: value` rows with values aligned in one column.
#[must_use]
pub fn format_detail_rows(indent: &str, rows: &[(&str, &str)]) -> Vec<String> {
    if rows.is_empty() {
        return vec![];
    }
    let width = rows
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|(label, value)| format!("{indent}{label:width$}: {value}", width = width))
        .collect()
}

/// Format a titled block of aligned `label: value` rows.
#[must_use]
pub fn format_labeled_section(title: &str, indent: &str, rows: &[(&str, &str)]) -> String {
    let mut lines = vec![title.to_string()];
    lines.extend(format_detail_rows(indent, rows));
    lines.join("\n")
}

/// Format the virtual system-default footer block (shared with `status`).
pub fn format_system_default_block(info: &SystemDefaultInfo) -> String {
    let mut rows: Vec<(&str, String)> = Vec::new();

    if let Some(router) = &info.router {
        let version = info
            .driver
            .as_ref()
            .and_then(|d| d.version.as_deref())
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        rows.push(("router", format!("{router}{version}")));
    }

    rows.push(("device", info.name.clone()));
    rows.push(("uid", info.uid.clone()));
    rows.push(("transport", info.transport.to_string()));

    if let Some(m) = &info.manufacturer {
        rows.push(("manufacturer", m.clone()));
    }
    if let Some(model) = &info.model_uid {
        rows.push(("model uid", model.clone()));
    }

    if let Some(driver) = &info.driver {
        rows.push(("driver", driver.bundle_id.clone()));
        if let Some(version) = &driver.version {
            rows.push(("driver ver", version.clone()));
        }
        rows.push(("driver path", driver.install_path.clone()));
    }

    if let Some(label) = &info.routed_to_label {
        rows.push(("routing to", label.clone()));
    } else if let Some(uid) = &info.routed_to_uid {
        rows.push(("routing to", uid.clone()));
    }

    let borrowed: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format!("\n{}", format_labeled_section("System default (virtual)", "  ", &borrowed))
}

/// Print JSON to stdout.
pub fn print_json(list: &DeviceList) -> Result<()> {
    let value = serde_json::to_string_pretty(list)?;
    println!("{value}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_device::OutputDevice;
    use crate::transport::TransportKind;

    fn sample_devices() -> Vec<OutputDevice> {
        vec![
            OutputDevice {
                id: 1,
                uid: "BuiltInSpeakerDevice".into(),
                name: "Built-in Output".into(),
                transport: TransportKind::BuiltIn,
                is_alive: true,
                is_default: false,
                is_active: false,
                monitor_name: None,
            },
            OutputDevice {
                id: 2,
                uid: "AppleHDAEngineOutputDP:0,1,0,1,0:0:{AC10-A120-30594A4C}".into(),
                name: "HDMI".into(),
                transport: TransportKind::Hdmi,
                is_alive: true,
                is_default: false,
                is_active: true,
                monitor_name: Some("DELL U3219Q".into()),
            },
            OutputDevice {
                id: 3,
                uid: "AppleHDAEngineOutputDP:0,1,0,1,4:1:{AC10-4273-424D414C}".into(),
                name: "DisplayPort".into(),
                transport: TransportKind::DisplayPort,
                is_alive: true,
                is_default: false,
                is_active: false,
                monitor_name: Some("DELL U3223QE".into()),
            },
        ]
    }

    fn split_cells(line: &str, widths: &TableWidths) -> Vec<String> {
        column_starts(widths)
            .into_iter()
            .enumerate()
            .map(|(i, start)| {
                let end = if i + 1 < widths.cols.len() {
                    column_starts(widths)[i + 1] - COL_GAP.len()
                } else {
                    line.len()
                };
                line.get(start..end)
                    .unwrap_or("")
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn test_all_rows_share_column_offsets() {
        let devices = sample_devices();
        let table = format_table(&devices);
        let w = compute_widths(&devices);
        let expected = column_starts(&w);
        let lines: Vec<&str> = table.lines().collect();

        for line in &lines {
            for (col, &start) in expected.iter().enumerate() {
                let cell = split_cells(line, &w)[col].clone();
                assert!(
                    line.len() >= start,
                    "line too short for col {col}: {line:?}"
                );
                let padded = pad_cell(&cell, w.cols[col]);
                assert!(
                    line[start..].starts_with(&padded),
                    "col {col} misaligned in: {line}"
                );
            }
        }
    }

    #[test]
    fn test_table_content() {
        let devices = sample_devices();
        let w = compute_widths(&devices);
        let table = format_table(&devices);
        let lines: Vec<&str> = table.lines().collect();

        let header = split_cells(lines[0], &w);
        assert_eq!(header[0], "IDX");
        assert_eq!(header[6], "UID");

        let hdmi = split_cells(lines[2], &w);
        assert_eq!(hdmi[1], ">");
        assert_eq!(hdmi[3], "hdmi");
        assert_eq!(hdmi[5], "DELL U3219Q");
    }

    #[test]
    fn test_non_selectable_row_dimmed() {
        let mut devices = sample_devices();
        devices.push(OutputDevice {
            id: 4,
            uid: "zoom.us:0".into(),
            name: "ZoomAudioDevice".into(),
            transport: TransportKind::Virtual,
            is_alive: true,
            is_default: false,
            is_active: false,
            monitor_name: None,
        });
        let table = format_table_with_color(&devices, true);
        let zoom_line = table.lines().last().unwrap();
        assert!(zoom_line.starts_with(ANSI_DIM));
    }

    #[test]
    fn test_active_row_colored_when_enabled() {
        let table = format_table_with_color(&sample_devices(), true);
        let active_line = table.lines().nth(2).unwrap();
        assert!(active_line.starts_with(ANSI_GREEN));
        assert!(active_line.ends_with(ANSI_RESET));
    }

    #[test]
    fn test_no_color_when_disabled() {
        let table = format_table_with_color(&sample_devices(), false);
        assert!(!table.contains(ANSI_GREEN));
    }

    #[test]
    fn test_detail_rows_align_value_column() {
        let rows = [
            ("config volume", "13%"),
            ("volume", "13%"),
            ("note", "hello"),
        ];
        let lines = format_detail_rows("  ", &rows);
        let value_starts: Vec<usize> = lines
            .iter()
            .map(|line| line.find(": ").unwrap() + 2)
            .collect();
        assert!(value_starts.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn test_system_default_block() {
        use crate::system_default::{HalDriverInfo, SystemDefaultInfo};
        use crate::transport::TransportKind;

        let block = format_system_default_block(&SystemDefaultInfo {
            uid: "EQMOutputCapture".into(),
            name: "DELL U3219Q (eqMac)".into(),
            transport: TransportKind::Virtual,
            manufacturer: Some("Bitgapp Ltd".into()),
            model_uid: Some("EQMOutputCapture".into()),
            router: Some("eqMac".into()),
            driver: Some(HalDriverInfo {
                name: "eqMac".into(),
                bundle_id: "com.bitgapp.eqmac.driver".into(),
                version: Some("2.6.0".into()),
                install_path: "/Library/Audio/Plug-Ins/HAL/eqMac.driver".into(),
            }),
            routed_to_uid: Some("hdmi-uid".into()),
            routed_to_label: Some("HDMI (DELL U3219Q)".into()),
        });

        assert!(block.contains("eqMac 2.6.0"));
        assert!(block.contains("com.bitgapp.eqmac.driver"));
        assert!(block.contains("routing to"));
        assert!(block.contains("HDMI (DELL U3219Q)"));
        let detail_lines: Vec<&str> = block
            .lines()
            .filter(|line| line.starts_with("  ") && line.contains(": "))
            .collect();
        let value_starts: Vec<usize> = detail_lines
            .iter()
            .map(|line| line.find(": ").unwrap() + 2)
            .collect();
        assert!(value_starts.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn test_print_json_includes_active_flag() {
        let list = DeviceList {
            devices: vec![OutputDevice {
                id: 42,
                uid: "uid".into(),
                name: "Test".into(),
                transport: TransportKind::Hdmi,
                is_alive: true,
                is_default: false,
                is_active: true,
                monitor_name: Some("LG TV".into()),
            }],
            system_default: None,
        };
        let json = serde_json::to_string(&list).unwrap();
        assert!(json.contains("\"is_active\":true"));
        assert!(json.contains("\"devices\""));
    }
}
