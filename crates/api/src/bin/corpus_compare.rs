//! `corpus_compare`: read-only report comparing the loaded Network Rail
//! CORPUS (`corpus_locations`) with the timetable-derived crosswalk
//! (`tiploc_crs`, `stanox_crs`) and the Knowledgebase station names. The
//! logic and the meaning of each bucket are in
//! `api::data::corpus_comparison`.
//!
//! Shipped in the api image, so in production it runs inside the api pod
//! with the pod's own `DATABASE_URL` (no port-forward, no credential):
//!
//! ```text
//!   kubectl -n distant-signal exec deploy/distant-signal-api -c api -- corpus_compare > corpus-report.txt
//!   kubectl -n distant-signal exec deploy/distant-signal-api -c api -- corpus_compare --full > corpus-report.txt
//! ```
//!
//! Locally: `DATABASE_URL=postgres://... cargo run -p api --bin corpus_compare`.
//!
//! `--export-corpus-json` instead prints the loaded CORPUS back out in the
//! RDM extract's own JSON shape (`{"TIPLOCDATA":[...]}`), for
//! `line-catalogue-validator --regenerate-crs-tiploc-from-corpus` (see
//! reference-data/line-catalogue-validation.md, "Regenerating this
//! snapshot").
//!
//! Never writes. Exits 0 with a note when no CORPUS has been loaded (after
//! one `COUNT(*)`), non-zero only on a database error.

use chrono::NaiveDate;
use clap::Parser;
use sqlx::postgres::PgPoolOptions;

use api::data::corpus_comparison::{self, ReportDetail};
use api::data::corpus_crosswalk;

#[derive(Debug, Parser)]
#[command(
    about = "Compares the loaded CORPUS with the timetable-derived TIPLOC/STANOX->CRS crosswalk."
)]
struct Args {
    /// Also list every name difference and every key only one side has.
    #[arg(long)]
    full: bool,
    /// Service date whose calling points count as "in the timetable" for
    /// the "CRS-less TIPLOCs CORPUS could fill" line. Default: today in
    /// Europe/London.
    #[arg(long, value_name = "YYYY-MM-DD", conflicts_with = "no_calling_points")]
    calling_points_date: Option<NaiveDate>,
    /// Skip the calling-points read.
    #[arg(long)]
    no_calling_points: bool,
    /// Print the loaded CORPUS as `{"TIPLOCDATA":[...]}` instead of a report.
    #[arg(long)]
    export_corpus_json: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await?;

    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM corpus_locations")
        .fetch_one(&pool)
        .await?;
    if count == 0 {
        eprintln!("no CORPUS delivery is loaded (corpus_locations is empty); nothing to compare");
        return Ok(());
    }
    let delivery: Option<(chrono::DateTime<chrono::Utc>, String)> = sqlx::query_as(
        "SELECT delivered_at, source_file FROM corpus_deliveries ORDER BY delivered_at DESC LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?;
    let locations = corpus_crosswalk::load_corpus_locations(&pool).await?;

    if args.export_corpus_json {
        let blank = |v: &Option<String>| v.clone().unwrap_or_else(|| " ".to_owned());
        let rows: Vec<serde_json::Value> = locations
            .iter()
            .map(|l| {
                serde_json::json!({
                    "NLC": l.nlc,
                    "STANOX": blank(&l.stanox),
                    "TIPLOC": blank(&l.tiploc),
                    "3ALPHA": blank(&l.crs),
                    "UIC": blank(&l.uic),
                    "NLCDESC": blank(&l.nlc_desc),
                    "NLCDESC16": blank(&l.nlc_desc16),
                })
            })
            .collect();
        println!("{}", serde_json::json!({ "TIPLOCDATA": rows }));
        return Ok(());
    }

    let date = if args.no_calling_points {
        None
    } else {
        Some(args.calling_points_date.unwrap_or_else(|| {
            chrono::Utc::now()
                .with_timezone(&chrono_tz::Europe::London)
                .date_naive()
        }))
    };
    let timetable = corpus_comparison::load_timetable(&pool, date).await?;
    let comparison = corpus_comparison::compare(&locations, &timetable);
    if let Some((delivered_at, source_file)) = delivery {
        println!("CORPUS delivery: {source_file}, delivered {delivered_at}");
    }
    let detail = if args.full {
        ReportDetail::Full
    } else {
        ReportDetail::Standard
    };
    print!("{}", corpus_comparison::render(&comparison, detail));
    Ok(())
}
