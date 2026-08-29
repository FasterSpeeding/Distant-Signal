//! Best-effort, review-before-save auto-fill for ticket entry: reads
//! openly-documented file formats a user already has (Apple Wallet
//! `.pkpass`, PDF e-tickets) and returns a `PartialTicket` preview -- this
//! module and every function in it NEVER writes to the database (see
//! docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md's Global
//! Constraints on review-before-save) and NEVER decodes a barcode or
//! touches ITSO data, in either format (see the design doc's Non-goals).

use serde::Serialize;

/// What a `.pkpass`/PDF parse could recover -- the same fillable fields as
/// `common::TicketEntryRequest`, minus a user-chosen `source` (this is
/// fixed per parse path) plus a fixed `source` describing which one
/// produced it. `None` means "not found in this file, leave for the user
/// to fill in" -- never guessed at. This is exactly what a human sees on a
/// review-before-save form pre-filled from an upload; nothing here is ever
/// written to `tracked_train_tickets` directly -- see this module's own
/// doc comment.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialTicket {
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    /// Best-effort station identifier -- almost never a real CRS code in
    /// practice (neither `.pkpass` nor PDF extraction publishes one; both
    /// give station NAMES, e.g. "Kings Cross"). Deliberately NOT
    /// normalized here: `train_tracking::validate_ticket_entry`'s existing
    /// CRS-format check is what actually forces a human to correct this
    /// into a real code before it can be saved -- see this plan's Global
    /// Constraints.
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub source: &'static str,
}

/// Pure: given `pass.json`'s already-parsed content, returns a
/// `PartialTicket`, preferring Apple's standardised `semantics` dictionary
/// (`departureStationName`/`destinationStationName`) when present, falling
/// back to the positional `primaryFields` convention Apple's own PassKit
/// docs specify for a boarding/transit pass (exactly two entries:
/// departure, then arrival, in that order -- positional, not per-issuer
/// label-string matching, since the ordering is Apple's own convention,
/// not each issuer's choice) when it isn't. See
/// docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
/// Open Question 1: which real UK retailers populate `semantics` is
/// unconfirmed, so both paths are implemented, not just the optimistic
/// one -- obtain 1-2 real sample passes to confirm this split's real-world
/// hit rate before relying on it heavily.
pub fn parse_pass_json(pass: &serde_json::Value) -> anyhow::Result<PartialTicket> {
    let boarding_pass = pass.get("boardingPass").ok_or_else(|| anyhow::anyhow!("not a boardingPass-style pkpass"))?;
    let transit_type = boarding_pass.get("transitType").and_then(|v| v.as_str()).unwrap_or_default();
    anyhow::ensure!(transit_type == "PKTransitTypeTrain", "not a train boarding pass (transitType = {transit_type:?})");

    let operator = pass.get("organizationName").and_then(|v| v.as_str()).map(str::to_string);
    let semantics = boarding_pass.get("semantics");

    let (origin, destination, source) = if let Some((origin, destination)) = semantics.and_then(semantics_origin_destination) {
        (Some(origin), Some(destination), "pkpass-semantics")
    } else {
        let (origin, destination) = primary_fields_origin_destination(boarding_pass);
        (origin, destination, "pkpass-heuristic")
    };

    Ok(PartialTicket { operator, ticket_type: None, origin_crs: origin, destination_crs: destination, source })
}

fn semantics_origin_destination(semantics: &serde_json::Value) -> Option<(String, String)> {
    let origin = semantics.get("departureStationName").and_then(|v| v.as_str())?;
    let destination = semantics.get("destinationStationName").and_then(|v| v.as_str())?;
    Some((origin.to_string(), destination.to_string()))
}

/// Apple's PassKit docs specify a boarding-pass-style pass's
/// `primaryFields` array holds exactly two entries for a transit pass:
/// departure, then arrival, in that order. Returns `(None, None)` for
/// anything that doesn't match that exact two-field shape, rather than
/// guessing at which field is which.
fn primary_fields_origin_destination(boarding_pass: &serde_json::Value) -> (Option<String>, Option<String>) {
    let Some(fields) = boarding_pass.get("primaryFields").and_then(|v| v.as_array()) else {
        return (None, None);
    };
    match fields.as_slice() {
        [origin, destination] => (
            origin.get("value").and_then(|v| v.as_str()).map(str::to_string),
            destination.get("value").and_then(|v| v.as_str()).map(str::to_string),
        ),
        _ => (None, None),
    }
}

use std::io::Read;

/// `pass.json` is plain-text JSON, and real ones are a few KB -- this
/// bounds every ZIP-entry read in this function against a zip-bomb-style
/// small-file/huge-decompressed-content mismatch (see this plan's Global
/// Constraints on file upload hygiene).
const MAX_ENTRY_BYTES: u64 = 1_000_000; // 1 MiB

/// Thin wrapper: unzips the `.pkpass` container, reads `pass.json`,
/// deserializes it, and hands off to `parse_pass_json` (the actual logic,
/// fully unit-tested above). Not unit-tested beyond the round-trip smoke
/// test below -- this function's own job (calling into the `zip` crate
/// correctly) is thin enough that `parse_pass_json`'s own tests carry the
/// real coverage, mirroring `auth::oidc::OidcClient`'s untested-plumbing
/// precedent.
pub fn parse_pkpass(bytes: &[u8]) -> anyhow::Result<PartialTicket> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|err| anyhow::anyhow!("not a valid .pkpass (zip) file: {err}"))?;
    let mut entry = archive
        .by_name("pass.json")
        .map_err(|err| anyhow::anyhow!("pass.json not found in .pkpass archive: {err}"))?;

    let mut buf = Vec::new();
    entry.by_ref().take(MAX_ENTRY_BYTES).read_to_end(&mut buf)?;

    let pass: serde_json::Value = serde_json::from_slice(&buf).map_err(|err| anyhow::anyhow!("pass.json is not valid JSON: {err}"))?;
    parse_pass_json(&pass)
}

#[cfg(test)]
mod pass_json_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semantics_present_is_preferred_and_labelled_accordingly() {
        let pass = json!({
            "organizationName": "LNER",
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [{"key":"origin","label":"FROM","value":"Kings Cross"}],
                "semantics": {
                    "departureStationName": "Kings Cross",
                    "destinationStationName": "Edinburgh"
                }
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(ticket.operator, Some("LNER".to_string()));
        assert_eq!(ticket.origin_crs, Some("Kings Cross".to_string()));
        assert_eq!(ticket.destination_crs, Some("Edinburgh".to_string()));
        assert_eq!(ticket.source, "pkpass-semantics");
    }

    #[test]
    fn semantics_absent_falls_back_to_the_two_field_primary_fields_heuristic() {
        let pass = json!({
            "organizationName": "Trainline",
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [
                    {"key":"origin","label":"FROM","value":"London Waterloo"},
                    {"key":"destination","label":"TO","value":"Woking"}
                ]
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(ticket.origin_crs, Some("London Waterloo".to_string()));
        assert_eq!(ticket.destination_crs, Some("Woking".to_string()));
        assert_eq!(ticket.source, "pkpass-heuristic");
    }

    #[test]
    fn a_primary_fields_array_of_the_wrong_length_yields_none_not_a_guess() {
        let pass = json!({
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [{"key":"a","value":"1"}, {"key":"b","value":"2"}, {"key":"c","value":"3"}]
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(ticket.origin_crs, None);
        assert_eq!(ticket.destination_crs, None);
        assert_eq!(ticket.source, "pkpass-heuristic");
    }

    #[test]
    fn a_non_train_transit_type_is_rejected() {
        let pass = json!({"boardingPass": {"transitType": "PKTransitTypeAir"}});
        assert!(parse_pass_json(&pass).is_err());
    }

    #[test]
    fn a_pass_with_no_boarding_pass_at_all_is_rejected() {
        let pass = json!({"organizationName": "Not A Boarding Pass"});
        assert!(parse_pass_json(&pass).is_err());
    }

    #[test]
    fn ticket_type_is_never_guessed_at() {
        let pass = json!({"boardingPass": {"transitType": "PKTransitTypeTrain"}});
        assert_eq!(parse_pass_json(&pass).unwrap().ticket_type, None);
    }
}

#[cfg(test)]
mod parse_pkpass_tests {
    use super::*;
    use std::io::Write;

    fn build_pkpass(pass_json: &serde_json::Value) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            writer.start_file("pass.json", zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(pass_json.to_string().as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn a_well_formed_pkpass_round_trips_through_the_full_pipeline() {
        let pass = serde_json::json!({
            "organizationName": "LNER",
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "semantics": {"departureStationName": "Kings Cross", "destinationStationName": "Edinburgh"}
            }
        });
        let bytes = build_pkpass(&pass);
        let ticket = parse_pkpass(&bytes).unwrap();
        assert_eq!(ticket.operator, Some("LNER".to_string()));
        assert_eq!(ticket.source, "pkpass-semantics");
    }

    #[test]
    fn a_zip_with_no_pass_json_is_rejected() {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            writer.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(b"not a pass").unwrap();
            writer.finish().unwrap();
        }
        assert!(parse_pkpass(&buf).is_err());
    }

    #[test]
    fn bytes_that_are_not_a_zip_at_all_are_rejected() {
        assert!(parse_pkpass(b"this is definitely not a zip file").is_err());
    }
}

/// Pure: given a PDF's already-extracted raw text, applies a small,
/// explicitly per-retailer set of best-effort heuristics. No standardised
/// UK rail e-ticket PDF layout exists across retailers (see
/// docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
/// Research summary §3 and Open Question 2) -- this is a genuinely
/// fragile, lower-confidence tier than `.pkpass` parsing, by design; an
/// unmatched field is left `None` for manual completion, never guessed at
/// when nothing matches. `ROUTE_PATTERN` in particular is best-effort in
/// the other direction too: because it matches against unstructured,
/// unanchored text with no field boundaries, it can occasionally capture
/// nearby boilerplate prose rather than the actual route (see that
/// pattern's own doc comment) -- `train_tracking::validate_ticket_entry`'s
/// CRS-format check (Task 2) is what actually prevents an unedited false
/// match from ever being saved, not this regex's own precision.
pub fn parse_pdf_text(text: &str) -> PartialTicket {
    let operator = KNOWN_RETAILER_MARKERS.iter().find(|marker| text.contains(**marker)).map(|marker| marker.to_string());

    let (origin, destination) = ROUTE_PATTERN
        .captures(text)
        .map(|caps| (Some(caps[1].trim().to_string()), Some(caps[2].trim().to_string())))
        .unwrap_or((None, None));

    let text_lower = text.to_lowercase();
    let ticket_type = TICKET_TYPE_KEYWORDS.iter().find(|kw| text_lower.contains(&kw.to_lowercase())).map(|kw| kw.to_string());

    PartialTicket { operator, ticket_type, origin_crs: origin, destination_crs: destination, source: "pdf-heuristic" }
}

/// The "smallest possible set of known templates" the design doc's Open
/// Question 2 calls for -- LNER and Trainline only, per that same note.
/// Expanding this list is real follow-up work, not attempted here.
const KNOWN_RETAILER_MARKERS: &[&str] = &["LNER", "Trainline"];

const TICKET_TYPE_KEYWORDS: &[&str] =
    &["Anytime Day Single", "Off-Peak Day Single", "Off-Peak Day Return", "Advance Single", "Season", "Open Return"];

/// Matches the "<origin> to <destination>" shape the design doc's own
/// worked example uses ("18:32 London Waterloo to Woking, Off-Peak Day
/// Single") -- deliberately conservative (letters/spaces/apostrophes/
/// hyphens only) since this matches against unstructured extracted text
/// with no field boundaries at all. The trailing delimiter accepts a
/// comma/period/newline OR end-of-string, so a route with nothing after it
/// (e.g. the destination is the last thing in the extracted text) still
/// matches. `captures()` returns the leftmost match in the whole document
/// with no anchoring to a specific line, so this can and occasionally will
/// latch onto unrelated boilerplate prose containing "... to ..." before
/// the real route line (e.g. "Please remember to bring photo ID... Leeds
/// to York."), not just the intended route -- this is a known, accepted
/// imprecision, not a bug to chase here; see `parse_pdf_text`'s doc comment
/// for why that's still safe. Confirm this against 1-2 real e-ticket PDFs
/// at implementation time (Open Question 2 flags real samples as needed,
/// same as `.pkpass`'s Open Question 1) and adjust -- this is a starting
/// point, not a pattern verified against real tickets.
static ROUTE_PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"([A-Za-z][A-Za-z '\-]+?)\s+to\s+([A-Za-z][A-Za-z '\-]+?)(?:[,\.\n]|$)").unwrap()
});

#[cfg(test)]
mod parse_pdf_text_tests {
    use super::*;

    #[test]
    fn matches_the_design_docs_own_worked_example() {
        let text = "LNER e-ticket\n18:32 London Waterloo to Woking, Off-Peak Day Single\nFare: withheld";
        let ticket = parse_pdf_text(text);
        assert_eq!(ticket.operator, Some("LNER".to_string()));
        assert_eq!(ticket.origin_crs, Some("London Waterloo".to_string()));
        assert_eq!(ticket.destination_crs, Some("Woking".to_string()));
        assert_eq!(ticket.ticket_type, Some("Off-Peak Day Single".to_string()));
        assert_eq!(ticket.source, "pdf-heuristic");
    }

    #[test]
    fn an_unrecognized_retailer_yields_no_operator_guess() {
        let ticket = parse_pdf_text("Some Other Retailer Ltd e-ticket, King's Cross to York, Anytime Day Single");
        assert_eq!(ticket.operator, None);
    }

    #[test]
    fn text_with_no_route_pattern_match_yields_no_stations() {
        let ticket = parse_pdf_text("LNER receipt: thank you for your purchase");
        assert_eq!(ticket.origin_crs, None);
        assert_eq!(ticket.destination_crs, None);
    }

    #[test]
    fn no_ticket_type_keyword_present_yields_none_not_a_guess() {
        let ticket = parse_pdf_text("Trainline: London Waterloo to Woking");
        assert_eq!(ticket.ticket_type, None);
    }

    #[test]
    fn a_route_with_nothing_after_the_destination_still_matches() {
        // No trailing comma/period/newline after "Woking" -- the
        // destination is the last thing in the text. See ROUTE_PATTERN's
        // doc comment for why `$` is part of its trailing delimiter.
        let ticket = parse_pdf_text("Trainline: London Waterloo to Woking");
        assert_eq!(ticket.origin_crs, Some("London Waterloo".to_string()));
        assert_eq!(ticket.destination_crs, Some("Woking".to_string()));
    }
}

/// Thin wrapper: validates the `%PDF-` magic header, extracts the native
/// text layer via the third-party `pdf_extract` crate, and hands off to
/// `parse_pdf_text` (the actual logic, fully unit-tested above).
///
/// `catch_unwind`: `pdf_extract` parses untrusted, potentially-malformed
/// input via code this app doesn't control; a panic inside it must fail
/// this one request, not take the whole handler down. See this plan's
/// Global Constraints on file upload hygiene.
pub fn parse_pdf(bytes: &[u8]) -> anyhow::Result<PartialTicket> {
    anyhow::ensure!(bytes.starts_with(b"%PDF-"), "not a PDF file (missing %PDF- header)");

    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes))
        .map_err(|_| anyhow::anyhow!("PDF text extraction panicked"))?
        .map_err(|err| anyhow::anyhow!("failed to extract text from PDF: {err}"))?;

    Ok(parse_pdf_text(&text))
}

#[cfg(test)]
mod parse_pdf_tests {
    use super::*;

    #[test]
    fn bytes_without_the_pdf_magic_header_are_rejected_before_extraction_is_attempted() {
        assert!(parse_pdf(b"this is not a pdf").is_err());
    }
}
