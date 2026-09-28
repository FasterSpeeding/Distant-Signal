//! The CORPUS-derived crosswalk (`corpus_tiploc_crs`, `corpus_stanox_crs`,
//! migration `20260928110000_corpus_crosswalk.sql`) and the OFF-BY-DEFAULT
//! runtime fallback that reads it.
//!
//! **Derivation.** `common::corpus_inference` (the same conservative rule
//! `line-catalogue-validator` regenerates `reference-data/crs-tiploc.csv`
//! with) runs over the whole current `corpus_locations`; [`write`] stores its
//! unambiguous keys. That happens in the same transaction as every CORPUS
//! load ([`crate::data::corpus::replace_corpus_locations`]), and at startup
//! when the stored build is older than the newest delivery or than this
//! build's `RULES_VERSION` ([`rebuild_if_stale`]). With no CORPUS loaded,
//! the startup check is one `MAX()` over the empty `corpus_deliveries`.
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

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use common::corpus_inference::{self, CorpusCrsTiploc, CorpusRow, Crosswalk};
use sqlx::{PgPool, Postgres, Transaction};

use crate::data::corpus::CorpusLocation;

/// Env var turning the fallback on. Unset or `false`: off.
pub const FALLBACK_ENV: &str = "CORPUS_FALLBACK_ENABLED";

static FALLBACK_ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether the lookups fall back to CORPUS. Always false unless
/// [`init_fallback_from_env`] read `true`.
pub fn fallback_enabled() -> bool {
    FALLBACK_ENABLED.load(Ordering::Relaxed)
}

/// Parses a [`FALLBACK_ENV`] value: unset or blank is off, anything other
/// than `true`/`false` is a startup error rather than a silent default.
pub fn parse_fallback_flag(value: Option<&str>) -> Result<bool> {
    match value.map(str::trim) {
        None | Some("") => Ok(false),
        Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
        Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
        Some(v) => anyhow::bail!("{FALLBACK_ENV} must be true or false, got {v:?}"),
    }
}

/// Reads [`FALLBACK_ENV`] once at startup and logs the outcome.
pub fn init_fallback_from_env() -> Result<()> {
    let enabled = parse_fallback_flag(std::env::var(FALLBACK_ENV).ok().as_deref())?;
    FALLBACK_ENABLED.store(enabled, Ordering::Relaxed);
    if enabled {
        tracing::info!(
            "CORPUS fallback ON: TIPLOC/STANOX lookups fall back to corpus_tiploc_crs/corpus_stanox_crs after the timetable crosswalk"
        );
    }
    Ok(())
}

/// `corpus_locations` rows as the inference's input.
pub fn corpus_rows(locations: &[CorpusLocation]) -> Vec<CorpusRow> {
    locations
        .iter()
        .map(|l| CorpusRow {
            nlc: Some(l.nlc.clone()),
            stanox: l.stanox.clone(),
            tiploc: l.tiploc.clone(),
            crs: l.crs.clone(),
            nlc_desc: l.nlc_desc.clone(),
        })
        .collect()
}

/// Runs the shared inference over `rows` and narrows it to the crosswalk.
pub fn derive(rows: &[CorpusRow]) -> (CorpusCrsTiploc, Crosswalk) {
    let inferred = corpus_inference::infer_crs_tiploc(rows);
    let crosswalk = corpus_inference::crosswalk(rows, &inferred);
    (inferred, crosswalk)
}

/// Every `corpus_locations` row, for a rebuild or a comparison.
pub async fn load_corpus_locations<'e, E>(executor: E) -> Result<Vec<CorpusLocation>>
where
    E: sqlx::PgExecutor<'e>,
{
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT nlc, stanox, tiploc, crs, uic, nlc_desc, nlc_desc16 FROM corpus_locations ORDER BY id",
    )
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(nlc, stanox, tiploc, crs, uic, nlc_desc, nlc_desc16)| CorpusLocation {
                nlc,
                stanox,
                tiploc,
                crs,
                uic,
                nlc_desc,
                nlc_desc16,
            },
        )
        .collect())
}

/// Replaces the stored crosswalk with `crosswalk`, derived from the delivery
/// `delivered_at`, inside the caller's transaction (which must hold the
/// CORPUS load lock).
pub async fn write(
    tx: &mut Transaction<'_, Postgres>,
    delivered_at: DateTime<Utc>,
    crosswalk: &Crosswalk,
) -> Result<()> {
    sqlx::query("DELETE FROM corpus_tiploc_crs")
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM corpus_stanox_crs")
        .execute(&mut **tx)
        .await?;
    let t = &crosswalk.tiplocs;
    let tiploc: Vec<&str> = t.iter().map(|r| r.tiploc.as_str()).collect();
    let crs: Vec<&str> = t.iter().map(|r| r.crs.as_str()).collect();
    let stanox: Vec<Option<&str>> = t.iter().map(|r| r.stanox.as_deref()).collect();
    let name: Vec<&str> = t.iter().map(|r| r.station_name.as_str()).collect();
    let rule: Vec<&str> = t.iter().map(|r| r.rule.as_str()).collect();
    sqlx::query(
        "INSERT INTO corpus_tiploc_crs (tiploc, crs, stanox, station_name, rule) \
         SELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[])",
    )
    .bind(&tiploc)
    .bind(&crs)
    .bind(&stanox)
    .bind(&name)
    .bind(&rule)
    .execute(&mut **tx)
    .await?;
    let s = &crosswalk.stanoxes;
    let stanox: Vec<&str> = s.iter().map(|r| r.stanox.as_str()).collect();
    let crs: Vec<&str> = s.iter().map(|r| r.crs.as_str()).collect();
    let tiploc: Vec<&str> = s.iter().map(|r| r.tiploc.as_str()).collect();
    let name: Vec<&str> = s.iter().map(|r| r.station_name.as_str()).collect();
    sqlx::query(
        "INSERT INTO corpus_stanox_crs (stanox, crs, tiploc, station_name) \
         SELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[])",
    )
    .bind(&stanox)
    .bind(&crs)
    .bind(&tiploc)
    .bind(&name)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO corpus_crosswalk_build \
             (singleton, delivered_at, rules_version, tiploc_rows, stanox_rows, built_at) \
         VALUES (TRUE, $1, $2, $3, $4, now()) \
         ON CONFLICT (singleton) DO UPDATE SET \
             delivered_at = EXCLUDED.delivered_at, \
             rules_version = EXCLUDED.rules_version, \
             tiploc_rows = EXCLUDED.tiploc_rows, \
             stanox_rows = EXCLUDED.stanox_rows, \
             built_at = EXCLUDED.built_at",
    )
    .bind(delivered_at)
    .bind(corpus_inference::RULES_VERSION)
    .bind(i32::try_from(t.len())?)
    .bind(i32::try_from(s.len())?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Rebuilds the stored crosswalk from `corpus_locations` when it was derived
/// from an older delivery or by older rules (or never). Returns the delivery
/// it rebuilt from, or `None` when there was nothing to do -- always the
/// case before the first CORPUS load, which costs one `MAX()` over an empty
/// table.
pub async fn rebuild_if_stale(pool: &PgPool) -> Result<Option<DateTime<Utc>>> {
    let is_stale = |latest: Option<DateTime<Utc>>, built: Option<(DateTime<Utc>, i32)>| {
        latest.filter(|latest| built != Some((*latest, corpus_inference::RULES_VERSION)))
    };
    let (latest,): (Option<DateTime<Utc>>,) =
        sqlx::query_as("SELECT MAX(delivered_at) FROM corpus_deliveries")
            .fetch_one(pool)
            .await?;
    if latest.is_none() {
        return Ok(None);
    }
    let built = stored_build(pool).await?;
    if is_stale(latest, built).is_none() {
        return Ok(None);
    }

    // Stale: redo the check under the load lock, so a load committing
    // meanwhile (which writes its own crosswalk) is not overwritten with
    // rows derived from the older set.
    let mut tx = pool.begin().await?;
    crate::data::corpus::take_load_lock(&mut tx).await?;
    let (latest,): (Option<DateTime<Utc>>,) =
        sqlx::query_as("SELECT MAX(delivered_at) FROM corpus_deliveries")
            .fetch_one(&mut *tx)
            .await?;
    let built = stored_build(&mut *tx).await?;
    let Some(latest) = is_stale(latest, built) else {
        return Ok(None);
    };
    let locations = load_corpus_locations(&mut *tx).await?;
    let (_, crosswalk) = derive(&corpus_rows(&locations));
    write(&mut tx, latest, &crosswalk).await?;
    tx.commit().await?;
    tracing::info!(
        delivered_at = %latest,
        rules_version = corpus_inference::RULES_VERSION,
        tiplocs = crosswalk.tiplocs.len(),
        stanoxes = crosswalk.stanoxes.len(),
        "rebuilt the CORPUS crosswalk"
    );
    Ok(Some(latest))
}

async fn stored_build<'e, E>(executor: E) -> Result<Option<(DateTime<Utc>, i32)>>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query_as("SELECT delivered_at, rules_version FROM corpus_crosswalk_build")
        .fetch_optional(executor)
        .await
        .context("reading corpus_crosswalk_build")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_strict_and_off_by_default() {
        assert!(!parse_fallback_flag(None).unwrap());
        assert!(!parse_fallback_flag(Some(" ")).unwrap());
        assert!(!parse_fallback_flag(Some("false")).unwrap());
        assert!(parse_fallback_flag(Some("true")).unwrap());
        assert!(parse_fallback_flag(Some("TRUE")).unwrap());
        assert!(parse_fallback_flag(Some("1")).is_err());
        assert!(parse_fallback_flag(Some("yes")).is_err());
        assert!(!fallback_enabled());
    }

    /// The chart sets the flag on the api container, off by default.
    #[test]
    fn the_chart_wires_the_flag_off_by_default() {
        let chart = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../charts/distant-signal");
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
mod db_tests {
    use chrono::TimeZone;

    use super::*;
    use crate::data::corpus::replace_corpus_locations;
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
    /// the timetable lacks, and a station at a STANOX the timetable left out
    /// of `stanox_crs` on purpose.
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
        ]
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
            "CLPHMJN", "CLPHMJW", "CLPHJLP", "CONFLCT", "NEWSTN", "TTONLY", "NOPE",
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
        let (delivered_at, version, stanoxes): (DateTime<Utc>, i32, i32) = sqlx::query_as(
            "SELECT delivered_at, rules_version, stanox_rows FROM corpus_crosswalk_build",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            (delivered_at, version, stanoxes),
            (at(), corpus_inference::RULES_VERSION, 4)
        );
        // Up to date: nothing to rebuild.
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn an_outdated_build_is_rebuilt_from_corpus_locations(pool: PgPool) {
        replace_corpus_locations(&pool, at(), "CORPUSExtract.json.gz", &corpus())
            .await
            .unwrap();
        // As if built by older rules, or before this table existed.
        sqlx::query("DELETE FROM corpus_tiploc_crs")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE corpus_crosswalk_build SET rules_version = 0")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), Some(at()));
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM corpus_tiploc_crs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 5);
        assert_eq!(rebuild_if_stale(&pool).await.unwrap(), None);
    }

    /// Flag off: every lookup returns exactly what it returned before
    /// CORPUS was loaded, and exactly what the process-wide default (off)
    /// returns.
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn with_the_flag_off_corpus_changes_nothing(pool: PgPool) {
        seed_timetable(&pool).await;
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
