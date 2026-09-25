//! Best-effort, review-before-save auto-fill for ticket entry: reads
//! openly-documented file formats a user already has (Apple Wallet
//! `.pkpass`, PDF e-tickets) and returns a `PartialTicket` preview -- this
//! module and every function in it NEVER writes to the database (see
//! docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md's Global
//! Constraints on review-before-save) and NEVER decodes a barcode or
//! touches ITSO data, in either format (see the design doc's Non-goals).

use chrono::{DateTime, Utc};
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
    /// The ticket's own claimed departure INSTANT, when the source format
    /// carries one -- currently only ever populated from a `.pkpass`'s
    /// `semantics.currentDepartureDate` (Apple's standardised key for
    /// exactly this, an ISO 8601 date-time string), and only `Some` when
    /// that key parses as a real `DateTime`. `None` for every PDF-sourced
    /// ticket (no PDF date/time extraction exists in this module -- neither
    /// tier's ticket-type/route regexes attempt one, and adding one would
    /// be a new, separate false-positive surface against unstructured
    /// text); also `None` for a `.pkpass` whose `semantics` dictionary is
    /// absent, or present but missing this one key, or a `primaryFields`
    /// heuristic match (that positional fallback is names only, per
    /// `primary_fields_origin_destination`'s own doc comment -- it has
    /// nothing dateable to read either).
    ///
    /// This is deliberately a best-effort HINT for a journey-leg search
    /// window (`data::journey_leg_proposal::propose_window_leg`), never a
    /// hard pin: a ticket's booked departure can genuinely differ from the
    /// train the traveller actually catches (an earlier/later service on
    /// the same ticket, a rebooked journey, ...), so nothing in this
    /// codebase ever treats this field as an exact scheduled-departure
    /// match target the way `TrackPinRequest::scheduled_departure` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_departure_date: Option<DateTime<Utc>>,
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
/// `ticket_type` is read from `boardingPass.auxiliaryFields` by exact
/// `key` match (`"ticketType"`) via `keyed_field_value` -- `None` if that
/// key isn't present, never guessed from label text or other fields.
pub fn parse_pass_json(pass: &serde_json::Value) -> anyhow::Result<PartialTicket> {
    let boarding_pass = pass
        .get("boardingPass")
        .ok_or_else(|| anyhow::anyhow!("not a boardingPass-style pkpass"))?;
    let transit_type = boarding_pass
        .get("transitType")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    anyhow::ensure!(
        transit_type == "PKTransitTypeTrain",
        "not a train boarding pass (transitType = {transit_type:?})"
    );

    let operator = pass
        .get("organizationName")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let semantics = boarding_pass.get("semantics");

    let (origin, destination, source) =
        if let Some((origin, destination)) = semantics.and_then(semantics_origin_destination) {
            (Some(origin), Some(destination), "pkpass-semantics")
        } else {
            let (origin, destination) = primary_fields_origin_destination(boarding_pass);
            (origin, destination, "pkpass-heuristic")
        };

    let ticket_type = boarding_pass
        .get("auxiliaryFields")
        .and_then(|fields| keyed_field_value(fields, "ticketType"));

    let current_departure_date = semantics.and_then(semantics_current_departure_date);

    // Diagnostic only -- never surfaced in PartialTicket, the frontend, or
    // any persisted row. debug-level specifically so it costs nothing in
    // default-configured production logging and cannot become a de facto
    // data-collection channel without a deliberate decision to promote it.
    // See Decision 3 of
    // docs/superpowers/specs/2026-09-02-ticket-processing-improvements-design.md.
    tracing::debug!(barcode_format = ?barcode_format(pass), "parsed .pkpass");

    Ok(PartialTicket {
        operator,
        ticket_type,
        origin_crs: origin,
        destination_crs: destination,
        current_departure_date,
        source,
    })
}

fn semantics_origin_destination(semantics: &serde_json::Value) -> Option<(String, String)> {
    let origin = semantics
        .get("departureStationName")
        .and_then(|v| v.as_str())?;
    let destination = semantics
        .get("destinationStationName")
        .and_then(|v| v.as_str())?;
    Some((origin.to_string(), destination.to_string()))
}

/// Reads `semantics.currentDepartureDate` -- Apple's standardised key,
/// confirmed present in Apple's own semantic-tags schema (see this module's
/// doc comment's cross-reference to
/// docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
/// research section) -- and parses it as an RFC 3339/ISO 8601 date-time.
/// `None` if the key is absent, not a string, or doesn't parse as a real
/// date-time -- same "leave it blank, don't guess" contract as every other
/// optional read in this module; a malformed value is exactly as useless as
/// a missing one, and is never worth surfacing as a parse error for a
/// preview endpoint the user reviews before anything is saved anyway.
fn semantics_current_departure_date(semantics: &serde_json::Value) -> Option<DateTime<Utc>> {
    semantics
        .get("currentDepartureDate")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

/// Apple's PassKit docs specify a boarding-pass-style pass's
/// `primaryFields` array holds exactly two entries for a transit pass:
/// departure, then arrival, in that order. Returns `(None, None)` for
/// anything that doesn't match that exact two-field shape, rather than
/// guessing at which field is which.
fn primary_fields_origin_destination(
    boarding_pass: &serde_json::Value,
) -> (Option<String>, Option<String>) {
    let Some(fields) = boarding_pass
        .get("primaryFields")
        .and_then(|v| v.as_array())
    else {
        return (None, None);
    };
    match fields.as_slice() {
        [origin, destination] => (
            origin
                .get("value")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            destination
                .get("value")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        ),
        _ => (None, None),
    }
}

/// Looks up an entry in a PassKit field array (`primaryFields`,
/// `auxiliaryFields`, `secondaryFields` -- all the same `{key, label,
/// value}` shape) by its machine-readable `key`, not by its
/// issuer-chosen, freely-reworded `label` text. Returns `None` if `fields`
/// isn't an array, or no entry has that exact key, or the matching
/// entry's `value` isn't a string -- same "leave it blank, don't guess"
/// contract as every other optional read in this module.
fn keyed_field_value(fields: &serde_json::Value, key: &str) -> Option<String> {
    fields
        .as_array()?
        .iter()
        .find(|f| f.get("key").and_then(|v| v.as_str()) == Some(key))?
        .get("value")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Reads only the barcode's `format` string (e.g.
/// `"PKBarcodeFormatAztec"`) from `pass.json`'s singular `"barcode"`
/// object or, per Apple's newer PassKit convention, the first entry of
/// the plural `"barcodes"` array -- documented container metadata,
/// structurally no different from `organizationName` or `transitType`,
/// both already read elsewhere in this module. NEVER reads `"message"`,
/// the barcode payload -- that field is categorically off limits, see
/// this module's own doc comment and
/// docs/superpowers/specs/2026-09-02-ticket-processing-improvements-design.md's
/// Explicitly out of scope section.
fn barcode_format(pass: &serde_json::Value) -> Option<String> {
    pass.get("barcode")
        .or_else(|| {
            pass.get("barcodes")
                .and_then(|b| b.as_array())
                .and_then(|a| a.first())
        })
        .and_then(|b| b.get("format"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
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
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|err| anyhow::anyhow!("not a valid .pkpass (zip) file: {err}"))?;
    let mut entry = archive
        .by_name("pass.json")
        .map_err(|err| anyhow::anyhow!("pass.json not found in .pkpass archive: {err}"))?;

    let mut buf = Vec::new();
    entry.by_ref().take(MAX_ENTRY_BYTES).read_to_end(&mut buf)?;

    let pass: serde_json::Value = serde_json::from_slice(&buf)
        .map_err(|err| anyhow::anyhow!("pass.json is not valid JSON: {err}"))?;
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
    fn ticket_type_is_read_from_auxiliary_fields_by_key() {
        let pass = json!({
            "organizationName": "Southern",
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [
                    {"key":"origin","label":"FROM","value":"East Croydon"},
                    {"key":"destination","label":"TO","value":"Brighton"}
                ],
                "auxiliaryFields": [
                    {"key": "ticketType", "label": "TICKET TYPE", "value": "Super Off-Peak Return"}
                ]
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(
            ticket.ticket_type,
            Some("Super Off-Peak Return".to_string())
        );
    }

    #[test]
    fn ticket_type_ignores_a_field_with_the_wrong_key() {
        let pass = json!({
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "auxiliaryFields": [
                    {"key": "railcard", "label": "TICKET TYPE DISCOUNT", "value": "Network Railcard"}
                ]
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(
            ticket.ticket_type, None,
            "must match by the key field exactly, not by label text that happens to mention ticket type"
        );
    }

    #[test]
    fn ticket_type_is_never_guessed_at() {
        let pass = json!({"boardingPass": {"transitType": "PKTransitTypeTrain"}});
        assert_eq!(parse_pass_json(&pass).unwrap().ticket_type, None);
    }

    #[test]
    fn current_departure_date_is_read_from_semantics_when_present() {
        let pass = json!({
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "semantics": {
                    "departureStationName": "Kings Cross",
                    "destinationStationName": "Edinburgh",
                    "currentDepartureDate": "2026-09-22T18:32:00Z"
                }
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(
            ticket.current_departure_date,
            Some("2026-09-22T18:32:00Z".parse().unwrap())
        );
    }

    #[test]
    fn current_departure_date_is_none_when_semantics_omits_it() {
        let pass = json!({
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "semantics": {
                    "departureStationName": "Kings Cross",
                    "destinationStationName": "Edinburgh"
                }
            }
        });
        assert_eq!(parse_pass_json(&pass).unwrap().current_departure_date, None);
    }

    #[test]
    fn current_departure_date_is_none_when_semantics_is_absent_entirely() {
        // Falls back to the `primaryFields` heuristic for origin/destination
        // (see `primary_fields_origin_destination`'s own doc comment) --
        // that positional match is names only, so there is nothing dateable
        // to read either.
        let pass = json!({
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [
                    {"key":"origin","label":"FROM","value":"London Waterloo"},
                    {"key":"destination","label":"TO","value":"Woking"}
                ]
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(ticket.source, "pkpass-heuristic");
        assert_eq!(ticket.current_departure_date, None);
    }

    #[test]
    fn current_departure_date_is_none_when_the_value_does_not_parse_as_a_real_date_time() {
        let pass = json!({
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "semantics": {
                    "departureStationName": "Kings Cross",
                    "destinationStationName": "Edinburgh",
                    "currentDepartureDate": "not-a-real-date"
                }
            }
        });
        assert_eq!(parse_pass_json(&pass).unwrap().current_departure_date, None);
    }
}

#[cfg(test)]
mod barcode_format_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_format_from_the_singular_barcode_object() {
        // The `message` value here is deliberately an obvious
        // non-payload placeholder -- never anything resembling a real
        // RSP-6 payload shape. See this plan's Global Constraints.
        let pass = json!({
            "barcode": {
                "format": "PKBarcodeFormatAztec",
                "message": "PLACEHOLDER-NOT-A-REAL-PAYLOAD",
                "messageEncoding": "iso-8859-1"
            }
        });
        assert_eq!(
            barcode_format(&pass),
            Some("PKBarcodeFormatAztec".to_string())
        );
    }

    #[test]
    fn falls_back_to_the_plural_barcodes_array() {
        let pass = json!({
            "barcodes": [
                {"format": "PKBarcodeFormatQR", "message": "PLACEHOLDER-NOT-A-REAL-PAYLOAD"}
            ]
        });
        assert_eq!(barcode_format(&pass), Some("PKBarcodeFormatQR".to_string()));
    }

    #[test]
    fn returns_none_when_neither_field_is_present() {
        let pass = json!({"organizationName": "LNER"});
        assert_eq!(barcode_format(&pass), None);
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
            writer
                .start_file("pass.json", zip::write::SimpleFileOptions::default())
                .unwrap();
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
            writer
                .start_file("readme.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
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
/// when nothing matches. The generic fallback pattern in `ROUTE_PATTERNS`
/// in particular is best-effort in the other direction too: because it
/// matches against unstructured, unanchored text with no field
/// boundaries, it can occasionally capture nearby boilerplate prose
/// rather than the actual route (see that pattern's own doc comment) --
/// `train_tracking::validate_ticket_entry`'s CRS-format check (Task 2) is
/// what actually prevents an unedited false match from ever being saved,
/// not this regex's own precision.
pub fn parse_pdf_text(text: &str) -> PartialTicket {
    let operator = KNOWN_RETAILER_MARKERS
        .iter()
        .find(|marker| text.contains(**marker))
        .map(|marker| marker.to_string());

    let (origin, destination) = extract_route(text);

    let text_lower = text.to_lowercase();
    let ticket_type = TICKET_TYPE_KEYWORDS
        .iter()
        .find(|kw| text_lower.contains(&kw.to_lowercase()))
        .map(|kw| kw.to_string());

    PartialTicket {
        operator,
        ticket_type,
        origin_crs: origin,
        destination_crs: destination,
        // No PDF date/time extraction exists in this module -- see
        // `PartialTicket::current_departure_date`'s own doc comment.
        current_departure_date: None,
        source: "pdf-heuristic",
    }
}

/// The "smallest possible set of known templates" the design doc's Open
/// Question 2 calls for -- LNER and Trainline only, per that same note.
/// Expanding this list is real follow-up work, not attempted here.
const KNOWN_RETAILER_MARKERS: &[&str] = &["LNER", "Trainline"];

const TICKET_TYPE_KEYWORDS: &[&str] = &[
    "Anytime Day Single",
    "Off-Peak Day Single",
    "Off-Peak Day Return",
    "Advance Single",
    "Season",
    "Open Return",
    "Super Off-Peak Return",
];

/// Two route-extraction patterns, tried in order by `extract_route`:
/// OTRL's anchored `Out:`/`Ret:` line first (higher confidence -- an
/// explicit label plus already-CRS-shaped codes), falling back to the
/// original generic "<name> to <name>" prose match. Mirrors the ordering
/// precedent one function away: `parse_pass_json` tries the
/// higher-confidence `semantics` dictionary before falling back to the
/// positional `primaryFields` heuristic -- "most specific/structured
/// signal first, generic fallback second" is now the same shape in both
/// parsers.
static ROUTE_PATTERNS: std::sync::LazyLock<[regex::Regex; 2]> = std::sync::LazyLock::new(|| {
    [
        // OTRL's "Out:"/"Ret:" line -- anchored, already CRS-shaped,
        // tried first. A small Unicode hyphen/dash range is accepted
        // defensively alongside plain ASCII "-", since it is unconfirmed
        // whether every OTRL PDF generation renders a plain ASCII hyphen
        // here (see this module's Open questions).
        regex::Regex::new(r"(?:Out|Ret):\s*([A-Z]{3})\s*[-\u{2010}-\u{2015}]\s*([A-Z]{3})")
            .unwrap(),
        // The original generic "<origin> to <destination>" prose match --
        // matches the design doc's own worked example ("18:32 London
        // Waterloo to Woking, Off-Peak Day Single"). Deliberately
        // conservative (letters/spaces/apostrophes/hyphens only) since
        // this matches against unstructured extracted text with no field
        // boundaries at all. The trailing delimiter accepts a
        // comma/period/newline OR end-of-string, so a route with nothing
        // after it (e.g. the destination is the last thing in the
        // extracted text) still matches. This is unanchored and can latch
        // onto unrelated boilerplate prose containing "... to ..." (e.g.
        // "Please remember to bring photo ID... Leeds to York.") -- a
        // known, accepted imprecision; the OTRL pattern above is tried
        // first specifically to prefer the higher-confidence match when
        // both are present. `train_tracking::validate_ticket_entry`'s
        // CRS-format check is what actually prevents an unedited false
        // match from ever being saved, not this regex's own precision.
        regex::Regex::new(r"([A-Za-z][A-Za-z '\-]+?)\s+to\s+([A-Za-z][A-Za-z '\-]+?)(?:[,\.\n]|$)")
            .unwrap(),
    ]
});

/// Tries each pattern in `ROUTE_PATTERNS` in order, returning the first
/// match's `(origin, destination)` capture pair. Returns `(None, None)`
/// if neither pattern matches -- no panic path, since `Regex::captures`
/// never panics on non-matching input.
fn extract_route(text: &str) -> (Option<String>, Option<String>) {
    for pattern in ROUTE_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(text) {
            return (
                Some(caps[1].trim().to_string()),
                Some(caps[2].trim().to_string()),
            );
        }
    }
    (None, None)
}

#[cfg(test)]
mod parse_pdf_text_tests {
    use super::*;

    #[test]
    fn matches_the_design_docs_own_worked_example() {
        let text =
            "LNER e-ticket\n18:32 London Waterloo to Woking, Off-Peak Day Single\nFare: withheld";
        let ticket = parse_pdf_text(text);
        assert_eq!(ticket.operator, Some("LNER".to_string()));
        assert_eq!(ticket.origin_crs, Some("London Waterloo".to_string()));
        assert_eq!(ticket.destination_crs, Some("Woking".to_string()));
        assert_eq!(ticket.ticket_type, Some("Off-Peak Day Single".to_string()));
        assert_eq!(ticket.source, "pdf-heuristic");
        // No PDF date/time extraction exists in this module -- see
        // `PartialTicket::current_departure_date`'s own doc comment. The
        // "18:32" in this very fixture's text is deliberately NOT read as a
        // departure time.
        assert_eq!(ticket.current_departure_date, None);
    }

    #[test]
    fn an_unrecognized_retailer_yields_no_operator_guess() {
        let ticket = parse_pdf_text(
            "Some Other Retailer Ltd e-ticket, King's Cross to York, Anytime Day Single",
        );
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
        // destination is the last thing in the text. See ROUTE_PATTERNS's
        // generic-fallback entry's doc comment for why `$` is part of its
        // trailing delimiter.
        let ticket = parse_pdf_text("Trainline: London Waterloo to Woking");
        assert_eq!(ticket.origin_crs, Some("London Waterloo".to_string()));
        assert_eq!(ticket.destination_crs, Some("Woking".to_string()));
    }

    #[test]
    fn otrl_out_ret_line_is_matched_when_the_generic_to_pattern_fails() {
        // Modeled on the real extracted-text shape a real OTRL PDF
        // produces: the route "arrow" line renders as a mangled glyph
        // with no literal "to" in it (here stood in for by a plain
        // placeholder line, since the real glyph mapping is unconfirmed
        // to be stable -- see this module's Open questions), while an
        // anchored Ret:/Out: line sits nearby with clean CRS-shaped codes.
        let text = "Southern e-ticket\n= 1 Sep 2026 Ret: ABC - XYZ\nSTATION A [glyph] STATION B\nSuper Off-Peak Return";
        let ticket = parse_pdf_text(text);
        assert_eq!(ticket.origin_crs, Some("ABC".to_string()));
        assert_eq!(ticket.destination_crs, Some("XYZ".to_string()));
    }

    #[test]
    fn otrl_pattern_is_preferred_over_a_coincidental_to_match_earlier_in_the_text() {
        // A generic-pattern false positive (unrelated "...to bring..."
        // prose) appears BEFORE the anchored Out:/Ret: line in document
        // order -- the ordered chain must still prefer the higher-confidence
        // anchored pattern, not whichever a plain first-match scan hits.
        let text = "Please remember to bring photo ID.\nOut: ABC - XYZ\nSuper Off-Peak Return";
        let ticket = parse_pdf_text(text);
        assert_eq!(ticket.origin_crs, Some("ABC".to_string()));
        assert_eq!(ticket.destination_crs, Some("XYZ".to_string()));
    }

    #[test]
    fn ticket_type_matches_the_super_off_peak_return_keyword() {
        let ticket = parse_pdf_text("Southern e-ticket\nOut: ABC - XYZ\nSuper Off-Peak Return");
        assert_eq!(
            ticket.ticket_type,
            Some("Super Off-Peak Return".to_string())
        );
    }
}

/// Thin wrapper: validates the `%PDF-` magic header, runs
/// `reject_pdf_compression_bombs` as a size guard, extracts the native
/// text layer via the third-party `pdf_extract` crate, and hands off to
/// `parse_pdf_text` (the actual logic, fully unit-tested above).
///
/// `catch_unwind`: `pdf_extract` parses untrusted, potentially-malformed
/// input via code this app doesn't control; a panic inside it must fail
/// this one request, not take the whole handler down. See this plan's
/// Global Constraints on file upload hygiene.
///
/// **`catch_unwind` does NOT cover an allocation failure (Finding #1 of
/// the 2026-09-25 review).** In Rust, a failed allocation `abort()`s the
/// whole process -- it is not a panic, `catch_unwind` cannot intercept it,
/// and neither can the `tokio::time::timeout` wrapped around this call by
/// `routes::train::handle_pdf_upload` (that timeout only stops the caller
/// from *awaiting* the spawned blocking task; the task itself, and the OS
/// thread running it, keep executing to completion or crash regardless).
/// `pdf_extract` fully inflates every `FlateDecode`/`LZWDecode` stream in
/// the document into memory via `lopdf`, which places NO bound of its own
/// on the decompressed size anywhere -- confirmed directly against
/// `lopdf` 0.42.0's source
/// (`Stream::decompressed_content`/`decompress_zlib` in `object.rs`
/// `read_to_end`s an unbounded `Vec`, and this happens even during
/// `Document::load_mem` itself, for compressed xref/object streams, not
/// merely while extracting a page's own content stream later). A small,
/// highly-compressed stream within this route's ordinary 8 MiB
/// `DefaultBodyLimit` (see `routes::train`) can therefore make `lopdf` try
/// to allocate gigabytes, aborting the entire API process for every
/// in-flight request, not just this one.
///
/// `reject_pdf_compression_bombs` below is called BEFORE
/// `pdf_extract::extract_text_from_mem` specifically so a pathological
/// upload never reaches `lopdf`'s unbounded path at all. See that
/// function's own doc comment for exactly what it does and does not
/// protect against -- it is a real, meaningful mitigation, not a complete
/// one; complete protection would require running the actual parse in a
/// separate, resource-limited process, which is a larger follow-up than
/// this pass's scope.
pub fn parse_pdf(bytes: &[u8]) -> anyhow::Result<PartialTicket> {
    anyhow::ensure!(
        bytes.starts_with(b"%PDF-"),
        "not a PDF file (missing %PDF- header)"
    );
    anyhow::ensure!(
        bytes.len() <= MAX_PDF_UPLOAD_BYTES,
        "PDF is too large ({} bytes; this route accepts at most {} bytes for a PDF e-ticket)",
        bytes.len(),
        MAX_PDF_UPLOAD_BYTES
    );

    reject_pdf_compression_bombs(bytes)?;

    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes))
        .map_err(|_| anyhow::anyhow!("PDF text extraction panicked"))?
        .map_err(|err| anyhow::anyhow!("failed to extract text from PDF: {err}"))?;

    Ok(parse_pdf_text(&text))
}

/// A PDF upload above this size is rejected before any parsing is
/// attempted -- tighter than `routes::train`'s generic 8 MiB
/// `DefaultBodyLimit` (which also covers the unrelated `.pkpass` route, a
/// bounded zip-entry read with no comparable risk). No legitimate ticket
/// PDF (a boarding pass or e-ticket, typically well under 1 MiB)
/// approaches this size; a smaller input also caps how many
/// `stream`/`endstream` spans `reject_pdf_compression_bombs` below has to
/// scan for a given upload.
const MAX_PDF_UPLOAD_BYTES: usize = 4 * 1024 * 1024;

/// Cumulative decompressed-bytes budget for `reject_pdf_compression_bombs`'s
/// bounded pre-scan. Comfortably above any legitimate ticket PDF's actual
/// content (a boarding pass's real text/image streams add up to at most a
/// few MiB decompressed) while still bounding a malicious stream's blast
/// radius to a fixed, small amount of real memory -- see
/// `reject_pdf_compression_bombs`'s own doc comment for how this bound is
/// actually enforced without ever materializing a bomb's full output.
const MAX_TOTAL_INFLATED_BYTES: usize = 256 * 1024 * 1024;

/// **Finding #1 of the 2026-09-25 review's mitigation**: a best-effort,
/// in-process guard against a PDF compression bomb, run BEFORE this
/// module hands `bytes` to `pdf_extract`/`lopdf` (see `parse_pdf`'s own
/// doc comment for why that hand-off is otherwise unprotected).
///
/// ## What this does
///
/// `lopdf` exposes no way to cap decompressed stream size, and no way to
/// learn a stream's DECOMPRESSED length up front (a PDF's own `/Length`
/// key on a stream is the COMPRESSED length -- already bounded by
/// `MAX_PDF_UPLOAD_BYTES` above -- not the decompressed one, so it cannot
/// be used to reject a high compression RATIO). So instead of asking
/// `lopdf` anything, this function does its OWN, completely independent,
/// bounded decompression pass directly over the raw file bytes:
///
/// 1. Naively scans for every `stream` ... `endstream` span in the raw
///    bytes (the literal PDF syntax marking a stream's raw data --
///    intentionally NOT a real PDF object/xref parse, so this doesn't
///    need to trust or replicate any of `lopdf`'s own object-graph
///    logic).
/// 2. For each span, attempts a zlib inflate (`FlateDecode` is the
///    overwhelming majority filter in real-world PDFs, and the only one
///    a genuine ticket PDF is realistically going to use) through a
///    small, FIXED-SIZE, REUSED buffer -- never a growing `Vec` -- so
///    this function's own memory use stays tiny regardless of how large
///    a malicious stream claims to decompress to.
/// 3. Aborts a single stream's decompression, and immediately rejects the
///    whole file, the instant the RUNNING TOTAL across every stream seen
///    so far exceeds `MAX_TOTAL_INFLATED_BYTES` -- without ever letting
///    any decoder run to completion on a bomb.
///
/// A span that isn't valid zlib data (most real PDF streams aren't --
/// images, fonts, and already-compressed content commonly use other or no
/// filters) simply contributes 0 bytes and is skipped: this is a pre-scan
/// for compression bombs specifically, not a general PDF parser, so a
/// span this function can't make sense of is, by definition, not a
/// zlib-based bomb it needs to catch.
///
/// ## What this does NOT protect against (documented honestly, not
/// swept under the rug)
///
/// - **Not a real PDF parser.** The naive `stream`/`endstream` token scan
///   can be fooled by a stream whose raw (compressed) bytes happen to
///   contain the literal ASCII bytes `endstream` before the real
///   terminator -- entirely possible in arbitrary binary data. This would
///   make this function inspect a truncated slice, which can UNDER-count
///   (and in the worst case, mean a bomb hiding past a spurious match
///   isn't caught by this particular scan iteration) rather than over-count.
/// - **Chained filters beyond the first stage are only partly covered.**
///   A PDF stream may declare multiple chained filters (e.g.
///   `/Filter [FlateDecode FlateDecode]`); this function's raw-byte scan
///   has no dictionary to read `/Filter` from, so it only ever inflates
///   the RAW bytes once. Any stream whose FIRST inflate stage alone would
///   exceed `MAX_TOTAL_INFLATED_BYTES` is still caught (which covers the
///   realistic, single-stage bomb shape this finding describes); a bomb
///   engineered to stay small through its first stage and only explode on
///   a LATER chained stage would evade this specific guard.
/// - **Not process isolation.** This is an in-process heuristic sitting in
///   front of `lopdf`, not a sandbox around it. `lopdf` itself is
///   unmodified and just as unbounded as before for anything this
///   pre-scan doesn't happen to catch. The genuinely complete fix -- running
///   the actual `pdf_extract` parse in a separate, resource-limited OS
///   process so an allocation failure there can't take down the API -- is
///   a larger change than this pass's scope and is named here as the real
///   follow-up, not silently deferred.
fn reject_pdf_compression_bombs(bytes: &[u8]) -> anyhow::Result<()> {
    let mut total_inflated: usize = 0;
    let mut pos: usize = 0;

    while let Some(rel) = find_subslice(&bytes[pos..], b"stream") {
        let keyword_start = pos + rel;
        let mut data_start = keyword_start + b"stream".len();
        // PDF spec (ISO 32000-1 §7.3.8.1): the `stream` keyword is
        // followed by CRLF or a bare LF (never a bare CR alone) before the
        // raw stream data begins.
        if bytes.get(data_start) == Some(&b'\r') && bytes.get(data_start + 1) == Some(&b'\n') {
            data_start += 2;
        } else if bytes.get(data_start) == Some(&b'\n') {
            data_start += 1;
        }

        let Some(end_rel) = find_subslice(&bytes[data_start..], b"endstream") else {
            // No matching terminator for the rest of the file -- nothing
            // further to scan.
            break;
        };
        let data_end = data_start + end_rel;
        let raw = &bytes[data_start..data_end];

        let remaining_budget = MAX_TOTAL_INFLATED_BYTES.saturating_sub(total_inflated);
        total_inflated += bounded_inflate_len(raw, remaining_budget);
        anyhow::ensure!(
            total_inflated <= MAX_TOTAL_INFLATED_BYTES,
            "PDF contains a stream that decompresses to an implausible size; refusing to parse it"
        );

        pos = data_end + b"endstream".len();
    }

    Ok(())
}

/// Returns the byte offset of the first occurrence of `needle` in
/// `haystack`, or `None`. A tiny, dependency-free substring search --
/// `bytes::Bytes`/`memchr` aren't already dependencies of this crate for
/// this one call site, and `haystack`/`needle` here are small enough
/// (bounded by `MAX_PDF_UPLOAD_BYTES`) that the naive `O(n*m)` worst case
/// is not a real concern.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Streams `raw` through zlib inflate into a small, reused, FIXED-SIZE
/// buffer -- never materializing the decompressed output as a whole -- and
/// returns how many bytes came out. Stops reading (without decoding the
/// rest) the instant the running total exceeds `budget`, so this function
/// never spends more real memory than one buffer's worth, however large
/// `raw` claims to decompress to. `raw` that isn't valid zlib data (or
/// whose stream is truncated/corrupt) simply stops early via its own
/// `Err`, contributing whatever it decoded before failing -- there is
/// nothing more that can be recovered from it, and it contributed no risk
/// either, since it never got the chance to produce unbounded output.
fn bounded_inflate_len(raw: &[u8], budget: usize) -> usize {
    use std::io::Read;

    let mut decoder = flate2::read::ZlibDecoder::new(raw);
    let mut buf = [0u8; 64 * 1024];
    let mut total = 0usize;
    loop {
        let n = match decoder.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        total += n;
        if total > budget {
            break;
        }
    }
    total
}

#[cfg(test)]
mod parse_pdf_tests {
    use super::*;

    #[test]
    fn bytes_without_the_pdf_magic_header_are_rejected_before_extraction_is_attempted() {
        assert!(parse_pdf(b"this is not a pdf").is_err());
    }
}

#[cfg(test)]
mod reject_pdf_compression_bombs_tests {
    use super::*;

    /// Builds a minimal, syntactically-plausible `stream ... endstream`
    /// span (with no surrounding PDF object machinery -- the scan doesn't
    /// need it) wrapping `raw` compressed bytes, the exact shape
    /// `reject_pdf_compression_bombs`'s naive token scan looks for.
    fn wrap_stream(raw: &[u8]) -> Vec<u8> {
        let mut out = b"stream\n".to_vec();
        out.extend_from_slice(raw);
        out.extend_from_slice(b"\nendstream");
        out
    }

    fn zlib_compress(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        // `fast()`, not `best()`: these fixtures are large, all-zero
        // buffers specifically so the compression ratio is enormous even
        // at the cheapest effort level -- keeping the test suite itself
        // fast is more useful here than squeezing out a marginally better
        // ratio.
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(data).expect("compress fixture data");
        encoder.finish().expect("finish zlib stream")
    }

    #[test]
    fn an_ordinary_small_stream_is_accepted() {
        let compressed = zlib_compress(b"a perfectly ordinary, small ticket PDF content stream");
        let pdf_like = wrap_stream(&compressed);
        assert!(reject_pdf_compression_bombs(&pdf_like).is_ok());
    }

    #[test]
    fn a_file_with_no_stream_keyword_at_all_is_accepted() {
        assert!(
            reject_pdf_compression_bombs(b"%PDF-1.4\nnothing stream-shaped here at all").is_ok()
        );
    }

    /// The real failure shape Finding #1 exists to prevent: a small,
    /// highly-compressed stream that would decompress to something far
    /// past any real ticket PDF's needs. `flate2` can happily compress a
    /// large, repetitive buffer down to a tiny fraction of its size --
    /// this fixture is well within `MAX_PDF_UPLOAD_BYTES`, well within
    /// what a real request body limit would allow through, and still
    /// decompresses past `MAX_TOTAL_INFLATED_BYTES`.
    #[test]
    fn a_highly_compressed_bomb_stream_is_rejected() {
        let bomb_plaintext = vec![0u8; MAX_TOTAL_INFLATED_BYTES + 1024 * 1024];
        let compressed = zlib_compress(&bomb_plaintext);
        // Ratio-based, not an absolute byte count: at this fixture's size
        // (257 MiB of zeros) DEFLATE's own match-length cap means the
        // compressed form is a couple of MiB, not the "well under 1 MiB" an
        // earlier version of this check assumed -- still a >100x ratio, and
        // still the point of this sanity check: prove the fixture really is
        // a small upload that decompresses to something enormous, not
        // (accidentally) a large upload to begin with.
        assert!(
            compressed.len() < bomb_plaintext.len() / 20,
            "fixture sanity check: an all-zero buffer must compress to a small fraction of its \
             original size, got {} bytes compressed from {} bytes",
            compressed.len(),
            bomb_plaintext.len()
        );
        let pdf_like = wrap_stream(&compressed);

        let result = reject_pdf_compression_bombs(&pdf_like);
        assert!(
            result.is_err(),
            "a stream that decompresses past MAX_TOTAL_INFLATED_BYTES must be rejected before \
             pdf_extract/lopdf ever sees it"
        );
    }

    /// The guard's own memory discipline: `bounded_inflate_len` must not
    /// have materialized the bomb's full output to detect it -- it should
    /// have stopped reading from the decoder as soon as the budget was
    /// exceeded. Proven indirectly: this test's bomb decompresses to
    /// several times `MAX_TOTAL_INFLATED_BYTES`, and the whole test
    /// (compress + scan) still completes quickly, with no attempt to
    /// `Vec`-allocate anything close to the bomb's true decompressed size.
    #[test]
    fn the_bounded_inflate_helper_stops_reading_once_the_budget_is_exceeded() {
        let bomb_plaintext = vec![b'x'; 32 * 1024 * 1024];
        let compressed = zlib_compress(&bomb_plaintext);
        let produced = bounded_inflate_len(&compressed, 1024);
        assert!(
            produced > 1024,
            "must have exceeded the tiny budget (proving it actually decoded real data)"
        );
        assert!(
            produced < bomb_plaintext.len(),
            "must have stopped well short of the bomb's true, much larger decompressed size"
        );
    }

    /// Multiple individually-small streams whose DECOMPRESSED sizes sum
    /// past the budget must still be rejected -- the running total is
    /// cumulative across every stream in the file, not reset per-stream
    /// (a PDF with many moderate content streams, e.g. one per page of a
    /// many-page document, is a realistic non-malicious shape this must
    /// still bound the total memory impact of).
    #[test]
    fn the_budget_is_cumulative_across_multiple_streams_not_reset_per_stream() {
        let each = MAX_TOTAL_INFLATED_BYTES / 2 + 1024 * 1024;
        let compressed = zlib_compress(&vec![0u8; each]);

        let mut pdf_like = wrap_stream(&compressed);
        pdf_like.extend_from_slice(b"\n");
        pdf_like.extend_from_slice(&wrap_stream(&compressed));

        assert!(
            reject_pdf_compression_bombs(&pdf_like).is_err(),
            "two streams each just over half the budget must still be rejected on their combined \
             total, even though neither alone exceeds it"
        );
    }

    /// A span between `stream`/`endstream` that isn't valid zlib data
    /// (most real PDF streams aren't -- images, fonts, uncompressed
    /// content) must not be treated as an error; it simply contributes no
    /// bytes and the scan moves on.
    #[test]
    fn a_non_zlib_stream_span_is_skipped_without_error() {
        let pdf_like = wrap_stream(b"this is not zlib data at all, just raw bytes");
        assert!(reject_pdf_compression_bombs(&pdf_like).is_ok());
    }
}
