# Ticket Processing Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three self-contained extraction improvements, all inside
`crates/api/src/data/ticket_extraction.rs`: (1) actually populate
`.pkpass` `ticket_type` from `boardingPass.auxiliaryFields` instead of
hardcoding `None`; (2) add an anchored `Out:`/`Ret:` route pattern for
OTRL-issued PDF e-tickets, tried before the existing generic `"X to Y"`
pattern, plus one new `TICKET_TYPE_KEYWORDS` literal ("Super Off-Peak
Return"); (3) read and `tracing::debug!`-log a `.pkpass`'s barcode
*format* string (e.g. `"PKBarcodeFormatAztec"`) as a diagnostic signal
only — never the barcode `message` payload, never returned in
`PartialTicket`, never persisted.

**Architecture:** No new files, modules, routes, dependencies, or
migrations. All three changes live inside the same file's existing
`parse_pass_json`/`parse_pdf_text` functions and their sibling
`#[cfg(test)]` modules, following that module's own established
inline-fixture convention (`serde_json::json!()` for `.pkpass`, literal
string fixtures for PDF text).

**Tech Stack:** Rust (existing `crates/api` workspace conventions) —
`serde_json::Value` traversal, `regex`/`std::sync::LazyLock` (already used
in this file for `ROUTE_PATTERN`), `tracing::debug!` (already a `crates/api`
dependency, used fully-qualified elsewhere in `src/data/*.rs` with no `use`
import needed).

**Spec:** `docs/superpowers/specs/2026-09-02-ticket-processing-improvements-design.md`
— read in full before starting; this plan carries its three Decisions into
concrete tasks and does not restate its research. Cross-references below
to "Decision N" refer to that document.

**Status note — every `crates/api/src/data/ticket_extraction.rs` citation
below independently re-read against this worktree's current file (442
lines total), not trusted blind from the spec:**

- **No drift found.** `git log --oneline -- crates/api/src/data/ticket_extraction.rs`
  shows the spec's own commit (`3bea033`, "Add design spec: ticket file
  processing improvements...") sits *after* the latest commit that touched
  this file (`8e268d7`, a repo-wide `cargo fmt` pass with no logic
  changes) — nothing has edited `ticket_extraction.rs` since the spec's
  citations were written. Every line number the spec cites was
  independently re-confirmed exactly against the current file during this
  planning pass: `PartialTicket` struct 19-34, `parse_pass_json` 49-83
  (`ticket_type: None` at line 78 exactly, `organizationName` read at
  62-65), `semantics_origin_destination` 85-93, `primary_fields_origin_destination`
  95-122 (doc comment 95-99), `parse_pkpass` 139-153, `MAX_ENTRY_BYTES` at
  line 130, `parse_pdf_text` 298-327 (the `ROUTE_PATTERN.captures()` call
  site at 304-312 exactly), `KNOWN_RETAILER_MARKERS` at line 332,
  `TICKET_TYPE_KEYWORDS` 334-341 (six literal strings, confirmed none is or
  contains "Super Off-Peak Return"), `ROUTE_PATTERN` 343-363 (regex literal
  at line 361 matches the spec's quote exactly), `parse_pdf` 421-432,
  `ticket_type_is_never_guessed_at` test at 224-228. The other cited files
  (`crates/api/src/routes/train.rs`'s upload handlers, `crates/api/src/data/train_tracking.rs`'s
  `validate_ticket_entry`/`TICKET_SOURCES`, `frontend/components/TicketEntryForm.tsx`'s
  `applyPreview`, the `journey_ticket_tracking.sql` migration header) were
  spot-checked this session too and match the spec's descriptions; none of
  them is modified by this plan (see Global Constraints).

## Global Constraints

- **Every task in this plan touches the same single file**,
  `crates/api/src/data/ticket_extraction.rs` — run Tasks 1, 2, and 3
  **serially, each with its own commit**, not dispatched to parallel
  subagents. A parallel edit here would produce a merge conflict on every
  task, not just some.
- **No new file, module, route, dependency, or migration anywhere in this
  plan.** All three decisions are additive changes inside one existing
  file's existing functions/test modules.
- **`PartialTicket` gains no new field.** Decision 3's barcode-format
  signal is a `tracing::debug!` log line only — it must never be added to
  the `PartialTicket` struct (crates/api/src/data/ticket_extraction.rs:19-34),
  which is the literal JSON response body for both upload routes
  (`crates/api/src/routes/train.rs`'s `handle_pkpass_upload`/`handle_pdf_upload`).
  No task in this plan touches `train.rs` or `TicketEntryForm.tsx`.
- **Never read `barcode`/`barcodes[].message` anywhere in this plan** — only
  `.format`. `message` is the barcode payload (RSP-6, per the companion
  research doc); reading it in any form, including a truncated prefix or
  its byte length, is explicitly out of scope (Decision 3's "Alternatives
  weighed"). No task below decodes, renders, stores, or logs anything
  derived from a barcode payload.
- **No test fixture may reproduce the two real gitignored example
  tickets' actual data.** Every new test fixture in this plan is
  hand-written and structurally representative only (real key names, real
  label conventions, real line/field shapes) — invented values throughout,
  following `semantics_present_is_preferred_and_labelled_accordingly`'s
  existing precedent (`"LNER"` / `"Kings Cross"` / `"Edinburgh"`, none of
  it real). The barcode fixture's `message` value in Task 3 must be an
  obvious placeholder string (e.g. `"PLACEHOLDER-NOT-A-REAL-PAYLOAD"`),
  never anything shaped like a real ~233-character RSP-6 payload.
- **No fallback to `secondaryFields` or `backFields`, no additional
  `TICKET_TYPE_KEYWORDS` beyond "Super Off-Peak Return", no PDF-side
  barcode detection.** All explicitly out of scope per the spec's own
  "Explicitly out of scope" section — do not add any of these while
  implementing the tasks below, even if it looks like a one-line
  extension.

---

## File structure

```
crates/api/src/data/ticket_extraction.rs   MODIFY -- all three tasks
  + fn keyed_field_value(fields, key)                       Task 1
  parse_pass_json: ticket_type <- keyed_field_value(...)     Task 1
  parse_pass_json doc comment: +1 sentence on ticket_type    Task 1
  pass_json_tests: +2 tests                                  Task 1

  + fn extract_route(text) -> (Option<String>, Option<String>)  Task 2
  + static ROUTE_PATTERNS: [Regex; 2] (replaces ROUTE_PATTERN)  Task 2
  parse_pdf_text: origin/destination <- extract_route(text)     Task 2
  TICKET_TYPE_KEYWORDS: + "Super Off-Peak Return"                Task 2
  parse_pdf_text_tests: +3 tests                                  Task 2

  + fn barcode_format(pass) -> Option<String>                 Task 3
  parse_pass_json: + tracing::debug!(barcode_format = ?..., "parsed .pkpass")  Task 3
  + mod barcode_format_tests: 3 tests                          Task 3
```

---

### Task 1: `.pkpass` `ticket_type` read from `auxiliaryFields` by key (Decision 1)

**Files:**
- Modify: `crates/api/src/data/ticket_extraction.rs`

**Interfaces:**
- Produces: `fn keyed_field_value(fields: &serde_json::Value, key: &str) -> Option<String>` — a general key-based lookup over a `{key, label, value}`-shaped JSON array, usable wherever a later task needs the same pattern (none currently does, but it is not private to `parse_pass_json`'s call site).
- Consumes: nothing new — reads only `boardingPass.auxiliaryFields`, part of the `serde_json::Value` `parse_pass_json` already receives whole.
- **Depends on:** nothing — first task, independent of Tasks 2 and 3.

- [ ] **Step 1: Write the failing tests**

In `crates/api/src/data/ticket_extraction.rs`, inside the existing
`pass_json_tests` module (currently lines 156-229), add two new tests
directly above the existing `ticket_type_is_never_guessed_at` test (which
stays unchanged — it is still a correct regression test after this
change, proving `ticket_type` stays `None` when `auxiliaryFields` is
absent entirely):

```rust
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
        assert_eq!(ticket.ticket_type, Some("Super Off-Peak Return".to_string()));
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api ticket_type_is_read_from_auxiliary_fields_by_key ticket_type_ignores_a_field_with_the_wrong_key`
Expected: both FAIL — `ticket_type` is `None` in both cases today because
`parse_pass_json` hardcodes it (line 78), so the first assertion
(`Some("Super Off-Peak Return".to_string())`) fails; the second happens to
already pass by coincidence (still `None`, but for the wrong reason — no
`auxiliaryFields` read exists at all yet). Confirm this by inspection of
the failure output, not just the exit code — the second test passing
"for free" before the implementation exists is expected and not a bug.

- [ ] **Step 3: Add `keyed_field_value` and wire it into `parse_pass_json`**

Add the new function directly below `primary_fields_origin_destination`
(which currently ends at line 122), before the `use std::io::Read;` line:

```rust
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
```

Then, in `parse_pass_json`, replace the hardcoded field at line 78:

```rust
    Ok(PartialTicket {
        operator,
        ticket_type: None,
        origin_crs: origin,
        destination_crs: destination,
        source,
    })
```

with:

```rust
    let ticket_type = boarding_pass
        .get("auxiliaryFields")
        .and_then(|fields| keyed_field_value(fields, "ticketType"));

    Ok(PartialTicket {
        operator,
        ticket_type,
        origin_crs: origin,
        destination_crs: destination,
        source,
    })
```

- [ ] **Step 4: Update `parse_pass_json`'s doc comment**

`parse_pass_json`'s doc comment (currently lines 36-48) describes the
origin/destination split but says nothing about `ticket_type`. Add one
sentence at the end of the existing doc comment, directly above `pub fn
parse_pass_json`:

```rust
/// `ticket_type` is read from `boardingPass.auxiliaryFields` by exact
/// `key` match (`"ticketType"`) via `keyed_field_value` -- `None` if that
/// key isn't present, never guessed from label text or other fields.
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p api ticket_type`
Expected: PASS — this matches `ticket_type_is_read_from_auxiliary_fields_by_key`,
`ticket_type_ignores_a_field_with_the_wrong_key`, and the existing
`ticket_type_is_never_guessed_at`, all three green. Then run the full
module's suite to confirm nothing else regressed: `cargo test -p api
ticket_extraction`.
Expected: all tests in the file PASS, including the unrelated
`semantics_present_is_preferred_and_labelled_accordingly` and
`semantics_absent_falls_back_to_the_two_field_primary_fields_heuristic`
tests (neither sets `auxiliaryFields`, so both still assert `ticket_type`
implicitly stays out of scope of their own assertions — confirm no
existing test asserts `ticket_type == None` in a way this change would
break; `ticket_type_is_never_guessed_at` is the only one that does, and it
still passes because its fixture has no `auxiliaryFields` at all).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/ticket_extraction.rs
git commit -m "Read .pkpass ticket_type from boardingPass.auxiliaryFields by key"
```

---

### Task 2: OTRL PDF route heuristic — ordered `extract_route` chain (Decision 2)

**Files:**
- Modify: `crates/api/src/data/ticket_extraction.rs`

**Interfaces:**
- Produces: `fn extract_route(text: &str) -> (Option<String>, Option<String>)`,
  replacing the current inline `ROUTE_PATTERN.captures(text)` call
  (crates/api/src/data/ticket_extraction.rs:304-312) at its one call site
  inside `parse_pdf_text`. `static ROUTE_PATTERNS: LazyLock<[regex::Regex; 2]>`
  replaces the current `static ROUTE_PATTERN: LazyLock<regex::Regex>`
  (lines 360-363) — nothing outside this file references either name, so
  this rename has no other call sites to update.
- Consumes: nothing new.
- **Depends on:** nothing — independent of Tasks 1 and 3, but see Global
  Constraints on running these serially (same file).

- [ ] **Step 1: Write the failing tests**

In `crates/api/src/data/ticket_extraction.rs`, inside the existing
`parse_pdf_text_tests` module (currently lines 365-411), add three new
tests after the existing `a_route_with_nothing_after_the_destination_still_matches`
test, before the module's closing brace:

```rust
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
        assert_eq!(ticket.ticket_type, Some("Super Off-Peak Return".to_string()));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api otrl_out_ret_line_is_matched otrl_pattern_is_preferred ticket_type_matches_the_super_off_peak_return_keyword`
Expected: `otrl_out_ret_line_is_matched_when_the_generic_to_pattern_fails`
and `otrl_pattern_is_preferred_over_a_coincidental_to_match_earlier_in_the_text`
FAIL (`origin_crs`/`destination_crs` come back `None` — today's single
`\s+to\s+` pattern never matches either fixture, since neither contains
the literal word "to" in route position). `ticket_type_matches_the_super_off_peak_return_keyword`
FAILS too — "Super Off-Peak Return" isn't in `TICKET_TYPE_KEYWORDS` yet.

- [ ] **Step 3: Replace `ROUTE_PATTERN` with the ordered `ROUTE_PATTERNS` chain and `extract_route`**

Replace the existing `static ROUTE_PATTERN` block (currently lines
343-363, doc comment included) with:

```rust
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
        regex::Regex::new(r"(?:Out|Ret):\s*([A-Z]{3})\s*[-\u{2010}-\u{2015}]\s*([A-Z]{3})").unwrap(),
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
        regex::Regex::new(r"([A-Za-z][A-Za-z '\-]+?)\s+to\s+([A-Za-z][A-Za-z '\-]+?)(?:[,\.\n]|$)").unwrap(),
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
```

Then, in `parse_pdf_text` (currently lines 298-327), replace the inline
call site (currently lines 304-312):

```rust
    let (origin, destination) = ROUTE_PATTERN
        .captures(text)
        .map(|caps| {
            (
                Some(caps[1].trim().to_string()),
                Some(caps[2].trim().to_string()),
            )
        })
        .unwrap_or((None, None));
```

with:

```rust
    let (origin, destination) = extract_route(text);
```

- [ ] **Step 4: Add the `TICKET_TYPE_KEYWORDS` literal**

In the existing `TICKET_TYPE_KEYWORDS` list (currently lines 334-341), add
one new literal (sibling variants like "Super Off-Peak Single" are
explicitly out of scope — see Global Constraints):

```rust
const TICKET_TYPE_KEYWORDS: &[&str] = &[
    "Anytime Day Single",
    "Off-Peak Day Single",
    "Off-Peak Day Return",
    "Advance Single",
    "Season",
    "Open Return",
    "Super Off-Peak Return",
];
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p api ticket_extraction`
Expected: all tests in the file PASS — the three new tests from Step 1,
and every pre-existing test in `parse_pdf_text_tests`
(`matches_the_design_docs_own_worked_example`,
`an_unrecognized_retailer_yields_no_operator_guess`,
`text_with_no_route_pattern_match_yields_no_stations`,
`no_ticket_type_keyword_present_yields_none_not_a_guess`,
`a_route_with_nothing_after_the_destination_still_matches`), confirming
the generic pattern's existing behavior is preserved as a fallback, not
replaced, and that adding "Super Off-Peak Return" doesn't change any
existing keyword match.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/ticket_extraction.rs
git commit -m "Add anchored OTRL Out:/Ret: PDF route pattern, tried before the generic 'to' pattern"
```

---

### Task 3: `.pkpass` barcode-format detection — diagnostic log only (Decision 3)

**Files:**
- Modify: `crates/api/src/data/ticket_extraction.rs`

**Interfaces:**
- Produces: `fn barcode_format(pass: &serde_json::Value) -> Option<String>`
  — a pure function, unit-tested directly (no assertion on log output
  anywhere in this task, matching this module's existing "thin wrapper vs.
  pure logic" testing split). **Not** added to `PartialTicket`; **not**
  returned from `parse_pass_json`; **not** persisted anywhere.
- Consumes: nothing new — reads only `pass.barcode`/`pass.barcodes`, part
  of the `serde_json::Value` `parse_pass_json` already receives whole.
- **Depends on:** nothing structurally, but implement after Task 1 (same
  function, `parse_pass_json`, gets a second edit) to avoid two tasks
  racing on the same few lines — see Global Constraints on serial
  same-file execution.

- [ ] **Step 1: Write the failing tests**

In `crates/api/src/data/ticket_extraction.rs`, add a new
`#[cfg(test)] mod barcode_format_tests` directly below the existing
`pass_json_tests` module (which now ends after Task 1's additions,
currently just after line 229 pre-Task-1, adjust for Task 1's two added
tests):

```rust
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
        assert_eq!(barcode_format(&pass), Some("PKBarcodeFormatAztec".to_string()));
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api barcode_format`
Expected: FAIL to compile — `barcode_format` doesn't exist yet.

- [ ] **Step 3: Add `barcode_format` and call it from `parse_pass_json`**

Add the new function directly below `keyed_field_value` (added in Task
1, immediately below `primary_fields_origin_destination`):

```rust
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
        .or_else(|| pass.get("barcodes").and_then(|b| b.as_array()).and_then(|a| a.first()))
        .and_then(|b| b.get("format"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}
```

Then, in `parse_pass_json`, add a diagnostic log call. Insert it directly
before the function's final `Ok(PartialTicket { ... })` (the same
`Ok(PartialTicket { ... })` Task 1 already edited to compute `ticket_type`
above it):

```rust
    // Diagnostic only -- never surfaced in PartialTicket, the frontend, or
    // any persisted row. debug-level specifically so it costs nothing in
    // default-configured production logging and cannot become a de facto
    // data-collection channel without a deliberate decision to promote it.
    // See Decision 3 of
    // docs/superpowers/specs/2026-09-02-ticket-processing-improvements-design.md.
    tracing::debug!(barcode_format = ?barcode_format(pass), "parsed .pkpass");
```

(`pass` is already in scope as `parse_pass_json`'s own parameter — no new
binding needed.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api ticket_extraction`
Expected: all tests in the file PASS, including the three new
`barcode_format_tests` and every test from Tasks 1 and 2.

- [ ] **Step 5: Full crate check**

Run: `cargo test -p api && cargo clippy -p api`
Expected: PASS clean — confirms this task's edits (and the accumulated
edits from Tasks 1-2) don't produce warnings elsewhere in the crate (e.g.
an unused-import or dead-code lint from the `ROUTE_PATTERN` -> `ROUTE_PATTERNS`
rename in Task 2).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/ticket_extraction.rs
git commit -m "Log .pkpass barcode format (never payload) as a diagnostic signal"
```

---

## Explicitly out of scope (restated from the spec — do not implement)

- Decoding any barcode payload (RSP-6 or otherwise), or rendering/displaying
  anything derived from a decoded payload. Shelved pending a legal decision
  the repo owner has not made — see the spec's "Explicitly out of scope"
  section in full.
- `.pkpass` `ticket_type` fallback to `secondaryFields` — only
  `auxiliaryFields` is confirmed against a real file.
- Ticket-type keyword variants beyond "Super Off-Peak Return" (e.g. "Super
  Off-Peak Single", "Super Off-Peak Day Return") — unconfirmed without
  more real samples.
- A CRS-code data-quality warning in `TicketEntryForm.tsx` — a frontend
  change, not an extraction change, scoped out by the spec separately.
- OCR over PDF logo/branding images to recover `operator`.
- PDF-side barcode presence/format detection — no cheap metadata field
  exists for a vector-drawn barcode; would require rasterization plus an
  image-based symbology scan, a materially larger lift than the `.pkpass`
  case for a diagnostic-only signal.
- Changing `validate_ticket_entry`'s CRS-format check to validate against
  a real station/CRS list.
- Any change to `backFields` handling.
