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

/// Database-gated: each test gets its own throwaway database
/// (`#[sqlx::test]`), because a CORPUS load replaces whole tables. Needs a
/// `DATABASE_URL` whose role may create databases.
#[cfg(test)]
#[expect(
    clippy::items_after_statements,
    reason = "test code: fixtures sit next to their use"
)]
mod db_tests {
    use chrono::{DateTime, TimeZone, Utc};
    use common::corpus_inference;
    use sqlx::PgPool;

    use super::*;
    use crate::data::corpus::{CorpusLocation, replace_corpus_locations};
    use crate::data::queries;

    fn loc(nlc: &str, stanox: &str, tiploc: &str, crs: &str, desc: &str) -> CorpusLocation {
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        CorpusLocation {
            nlc: nlc.to_owned(),
            stanox: opt(stanox),
            tiploc: opt(tiploc),
            crs: opt(crs),
            uic: None,
            nlc_desc: opt(desc),
            nlc_desc16: None,
        }
    }

    /// A small hand-made CORPUS: Clapham Junction with a platform TIPLOC
    /// and a loop, a TIPLOC the timetable maps to another CRS, a station
    /// the timetable lacks, a station at a STANOX the timetable left out
    /// of `stanox_crs` on purpose, and two CORPUS codes that are not
    /// stations (a pseudo `X` code and an Underground-style one).
    fn corpus() -> Vec<CorpusLocation> {
        vec![
            loc(
                "559500",
                "87219",
                "CLPHMJN",
                "CLJ",
                "CLAPHAM JUNCTION LONDON",
            ),
            loc("559572", "87219", "CLPHMJW", "", "CLAPHAM JN (WINDSOR)"),
            loc("559595", "87219", "CLPHJLP", "", "CLAPHAM JUNCTION LOOP"),
            loc("111100", "11111", "CONFLCT", "AAA", "CONFLICT A"),
            loc("222200", "22222", "NEWSTN", "NEW", "NEW STATION"),
            loc("333300", "33333", "SHARED1", "SHA", "SHARED ONE"),
            loc("000800", "", "", "", "MERSEYRAIL ELECTRICS-HQ INPUT"),
            loc("999900", "99999", "XBUSSTP", "XBS", "SOMEWHERE BUS STATION"),
            loc("888800", "88888", "LULSTOP", "LUA", "SOMEWHERE LUL"),
        ]
    }

    /// The Knowledgebase stations: every CORPUS CRS above except the two
    /// non-stations (`XBS`, `LUA`).
    const STATIONS: [&str; 4] = ["CLJ", "AAA", "NEW", "SHA"];

    async fn seed_stations(pool: &PgPool, codes: &[&str]) {
        for crs in codes {
            sqlx::query("INSERT INTO stations (crs, name) VALUES ($1, $1)")
                .bind(crs)
                .execute(pool)
                .await
                .unwrap();
        }
    }

    async fn stored_tiplocs(pool: &PgPool) -> Vec<String> {
        sqlx::query_as::<_, (String,)>("SELECT tiploc FROM corpus_tiploc_crs ORDER BY tiploc")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .map(|(t,)| t)
            .collect()
    }

    async fn seed_timetable(pool: &PgPool) {
        for (tiploc, crs, stanox) in [
            ("CLPHMJN", "CLJ", "87219"),
            ("CONFLCT", "BBB", "11111"),
            ("TTSHARE", "TTS", "33333"),
            ("TTONLY", "TTO", "44444"),
        ] {
            sqlx::query(
                "INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence) \
                 VALUES ($1, $2, $1, $3, 1)",
            )
            .bind(tiploc)
            .bind(crs)
            .bind(stanox)
            .execute(pool)
            .await
            .unwrap();
        }
        for (stanox, crs, tiploc) in [
            ("87219", "CLJ", "CLPHMJN"),
            ("11111", "BBB", "CONFLCT"),
            ("44444", "TTO", "TTONLY"),
        ] {
            sqlx::query(
                "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
                 VALUES ($1, $2, $3, $3, 1)",
            )
            .bind(stanox)
            .bind(crs)
            .bind(tiploc)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 1, 3, 0, 0).unwrap()
    }

    /// Everything the five lookups return, as JSON, for byte-for-byte
    /// comparison.
    async fn snapshot(pool: &PgPool, fallback: bool) -> serde_json::Value {
        let tiplocs: Vec<String> = [
            "CLPHMJN", "CLPHMJW", "CLPHJLP", "CONFLCT", "NEWSTN", "TTONLY", "NOPE", "XBUSSTP",
            "LULSTOP",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut single = Vec::new();
        for t in &tiplocs {
            single.push(
                queries::crs_for_tiploc_with(pool, t, fallback)
                    .await
                    .unwrap(),
            );
        }
        let batch: std::collections::BTreeMap<String, String> =
            queries::crs_for_tiplocs_batch_with(pool, &tiplocs, fallback)
                .await
                .unwrap()
                .into_iter()
                .collect();
        let mut for_crs = Vec::new();
        for crs in ["CLJ", "AAA", "BBB", "NEW", "SHA"] {
            for_crs.push(
                queries::list_stanox_crs_for_crs_with(pool, crs, fallback)
                    .await
                    .unwrap(),
            );
        }
        serde_json::json!({
            "single": single,
            "batch": batch,
            "stanox_crs": queries::list_stanox_crs_with(pool, fallback).await.unwrap(),
            "tiploc_crs": queries::list_tiploc_crs_with(pool, fallback).await.unwrap(),
            "for_crs": for_crs,
        })
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn a_load_stores_the_crosswalk_and_marks_the_build(pool: PgPool) {
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);
        seed_stations(&pool, &STATIONS).await;
        replace_corpus_locations(&pool, at(), "CORPUSExtract.json.gz", &corpus())
            .await
            .unwrap();
        let tiplocs: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
            "SELECT tiploc, crs, stanox, rule FROM corpus_tiploc_crs ORDER BY tiploc",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let s = |v: &str| v.to_owned();
        assert_eq!(
            tiplocs,
            vec![
                (s("CLPHMJN"), s("CLJ"), Some(s("87219")), s("direct")),
                (s("CLPHMJW"), s("CLJ"), Some(s("87219")), s("station_name")),
                (s("CONFLCT"), s("AAA"), Some(s("11111")), s("direct")),
                (s("NEWSTN"), s("NEW"), Some(s("22222")), s("direct")),
                (s("SHARED1"), s("SHA"), Some(s("33333")), s("direct")),
            ]
        );
        type Build = (
            DateTime<Utc>,
            i32,
            i32,
            Option<i32>,
            Option<i32>,
            Option<String>,
        );
        let (delivered_at, version, stanoxes, tiplocs_out, stanoxes_out, fingerprint): Build =
            sqlx::query_as(
                "SELECT delivered_at, rules_version, stanox_rows, tiploc_rows_excluded, \
                        stanox_rows_excluded, stations_fingerprint FROM corpus_crosswalk_build",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            (delivered_at, version, stanoxes, tiplocs_out, stanoxes_out),
            (at(), corpus_inference::RULES_VERSION, 4, Some(2), Some(2))
        );
        assert_eq!(
            fingerprint,
            Some(load_station_set(&pool).await.unwrap().fingerprint)
        );
        // Up to date: nothing to rebuild.
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn an_outdated_build_is_rebuilt_from_corpus_locations(pool: PgPool) {
        seed_stations(&pool, &STATIONS).await;
        replace_corpus_locations(&pool, at(), "CORPUSExtract.json.gz", &corpus())
            .await
            .unwrap();
        let expected = ["CLPHMJN", "CLPHMJW", "CONFLCT", "NEWSTN", "SHARED1"];
        // As built by RULES_VERSION 1 (before the stations filter): every
        // unambiguous key, non-stations included, and no fingerprint.
        sqlx::query(
            "INSERT INTO corpus_tiploc_crs (tiploc, crs, stanox, station_name, rule) VALUES \
             ('XBUSSTP', 'XBS', '99999', 'SOMEWHERE BUS STATION', 'direct'), \
             ('LULSTOP', 'LUA', '88888', 'SOMEWHERE LUL', 'direct')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE corpus_crosswalk_build SET rules_version = 1, stations_fingerprint = NULL",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(stored_tiplocs(&pool).await.len(), 7);
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), Some(at()));
        assert_eq!(stored_tiplocs(&pool).await, expected);
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);

        // An older rules version alone also rebuilds.
        sqlx::query("DELETE FROM corpus_tiploc_crs")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE corpus_crosswalk_build SET rules_version = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), Some(at()));
        assert_eq!(stored_tiplocs(&pool).await, expected);
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);
    }

    /// Flag on: a TIPLOC or STANOX whose CORPUS CRS is not a station
    /// (pseudo `XBS`, non-station `LUA`) is never filled; a platform TIPLOC
    /// of a real station (`CLPHMJW`, Clapham Junction's Windsor side) is.
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn the_stations_filter_drops_non_station_fills_and_keeps_platform_fills(pool: PgPool) {
        seed_timetable(&pool).await;
        seed_stations(&pool, &STATIONS).await;
        replace_corpus_locations(&pool, at(), "CORPUSExtract.json.gz", &corpus())
            .await
            .unwrap();
        let crs = |t: &'static str| {
            let pool = pool.clone();
            async move { queries::crs_for_tiploc_with(&pool, t, true).await.unwrap() }
        };
        assert_eq!(crs("CLPHMJW").await.as_deref(), Some("CLJ"));
        assert_eq!(crs("XBUSSTP").await, None);
        assert_eq!(crs("LULSTOP").await, None);
        let batch = queries::crs_for_tiplocs_batch_with(
            &pool,
            &["CLPHMJW".into(), "XBUSSTP".into(), "LULSTOP".into()],
            true,
        )
        .await
        .unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch["CLPHMJW"], "CLJ");
        let stanoxes: Vec<String> = queries::list_stanox_crs_with(&pool, true)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.stanox)
            .collect();
        assert!(!stanoxes.contains(&"99999".to_owned()));
        assert!(!stanoxes.contains(&"88888".to_owned()));
        assert!(
            queries::list_stanox_crs_for_crs_with(&pool, "XBS", true)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// A station added to (or removed from) `stations` after the CORPUS load
    /// changes the fingerprint, so the next check rebuilds and the fallback
    /// picks it up.
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn a_stations_change_rebuilds_the_crosswalk(pool: PgPool) {
        seed_stations(&pool, &STATIONS).await;
        replace_corpus_locations(&pool, at(), "CORPUSExtract.json.gz", &corpus())
            .await
            .unwrap();
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);
        assert_eq!(
            queries::crs_for_tiploc_with(&pool, "LULSTOP", true)
                .await
                .unwrap(),
            None
        );

        // The Knowledgebase publishes LUA as a station.
        seed_stations(&pool, &["LUA"]).await;
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), Some(at()));
        assert_eq!(
            queries::crs_for_tiploc_with(&pool, "LULSTOP", true)
                .await
                .unwrap()
                .as_deref(),
            Some("LUA")
        );
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);

        // An unrelated update (a renamed station) is not a change.
        sqlx::query("UPDATE stations SET name = 'Renamed' WHERE crs = 'LUA'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);

        // And it is withdrawn again.
        sqlx::query("DELETE FROM stations WHERE crs = 'LUA'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), Some(at()));
        assert!(!stored_tiplocs(&pool).await.contains(&"LULSTOP".to_owned()));
    }

    /// Flag off: every lookup returns exactly what it returned before
    /// CORPUS was loaded, and exactly what the process-wide default (off)
    /// returns.
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn with_the_flag_off_corpus_changes_nothing(pool: PgPool) {
        seed_timetable(&pool).await;
        seed_stations(&pool, &STATIONS).await;
        let before = snapshot(&pool, false).await;
        replace_corpus_locations(&pool, at(), "CORPUSExtract.json.gz", &corpus())
            .await
            .unwrap();
        assert_eq!(snapshot(&pool, false).await, before);
        assert!(!fallback_enabled());
        assert_eq!(
            queries::crs_for_tiploc(&pool, "CLPHMJW").await.unwrap(),
            None
        );
        assert_eq!(
            serde_json::to_value(queries::list_stanox_crs(&pool).await.unwrap()).unwrap(),
            before["stanox_crs"]
        );
    }

    /// Flag on: CORPUS fills what the timetable lacks, and the timetable
    /// wins every conflict.
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn with_the_flag_on_corpus_fills_gaps_and_the_timetable_wins(pool: PgPool) {
        seed_timetable(&pool).await;
        seed_stations(&pool, &STATIONS).await;
        replace_corpus_locations(&pool, at(), "CORPUSExtract.json.gz", &corpus())
            .await
            .unwrap();
        let crs = |t: &'static str| {
            let pool = pool.clone();
            async move { queries::crs_for_tiploc_with(&pool, t, true).await.unwrap() }
        };
        // Fills.
        assert_eq!(crs("CLPHMJW").await.as_deref(), Some("CLJ"));
        assert_eq!(crs("NEWSTN").await.as_deref(), Some("NEW"));
        // Timetable wins; a loop stays unresolved; timetable-only unchanged.
        assert_eq!(crs("CONFLCT").await.as_deref(), Some("BBB"));
        assert_eq!(crs("CLPHJLP").await, None);
        assert_eq!(crs("TTONLY").await.as_deref(), Some("TTO"));

        let batch = queries::crs_for_tiplocs_batch_with(
            &pool,
            &["CLPHMJW".into(), "CONFLCT".into(), "TTONLY".into()],
            true,
        )
        .await
        .unwrap();
        assert_eq!(batch["CLPHMJW"], "CLJ");
        assert_eq!(batch["CONFLCT"], "BBB");
        assert_eq!(batch["TTONLY"], "TTO");

        let stanoxes: Vec<(String, String)> = queries::list_stanox_crs_with(&pool, true)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.stanox, r.crs))
            .collect();
        let s = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        // 22222 filled; 33333 (in tiploc_crs, left out of stanox_crs) not;
        // 11111 keeps the timetable's BBB.
        assert_eq!(
            stanoxes,
            vec![
                s("11111", "BBB"),
                s("22222", "NEW"),
                s("44444", "TTO"),
                s("87219", "CLJ")
            ]
        );

        let for_crs = |c: &'static str| {
            let pool = pool.clone();
            async move {
                queries::list_stanox_crs_for_crs_with(&pool, c, true)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|r| r.tiploc)
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(for_crs("CLJ").await, ["CLPHMJN", "CLPHMJW"]);
        // CONFLCT is the timetable's BBB, so CORPUS's AAA lists nothing.
        assert!(for_crs("AAA").await.is_empty());
        assert_eq!(for_crs("BBB").await, ["CONFLCT"]);

        let tiplocs: Vec<(String, String, i32)> = queries::list_tiploc_crs_with(&pool, true)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.tiploc, r.crs, r.source_sequence))
            .collect();
        assert_eq!(
            tiplocs,
            vec![
                ("CLPHMJN".to_owned(), "CLJ".to_owned(), 1),
                ("CLPHMJW".to_owned(), "CLJ".to_owned(), 0),
                ("CONFLCT".to_owned(), "BBB".to_owned(), 1),
                ("NEWSTN".to_owned(), "NEW".to_owned(), 0),
                ("SHARED1".to_owned(), "SHA".to_owned(), 0),
                ("TTONLY".to_owned(), "TTO".to_owned(), 1),
                ("TTSHARE".to_owned(), "TTS".to_owned(), 1),
            ]
        );
    }
}
