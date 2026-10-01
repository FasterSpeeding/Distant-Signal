//! Sanity checks on an extracted CIF delivery before it is accepted
//! (docs/schedule-feed-sftp.md, "Delivery checks").
//!
//! The SFTP push account can be used by anyone holding its password, and
//! DTD cannot pin our host key, so a delivery is not trusted just because
//! it arrived. Before `delivery::ensure_extracted` marks a delivery
//! complete (and so before `schedule-reference` can publish from it), the
//! extracted files must look like the real full CIF extract and must not be
//! a step backwards from the last accepted one. A failure is a
//! [`RejectedZip`]: the delivery is quarantined with the reason, counted in
//! `schedule_feed_zip_rejected_total` (the `DistantSignalScheduleFeedZipRejected`
//! alert), and the previous timetable stays in service.
//!
//! What a real delivery looks like (2026-09-28 to 09-30, read-only from the
//! PVC): `RJTTF<seq>MCA.txt` is ~724 MB of fixed 80-column CRLF records,
//! one `HD` first (update indicator `F`, full extract), then `TI`, `AA`,
//! `BS`/`BX`/`LO`/`LI`/`CR`/`LT`, and one `ZZ` last; ~505,000 `BS` and
//! ~12,100 `TI`, changing by under 0.3% a day. `RJTTF<seq>MSN.txt` opens
//! with a `/!! Generated: dd/mm/yyyy` banner (the export date, the
//! delivery day). The `HD` record's own dates are NOT checked: they are a
//! fixed 2011 dataset identity carried forward in every extract (see
//! docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md),
//! so a check on them would reject every real delivery.

use std::io::BufRead;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::delivery::RejectedZip;

/// Written into each accepted delivery directory (hidden, so neither
/// discovery nor pruning treats it as a delivery file) so the next
/// delivery can be compared with this one without re-reading its MCA.
pub(crate) const STATS_FILE: &str = ".cif-stats.json";

/// The thresholds (`CIF_*` env vars, `scheduleFeed.ingest.cifChecks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CifChecks {
    /// Reject a delivery whose MSN `Generated` date is more than this many
    /// days before the delivery's own date: a replayed old extract. 0
    /// disables the check.
    pub max_generated_age_days: u32,
    /// Reject an MCA with fewer `BS` (schedule) records. 0 disables.
    pub min_schedules: u64,
    /// Reject a delivery whose `BS` or `TI` count fell by more than this
    /// percentage since the last accepted delivery. 0 disables.
    pub max_drop_percent: u32,
}

/// What [`inspect`] learned about one delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CifStats {
    /// The MSN banner's `Generated` date.
    pub generated: NaiveDate,
    /// The banner's `Sequence`, when present (informational).
    pub sequence: Option<u32>,
    /// `BS` records in the MCA.
    pub schedules: u64,
    /// `TI` records in the MCA.
    pub tiplocs: u64,
}

/// Record types a CIF file may contain (the CIF End User Specification).
/// Anything else in the MCA is not CIF.
const RECORD_TYPES: [&[u8; 2]; 14] = [
    b"HD", b"TI", b"TA", b"TD", b"AA", b"BS", b"BX", b"TN", b"LO", b"LI", b"CR", b"LT", b"LN",
    b"ZZ",
];

/// Byte offset of the `HD` record's update indicator (`F` full, `U`
/// update); column 47 of the CIF spec.
const HD_UPDATE_INDICATOR: usize = 46;

fn reject(reason: impl Into<String>) -> anyhow::Error {
    RejectedZip(reason.into()).into()
}

/// The one file in `dir` named `RJTTF*<suffix>`.
fn one_file(dir: &Path, suffix: &str) -> anyhow::Result<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("RJTTF") && name.ends_with(suffix) && entry.file_type()?.is_file() {
            found.push(entry.path());
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(reject(format!("no RJTTF*{suffix} in the delivery"))),
        n => Err(reject(format!("{n} RJTTF*{suffix} files in the delivery"))),
    }
}

/// Reads an extracted delivery in `dir`. A [`RejectedZip`] when it is not
/// shaped like a full CIF extract; another error for IO trouble.
pub(crate) fn inspect(dir: &Path) -> anyhow::Result<CifStats> {
    let mca = one_file(dir, "MCA.txt")?;
    let msn = one_file(dir, "MSN.txt")?;

    let mut reader = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&mca)?);
    let mut line = Vec::with_capacity(128);
    let (mut records, mut schedules, mut tiplocs, mut headers, mut trailers) = (0u64, 0, 0, 0, 0);
    let mut last_type = [0u8; 2];
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        records += 1;
        let Some(record_type) = line.get(..2).and_then(|t| <&[u8; 2]>::try_from(t).ok()) else {
            return Err(reject(format!("MCA record {records} is too short")));
        };
        if !RECORD_TYPES.contains(&record_type) {
            return Err(reject(format!(
                "MCA record {records} has unknown record type {:?}",
                String::from_utf8_lossy(record_type)
            )));
        }
        if records == 1 {
            if record_type != b"HD" {
                return Err(reject("MCA does not start with an HD header record"));
            }
            match line.get(HD_UPDATE_INDICATOR) {
                Some(b'F') => {}
                other => {
                    return Err(reject(format!(
                        "MCA header is not a full extract (update indicator {:?}, expected 'F')",
                        other.map(|b| char::from(*b))
                    )));
                }
            }
        }
        match record_type {
            b"HD" => headers += 1,
            b"ZZ" => trailers += 1,
            b"BS" => schedules += 1,
            b"TI" => tiplocs += 1,
            _ => {}
        }
        last_type = *record_type;
    }
    if records == 0 {
        return Err(reject("MCA is empty"));
    }
    if headers != 1 {
        return Err(reject(format!("MCA has {headers} HD records, expected 1")));
    }
    if trailers != 1 || &last_type != b"ZZ" {
        return Err(reject(
            "MCA does not end with exactly one ZZ trailer record (truncated?)",
        ));
    }

    let (generated, sequence) = read_banner(&msn)?;
    Ok(CifStats {
        generated,
        sequence,
        schedules,
        tiplocs,
    })
}

/// The `/!! Generated:` date and `/!! Sequence:` number from the first
/// lines of an RJTTF banner file.
fn read_banner(path: &Path) -> anyhow::Result<(NaiveDate, Option<u32>)> {
    let reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let (mut generated, mut sequence) = (None, None);
    for line in reader.split(b'\n').take(10) {
        let line = line?;
        let line = String::from_utf8_lossy(&line);
        if let Some(value) = line.strip_prefix("/!! Generated:") {
            generated = NaiveDate::parse_from_str(value.trim(), "%d/%m/%Y").ok();
            if generated.is_none() {
                return Err(reject(format!(
                    "MSN banner Generated date {:?} is not dd/mm/yyyy",
                    value.trim()
                )));
            }
        } else if let Some(value) = line.strip_prefix("/!! Sequence:") {
            sequence = value.trim().parse().ok();
        }
    }
    let generated = generated.ok_or_else(|| reject("MSN has no /!! Generated: banner line"))?;
    Ok((generated, sequence))
}

/// Applies `checks` to a delivery delivered at `delivered_at`, against the
/// last accepted delivery's `previous` stats (none for the first).
pub(crate) fn check(
    stats: &CifStats,
    previous: Option<&CifStats>,
    delivered_at: DateTime<Utc>,
    checks: &CifChecks,
) -> anyhow::Result<()> {
    let delivered = delivered_at.date_naive();
    // A London export date can be a day ahead of the UTC delivery time.
    if stats.generated > delivered + chrono::Days::new(1) {
        return Err(reject(format!(
            "generated {} is after the delivery date {delivered}",
            stats.generated
        )));
    }
    if checks.max_generated_age_days > 0
        && stats.generated + chrono::Days::new(u64::from(checks.max_generated_age_days)) < delivered
    {
        return Err(reject(format!(
            "generated {} is more than {} days before the delivery date {delivered} (a replayed old extract?)",
            stats.generated, checks.max_generated_age_days
        )));
    }
    if checks.min_schedules > 0 && stats.schedules < checks.min_schedules {
        return Err(reject(format!(
            "MCA has {} schedules (BS), fewer than CIF_MIN_SCHEDULES ({})",
            stats.schedules, checks.min_schedules
        )));
    }
    let Some(previous) = previous else {
        return Ok(());
    };
    if stats.generated < previous.generated {
        return Err(reject(format!(
            "generated {} is older than the last accepted delivery's {}",
            stats.generated, previous.generated
        )));
    }
    if checks.max_drop_percent > 0 {
        for (what, now, before) in [
            ("schedules (BS)", stats.schedules, previous.schedules),
            ("TIPLOCs (TI)", stats.tiplocs, previous.tiplocs),
        ] {
            // now < before * (100 - max) / 100, without overflow or floats.
            let floor = u128::from(before) * u128::from(100 - checks.max_drop_percent.min(100));
            if u128::from(now) * 100 < floor {
                return Err(reject(format!(
                    "{what} fell from {before} to {now}, more than CIF_MAX_RECORD_DROP_PERCENT ({}%) since the last accepted delivery",
                    checks.max_drop_percent
                )));
            }
        }
    }
    Ok(())
}

/// Writes `stats` into `dir` (see [`STATS_FILE`]).
pub(crate) fn write_stats(dir: &Path, stats: &CifStats) -> anyhow::Result<()> {
    let path = dir.join(STATS_FILE);
    std::fs::write(&path, serde_json::to_vec(stats)?)?;
    std::fs::File::open(&path)?.sync_all()?;
    Ok(())
}

/// The stats of the newest accepted (marked complete) delivery in
/// `storage_dir` other than `current`: from its [`STATS_FILE`], or, for a
/// delivery accepted before the file existed, by inspecting it (and then
/// saving the file, best effort). `None` when there is none, or it can't
/// be read (logged; the comparison is skipped rather than blocking).
pub(crate) fn previous_stats(storage_dir: &Path, current: &str) -> Option<CifStats> {
    let mut dirs: Vec<String> = std::fs::read_dir(storage_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| {
            name != current
                && crate::delivery::is_delivery_dir_name(name)
                && storage_dir
                    .join(name)
                    .join(common::schedule_delivery::COMPLETE_MARKER)
                    .is_file()
        })
        .collect();
    dirs.sort();
    let name = dirs.pop()?;
    let dir = storage_dir.join(&name);
    if let Ok(bytes) = std::fs::read(dir.join(STATS_FILE))
        && let Ok(stats) = serde_json::from_slice(&bytes)
    {
        return Some(stats);
    }
    match inspect(&dir) {
        Ok(stats) => {
            if let Err(err) = write_stats(&dir, &stats) {
                tracing::warn!(error = %err, dir = %name, "failed to save the previous delivery's CIF stats");
            }
            Some(stats)
        }
        Err(err) => {
            tracing::warn!(error = %err, dir = %name, "cannot read the previous delivery's CIF stats; skipping the comparison with it");
            None
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::format_collect,
    clippy::needless_pass_by_value,
    reason = "test code: test string building is not hot; helpers take owned fixtures"
)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const MCA: &str =
        include_str!("../tests/fixtures/cif_delivery_excerpt/RJTTF975MCA.txt");
    pub(crate) const MSN: &str =
        include_str!("../tests/fixtures/cif_delivery_excerpt/RJTTF975MSN.txt");
    pub(crate) const DAT: &str =
        include_str!("../tests/fixtures/cif_delivery_excerpt/RJTTF975DAT.txt");

    /// The real-shaped fixture's files, with the banner's Generated date
    /// replaced by `generated` (dd/mm/yyyy) when given.
    pub(crate) fn delivery_files(generated: Option<&str>) -> Vec<(&'static str, String)> {
        let date = |text: &str| match generated {
            Some(date) => text.replace("30/09/2026", date),
            None => text.to_string(),
        };
        vec![
            ("RJTTF975MCA.txt", MCA.to_string()),
            ("RJTTF975MSN.txt", date(MSN)),
            ("RJTTF975DAT.txt", date(DAT)),
        ]
    }

    /// A real-shaped delivery zip generated on `generated` (dd/mm/yyyy).
    pub(crate) fn delivery_zip(generated: &str) -> Vec<u8> {
        let files = delivery_files(Some(generated));
        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, text)| (*name, text.as_bytes()))
            .collect();
        crate::delivery::build_test_zip(&entries)
    }

    fn write(dir: &Path, files: &[(&str, String)]) {
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
    }

    fn at(date: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("{date}T19:59:59Z"))
            .unwrap()
            .with_timezone(&Utc)
    }

    const CHECKS: CifChecks = CifChecks {
        max_generated_age_days: 3,
        min_schedules: 1,
        max_drop_percent: 20,
    };

    fn stats(generated: &str, schedules: u64, tiplocs: u64) -> CifStats {
        CifStats {
            generated: NaiveDate::parse_from_str(generated, "%Y-%m-%d").unwrap(),
            sequence: None,
            schedules,
            tiplocs,
        }
    }

    #[test]
    fn the_real_shaped_delivery_passes_and_is_counted() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &delivery_files(None));
        let got = inspect(dir.path()).unwrap();
        assert_eq!(
            got,
            CifStats {
                generated: NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
                sequence: Some(975),
                schedules: 2,
                tiplocs: 4,
            }
        );
        check(&got, None, at("2026-09-30"), &CHECKS).unwrap();
    }

    /// The real counts (2026-09-29 to 09-30) pass the default thresholds.
    #[test]
    fn real_day_to_day_changes_pass_the_defaults() {
        let defaults = CifChecks {
            max_generated_age_days: 3,
            min_schedules: 100_000,
            max_drop_percent: 20,
        };
        check(
            &stats("2026-09-30", 505_163, 12_096),
            Some(&stats("2026-09-29", 505_342, 12_096)),
            at("2026-09-30"),
            &defaults,
        )
        .unwrap();
    }

    fn rejected_reason(files: Vec<(&'static str, String)>) -> String {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &files);
        let err = inspect(dir.path()).unwrap_err();
        assert!(crate::delivery::is_rejected(&err), "{err:?}");
        err.to_string()
    }

    fn with_mca(mca: String) -> Vec<(&'static str, String)> {
        let mut files = delivery_files(None);
        files[0].1 = mca;
        files
    }

    #[test]
    fn a_truncated_mca_is_rejected() {
        let truncated: String = MCA.lines().take(8).map(|l| format!("{l}\n")).collect();
        assert!(rejected_reason(with_mca(truncated)).contains("ZZ"));
    }

    #[test]
    fn an_update_extract_is_rejected() {
        let mut update = MCA.to_string();
        update.replace_range(46..47, "U");
        assert!(rejected_reason(with_mca(update)).contains("not a full extract"));
    }

    #[test]
    fn a_file_that_is_not_cif_is_rejected() {
        assert!(
            rejected_reason(with_mca("hello world\r\n".to_string()))
                .contains("unknown record type")
        );
        let injected = MCA.replace("TIABER ", "XXABER ");
        assert!(rejected_reason(with_mca(injected)).contains("unknown record type \"XX\""));
        let headerless: String = MCA.lines().skip(1).map(|l| format!("{l}\n")).collect();
        assert!(rejected_reason(with_mca(headerless)).contains("HD"));
        assert!(rejected_reason(with_mca(String::new())).contains("empty"));
    }

    #[test]
    fn missing_or_duplicate_files_are_rejected() {
        let mut no_msn = delivery_files(None);
        no_msn.remove(1);
        assert!(rejected_reason(no_msn).contains("no RJTTF*MSN.txt"));
        let mut two_mca = delivery_files(None);
        two_mca.push(("RJTTF976MCA.txt", MCA.to_string()));
        assert!(rejected_reason(two_mca).contains("2 RJTTF*MCA.txt"));
    }

    #[test]
    fn an_msn_without_a_generated_date_is_rejected() {
        let mut files = delivery_files(None);
        files[1].1 = MSN.replace("/!! Generated:", "/!! Created:");
        assert!(rejected_reason(files).contains("Generated"));
        let mut files = delivery_files(None);
        files[1].1 = MSN.replace("30/09/2026", "2026-09-30");
        assert!(rejected_reason(files).contains("dd/mm/yyyy"));
    }

    #[test]
    fn a_stale_or_future_extract_is_rejected() {
        let s = stats("2026-09-25", 10, 10);
        let err = check(&s, None, at("2026-09-30"), &CHECKS).unwrap_err();
        assert!(err.to_string().contains("replayed"), "{err}");
        check(&s, None, at("2026-09-28"), &CHECKS).unwrap();
        let err = check(
            &stats("2026-10-02", 10, 10),
            None,
            at("2026-09-30"),
            &CHECKS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("after the delivery date"), "{err}");
        // A London export date one day ahead of the UTC delivery is fine.
        check(
            &stats("2026-10-01", 10, 10),
            None,
            at("2026-09-30"),
            &CHECKS,
        )
        .unwrap();
        // 0 disables the age check.
        let lax = CifChecks {
            max_generated_age_days: 0,
            ..CHECKS
        };
        check(&s, None, at("2026-09-30"), &lax).unwrap();
    }

    #[test]
    fn going_backwards_or_shrinking_past_the_threshold_is_rejected() {
        let previous = stats("2026-09-29", 1000, 100);
        let older = check(
            &stats("2026-09-28", 1000, 100),
            Some(&previous),
            at("2026-09-30"),
            &CHECKS,
        );
        assert!(
            older
                .unwrap_err()
                .to_string()
                .contains("older than the last accepted")
        );
        // 20% drop allowed, 21% not.
        check(
            &stats("2026-09-30", 800, 100),
            Some(&previous),
            at("2026-09-30"),
            &CHECKS,
        )
        .unwrap();
        let err = check(
            &stats("2026-09-30", 790, 100),
            Some(&previous),
            at("2026-09-30"),
            &CHECKS,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("schedules (BS) fell from 1000 to 790"),
            "{err}"
        );
        let err = check(
            &stats("2026-09-30", 1000, 70),
            Some(&previous),
            at("2026-09-30"),
            &CHECKS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("TIPLOCs"), "{err}");
        let off = CifChecks {
            max_drop_percent: 0,
            ..CHECKS
        };
        check(
            &stats("2026-09-30", 1, 1),
            Some(&previous),
            at("2026-09-30"),
            &off,
        )
        .unwrap();
    }

    #[test]
    fn too_few_schedules_is_rejected() {
        let strict = CifChecks {
            min_schedules: 100_000,
            ..CHECKS
        };
        let err = check(&stats("2026-09-30", 2, 4), None, at("2026-09-30"), &strict).unwrap_err();
        assert!(err.to_string().contains("CIF_MIN_SCHEDULES"), "{err}");
    }

    #[test]
    fn previous_stats_come_from_the_newest_other_complete_delivery() {
        let storage = tempfile::tempdir().unwrap();
        let marker = common::schedule_delivery::COMPLETE_MARKER;
        for (name, complete) in [
            ("20260928T200446Z", true),
            ("20260929T195901Z", true),
            ("20260930T195959Z", true),
            ("20261001T200000Z", false),
        ] {
            let dir = storage.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            write(&dir, &delivery_files(None));
            if complete {
                std::fs::write(dir.join(marker), "").unwrap();
            }
        }
        // 20260930 is "current"; 20261001 is unmarked; so 20260929 wins.
        let saved = stats("2026-09-29", 7, 7);
        write_stats(&storage.path().join("20260929T195901Z"), &saved).unwrap();
        assert_eq!(
            previous_stats(storage.path(), "20260930T195959Z"),
            Some(saved)
        );
        // Without a stats file it is inspected, and the file saved.
        let legacy = storage.path().join("20260930T195959Z");
        assert_eq!(
            previous_stats(storage.path(), "20261001T200000Z").map(|s| s.schedules),
            Some(2)
        );
        assert!(legacy.join(STATS_FILE).is_file());
        assert_eq!(
            previous_stats(storage.path(), "20260928T200446Z").map(|s| s.schedules),
            Some(2)
        );
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(previous_stats(empty.path(), "x"), None);
    }
}
