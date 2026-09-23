//! Dev-only tool: reports the value distribution a candidate byte range
//! produces when applied to every real `A` line of a live delivery's MSN
//! file -- NOT part of `cargo test` (see crate module doc / this plan's
//! Task 2). Run against a real `timetable_full.zip` extract's
//! `RJTTFnnnMSN.txt` before trusting ANY specific byte range in shipped
//! parser code -- this app's own `49..52` CRS field and the sibling
//! project's documented `44..46` CRS field disagree by 6 bytes on the
//! exact same real fixture (this plan's Judgment Call 2), so a byte range
//! copied from another codebase's documentation is not sufficient
//! evidence on its own.
//!
//! Usage: `cargo run -p schedule-reference --example msn_change_time_probe -- \
//!     /path/to/RJTTFnnnMSN.txt <start> <end>`
//! (0-indexed, half-open, e.g. `63 65` to try the sibling's own hypothesis).
//!
//! A byte range is a good candidate when the printed distribution looks
//! like the sibling project's own independently-measured real shape
//! (`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:74`):
//! mostly single-digit values, a clear mode around 5, and a small number
//! (order of ten, not hundreds) of 98/99 outliers. A range producing
//! double-digit values across most stations, or a huge spread, is wrong.
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let [_, path, start, end] = args.as_slice() else {
        anyhow::bail!("usage: msn_change_time_probe <msn-file-path> <start> <end>");
    };
    let start: usize = start.parse()?;
    let end: usize = end.parse()?;
    let text = std::fs::read_to_string(path)?;

    let mut histogram: BTreeMap<String, u32> = BTreeMap::new();
    let mut lines_checked = 0u32;
    for line in text.lines() {
        if !line.starts_with('A') || line.len() < end {
            continue;
        }
        // Same "reject the FILE-SPEC pseudo-record" guard as
        // parse_msn_a_lines uses on the TIPLOC field, applied here on the
        // CRS field instead, since this probe doesn't parse TIPLOC.
        let crs = line.get(49..52).unwrap_or("").trim();
        if crs.is_empty() {
            continue;
        }
        lines_checked += 1;
        let raw = line[start..end].trim().to_string();
        *histogram.entry(raw).or_insert(0) += 1;
    }

    println!("checked {lines_checked} real 'A' station lines from {path}");
    println!("value distribution for byte range [{start}, {end}):");
    for (value, count) in &histogram {
        println!("  {value:>6} : {count}");
    }
    Ok(())
}
