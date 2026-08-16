//! Deterministic text and JSON output for an [`Analysis`](crate::Analysis).

use crate::control_flow::{Analysis, AnalysisItem};
use crate::regions::Classification;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Ca65,
    Annotated,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatOptions {
    pub show_addresses: bool,
    pub show_bytes: bool,
    pub use_labels: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            show_addresses: false,
            show_bytes: true,
            use_labels: true,
        }
    }
}

pub fn format(analysis: &Analysis, output: OutputFormat, options: FormatOptions) -> String {
    match output {
        OutputFormat::Ca65 => format_ca65(analysis, options),
        OutputFormat::Annotated => format_annotated(analysis, options),
        OutputFormat::Json => format_json(analysis),
    }
}

pub fn format_ca65(analysis: &Analysis, options: FormatOptions) -> String {
    let mut out = String::new();
    for item in &analysis.items {
        if options.use_labels {
            if let Some(label) = analysis.labels.get(&item.address()) {
                out.push_str(label);
                out.push_str(":\n");
            }
        }
        match item {
            AnalysisItem::Instruction(i) => {
                let mnemonic = i.mnemonic();
                let operand = if options.use_labels {
                    i.operand.format(Some(&analysis.labels))
                } else {
                    i.operand.format(None)
                };
                out.push_str("    ");
                out.push_str(mnemonic);
                if !operand.is_empty() {
                    out.push(' ');
                    out.push_str(&operand);
                }
                if options.show_bytes {
                    out.push_str(
                        "                         "
                            .get(..(1 + i.bytes.len() * 3).min(25))
                            .unwrap_or(" "),
                    );
                    out.push_str("; ");
                    out.push_str(&hex_bytes(&i.bytes));
                }
                out.push('\n');
            }
            AnalysisItem::Data(d) => {
                out.push_str("    .byte ");
                out.push_str(
                    &d.bytes
                        .iter()
                        .map(|b| format!("${b:02X}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                if d.classification != Classification::Data {
                    out.push_str(" ; uncertain");
                }
                out.push('\n');
            }
        }
    }
    out
}

pub fn format_annotated(analysis: &Analysis, options: FormatOptions) -> String {
    let mut out = String::new();
    for item in &analysis.items {
        let address = format!("${:04X}", item.address());
        let bytes = if options.show_bytes {
            format!("{:<11}", hex_bytes(item.bytes()))
        } else {
            String::new()
        };
        match item {
            AnalysisItem::Instruction(i) => {
                let operand = if options.use_labels {
                    i.operand.format(Some(&analysis.labels))
                } else {
                    i.operand.format(None)
                };
                out.push_str(&format!(
                    "{}  {}  {} {}\n",
                    if options.show_addresses {
                        address
                    } else {
                        String::new()
                    },
                    bytes,
                    i.mnemonic(),
                    operand
                ));
            }
            AnalysisItem::Data(d) => {
                out.push_str(&format!(
                    "{}  {}  .byte ; {:?}\n",
                    if options.show_addresses {
                        address
                    } else {
                        String::new()
                    },
                    bytes,
                    d.provenance
                ));
            }
        }
    }
    out
}

pub fn format_json(analysis: &Analysis) -> String {
    let mut out = String::from("{\"origin\":");
    out.push_str(&analysis.origin.to_string());
    out.push_str(",\"labels\":{");
    for (i, (address, label)) in analysis.labels.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&format!("{address:04X}"));
        out.push_str("\":\"");
        out.push_str(&json_escape(label));
        out.push('"');
    }
    out.push_str("},\"items\":[");
    for (index, item) in analysis.items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        match item {
            AnalysisItem::Instruction(i) => {
                out.push_str(&format!("{{\"kind\":\"instruction\",\"address\":{},\"opcode\":{},\"mnemonic\":\"{}\",\"bytes\":[{}],\"target\":{} ,\"operand\":\"{}\"}}", i.address, i.opcode, i.mnemonic(), i.bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(","), i.target.map_or("null".into(), |v| v.to_string()), json_escape(&i.operand.format(Some(&analysis.labels)))));
            }
            AnalysisItem::Data(d) => {
                out.push_str(&format!("{{\"kind\":\"data\",\"address\":{},\"bytes\":[{}],\"classification\":\"{:?}\",\"provenance\":\"{:?}\"}}", d.address, d.bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(","), d.classification, d.provenance));
            }
        }
    }
    out.push_str("],\"diagnostics\":[");
    for (i, diagnostic) in analysis.diagnostics.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"severity\":\"{:?}\",\"code\":\"{}\",\"message\":\"{}\"}}",
            diagnostic.severity,
            diagnostic.code,
            json_escape(&diagnostic.message)
        ));
    }
    out.push_str("]}");
    out
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn json_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| match c {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            c if c.is_control() => format!("\\u{:04x}", c as u32).chars().collect(),
            c => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_flow::{linear_sweep, LinearSweepOptions};
    #[test]
    fn formats_json_and_ca65() {
        let a = linear_sweep(&[0xa9, 1, 0x60], 0x8000, LinearSweepOptions::default()).unwrap();
        assert!(format_ca65(&a, FormatOptions::default()).contains("LDA #$01"));
        assert!(format_json(&a).contains("instruction"));
    }
}
