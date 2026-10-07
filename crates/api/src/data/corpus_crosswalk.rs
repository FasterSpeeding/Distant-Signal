//! The CORPUS-derived crosswalk (`corpus_tiploc_crs`, `corpus_stanox_crs`,
//! migration `20260928170000_corpus_crosswalk.sql`) and the OFF-BY-DEFAULT
//! runtime fallback that reads it.
//!
//! **Derivation.** `common::corpus_inference` (the same conservative rule
//! `line-catalogue-validator` regenerates `reference-data/crs-tiploc.csv`
//! with) runs over the whole current `corpus_locations`; [`write`] stores its
//! unambiguous keys whose CRS is a Knowledgebase station (`stations`; see
//! [`corpus_inference::restrict_to_stations`]). That happens in the same
//! transaction as every CORPUS load
//! ([`crate::data::corpus::replace_corpus_locations`]), and
//! ([`rebuild_if_stale`]) at startup and after every `stations` refresh
//! when the stored build is older than the newest delivery, than this
//! build's `RULES_VERSION`, or than the current set of `stations` CRS codes
//! (tracked by a fingerprint in `corpus_crosswalk_build`). With no CORPUS
//! loaded, the check is one `MAX()` over the empty `corpus_deliveries`.
//!
//! **Why filter at build time, and rebuild on a `stations` change**, rather
//! than join `stations` in every lookup: the lookups stay exactly the SQL
//! they were (no extra join in the per-stop hot paths or in the whole-table
//! `list_*` reads), the comparison report and the stored rows apply the one
//! same Rust filter, and `stations` changes rarely (a new Knowledgebase
//! station a few times a year), so a rebuild per changed station set costs
//! next to nothing. The fingerprint makes this self-healing: a crash
//! between a `stations` refresh and its rebuild is caught at the next
//! refresh or startup.
//!
//! **Fallback** (`CORPUS_FALLBACK_ENABLED=true`, chart
//! `api.corpusFallback.enabled`, default false). When on, the TIPLOC/STANOX
//! lookups in [`crate::data::queries`] (`crs_for_tiploc`,
//! `crs_for_tiplocs_batch`, `list_stanox_crs`, `list_stanox_crs_for_crs`,
//! `list_tiploc_crs`) add the CORPUS rows AFTER the timetable-derived
//! `tiploc_crs`/`stanox_crs`:
//!
//! - the timetable always wins: a CORPUS row is used only for a TIPLOC
//!   neither timetable table has, or a STANOX the timetable does not know
//!   at all (a STANOX that appears in `tiploc_crs` but not `stanox_crs` was
//!   deliberately left out by `schedule-reference` as shared by two
//!   stations, and stays out);
//! - when off, every lookup runs exactly the SQL it ran before this module
//!   existed.
//!
//! The flag is process-wide ([`init_fallback_from_env`], read once at
//! startup) because the lookups take only a pool and have dozens of
//! callers; each lookup also has a `*_with` variant taking the flag
//! explicitly, which is what the tests use.

// Moved to `ds_store::corpus` (ingest architecture plan 1A, wave 0): the
// train lookups (`stop_delay`) read the flag too.
pub use ds_store::corpus::{
    FALLBACK_ENV, fallback_enabled, init_fallback_from_env, parse_fallback_flag,
};

// Moved to `ds_store::corpus::crosswalk` (ingest architecture plan 1A.6).
pub use ds_store::corpus::crosswalk::{
    BuildCounts, StationSet, corpus_rows, derive, load_corpus_locations, load_station_set,
    rebuild_if_stale, write,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The chart sets the flag on the api container, off by default.
    #[test]
    fn the_chart_wires_the_flag_off_by_default() {
        let chart = common::manifest_dir!().join("../../charts/distant-signal");
        let template =
            std::fs::read_to_string(chart.join("templates/api-deployment.yaml")).unwrap();
        assert!(template.contains(&format!(
            "- name: {FALLBACK_ENV}\n              value: {{{{ .Values.api.corpusFallback.enabled | toString | quote }}}}"
        )));
        let values = std::fs::read_to_string(chart.join("values.yaml")).unwrap();
        assert!(values.contains("  corpusFallback:\n    enabled: false\n"));
    }
}
