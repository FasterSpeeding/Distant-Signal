//! Helpers shared by the CORPUS comparison (`crate::data::corpus_comparison`):
//! reading `corpus_locations` and running the shared conservative inference
//! (`common::corpus_inference`) over it.

use anyhow::Result;
use common::corpus_inference::{self, CorpusCrsTiploc, CorpusRow, Crosswalk};

use crate::data::corpus::CorpusLocation;

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

