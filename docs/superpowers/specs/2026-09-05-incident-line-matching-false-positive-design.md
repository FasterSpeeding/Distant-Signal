# Design: Fix Cross-Operator OperatorOnly Deletion and Uncorroborated Keyword False Positives in `lines_affected_by`

**Status: design proposal, not approved. No implementation in this document.**

## Bug being fixed (confirmed live, 2026-09-04/05)

National Rail Enquiries incident `7F69B9D781A941AD8305FECCE3ACAA43`
("Disruption between New Malden and Raynes Park") has exactly one real
`affectedOperators` entry, South Western Railway (`SW`). On this app's
`/incidents/{id}` page it is displayed as affecting **CrossCountry (XC)
instead**, with all four South Western lines' correct attribution deleted
outright, not merely outranked. This document designs the fix; it does not
implement it.

## Root cause, verified directly against this worktree

All line numbers below were re-read from
`crates/aggregator/src/matcher.rs` in this worktree on 2026-09-05 and match
the prior research pass exactly.

`lines_affected_by` (`matcher.rs:39-65`) builds a lowercased
`summary`+`description` haystack (line 44) and calls `match_one` once per
catalogue line. `match_one` (`matcher.rs:67-156`) tries three tiers in
order and returns the first that hits:

- **Tier 1**, `matcher.rs:92-125`: station hits against
  `incident.affected_stations`, classified via `SegmentRegistry` into
  `ExclusiveSegment` / `SharedSegment` / `StationHit`.
- **Tier 2**, `matcher.rs:127-139`: substring match of the line's own
  `match_keywords` against the haystack, scope `KeywordOnly`. Notably,
  `operator_overlap` (computed at `matcher.rs:73-78`, `line.operators ∩
  incident.operators`) is already threaded into this match's `Evidence`
  (line 135) but is **not used to gate acceptance** — any keyword substring
  hit is accepted unconditionally, however weak.
- **Tier 3**, `matcher.rs:141-153`: non-empty `operator_overlap` alone,
  scope `OperatorOnly`.

The incident's description contains a real, structurally unrelated
sentence — "your ticket is also valid on the following rail replacement
services... CrossCountry services between Reading and Bournemouth" — a
ticket-acceptance clause, not the actual disruption. `lines/cross-country.toml:9`'s
`match_keywords = ["CrossCountry", "Cross Country"]` substring-matches
this, producing a spurious Tier-2 `KeywordOnly` match for `cross-country`.
Every South Western line (`swr-south-west-main`, `swr-kingston-loop`,
`swr-chessington`, `swr-portsmouth-direct`, and also `swr-alton`, all
`operators = ["SW"]`) correctly gets a Tier-3 `OperatorOnly` match, since
`SW` is genuinely in `incident.operators`.

The actual bug is `lines_affected_by`'s post-processing, `matcher.rs:56-62`:

```rust
// If any precise match exists, drop operator-only matches -- they're
// almost certainly false positives where another line on the same
// operator is the actual target.
let has_precise = out.iter().any(|m| m.scope != MatchScope::OperatorOnly);
if has_precise {
    out.retain(|m| m.scope != MatchScope::OperatorOnly);
}
```

**The comment already states the intended semantics — "another line on
the *same operator*" — but the code checks nothing about operator
identity.** `has_precise` is computed over the entire incident's matches
across every line and every operator; if *any* line anywhere gets a
non-`OperatorOnly` match, *every* `OperatorOnly` match for *every* line on
*every* operator is stripped. The spurious XC `KeywordOnly` match
satisfies "a non-`OperatorOnly` match exists somewhere," which deletes all
four correct SW `OperatorOnly` matches. End state: only the spurious XC
line survives to reach `/incidents/{id}`.

This is a genuine, two-part bug:

1. The retain is unscoped by operator (deletes SW's real matches).
2. Tier 2 has no floor at all against contradicting structured operator
   data (creates XC's fake match in the first place).

Fixing only (1) stops SW's data from being deleted, but XC would still
display alongside SW as a second, fabricated "affected line." Fixing only
(2) happens to fully resolve *this specific* incident (see Decision 2's
worked trace below) but leaves the unscoped, cross-operator deletion bug
in place for any other incident shape that triggers it. Both are needed.

## Decision 1: Rescope the `OperatorOnly` retain to be per-operator

Replace the global `has_precise`/`retain` pair with a per-operator version
that matches the comment's own stated intent literally:

```rust
// Operators for which some line got a more-precise-than-OperatorOnly
// match anywhere in this incident.
let mut precise_operators: HashSet<&str> = HashSet::new();
for m in &out {
    if m.scope != MatchScope::OperatorOnly {
        precise_operators.extend(m.line.operators.iter().map(String::as_str));
    }
}
out.retain(|m| {
    m.scope != MatchScope::OperatorOnly
        || !m.line.operators.iter().any(|op| precise_operators.contains(op.as_str()))
});
```

An `OperatorOnly` match is dropped only when *some other line sharing at
least one of the same operator codes* has a more precise match — not when
any unrelated operator's line does anywhere in the incident.

**Verified this preserves the intended within-operator behavior exactly,
not just avoids the cross-operator bug.** Every South Western file
(`swr-south-west-main.toml`, `swr-kingston-loop.toml`,
`swr-chessington.toml`, `swr-portsmouth-direct.toml`, `swr-alton.toml`)
declares `operators = ["SW"]` (confirmed by direct grep of all five files).
If a hypothetical SW incident's text legitimately named a specific SW
route keyword (e.g. "Portsmouth Direct") while also being a genuine
system-wide SW disruption, today's code — and this rescoped version — both
still drop the *other* SW lines' `OperatorOnly` matches in favor of the
named one, per the comment's original stated rationale. Decision 1 changes
nothing about same-operator suppression; it only removes the accidental
cross-operator interaction. This should be called out explicitly in the
rescoped code's comment when implemented, since the original comment's
wording is otherwise easy to reintroduce the same bug against.

Applied to the real incident: `precise_operators` would contain `{XC}`
only (from the spurious keyword match) even before Decision 2 fires. SW's
four `OperatorOnly` matches check membership of `SW` in `{XC}` — false —
so they survive. **Decision 1 alone already stops the data-deletion half
of the bug.** It does not, by itself, stop XC from cosmetically appearing
as a second (fabricated) affected line alongside the correct SW entries;
that is what Decision 2 is for.

## Decision 2: Let `incident.operators`, when non-empty, contradict Tier-2 keyword hits

**Do not** implement the "obvious" symmetric fix — extending the
existing precise-match-demotes-weaker-match retain logic to *also* demote
`KeywordOnly` whenever a more-precise match exists elsewhere in the same
incident. That was flagged as the wrong move by the prior research pass,
and re-deriving it here confirms why: `lines/cross-country.toml:10-12`'s
own comment documents that "Cross Country Route" (the real
Birmingham-Bristol infrastructure name) must keep matching via keyword
alone, and a blanket "demote if anything more precise exists anywhere in
the incident" rule would wrongly suppress a legitimate Hull-Trains-only or
Grand-Central-only incident if some unrelated line elsewhere in the same
text happened to get a precise (station-based) match. That failure mode is
orthogonal to operator identity, so rescoping it per-operator (Decision
1's fix) wouldn't save it either — the demotion would still be
between unrelated operators' matches, on purpose, in that hypothetical.
The right lever is not "was something else more precise," it's "does the
incident's *own* structured operator data agree with this specific
keyword hit."

**Verified the corroboration data actually exists for every flagged
brand-only line**, by reading each TOML file directly in this worktree:

| Line file | `operators` | bare-brand `match_keywords` |
|---|---|---|
| `cross-country.toml` | `["XC"]` | `"CrossCountry"`, `"Cross Country"` |
| `grand-central.toml` | `["GC"]` | `"Grand Central"` |
| `grand-central-bradford.toml` | `["GC"]` | `"Grand Central"` |
| `hull-trains.toml` | `["HT"]` | `"Hull Trains"` |
| `lumo.toml` | `["LD"]` | `"Lumo"` |
| `heathrow-express.toml` | `["HX"]` | `"Heathrow Express"` |
| `lner-ecml.toml` | `["GR"]` | `"LNER"` (also `"ECML"`, `"East Coast Main Line"`) |
| `northern.toml` | `["NT"]` | `"Northern Rail"`, `"Northern Trains"`, `"Northern services"` (already qualified, not bare "Northern") |
| `southern-brighton-main-line.toml` | `["SN", "GX"]` | `"Brighton Main Line"`, `"Gatwick Express"` |
| `elizabeth-line.toml` | `["XR"]` | `"Elizabeth line"`, `"Crossrail"` |

Every one of these lines' own `operators` field already contains the ATOC
code for the exact brand its bare keyword names. `incident.operators` is
documented at `crates/common/src/lib.rs:557` as "ATOC codes, flattened
from `Affects.Operators.AffectedOperator[].OperatorRef`" — i.e. NRE's own
structured claim about which operators an incident affects. So a genuine
Hull-Trains incident should, by construction of NRE's own schema, carry
`HT` in `incident.operators`, which already makes `operator_overlap`
non-empty for `hull-trains.toml`'s Tier-2 hit via the code that already
exists today (`matcher.rs:73-78`, thread into `Evidence` at line 135) —
this signal is computed and simply discarded. This directly answers the
task's crux question: **the data needed for a safe fix already exists
without any new plumbing.**

**Chosen rule — negative/contradiction gating, not positive
corroboration-required gating.** Two shapes were considered:

- **(Rejected) Require corroboration to accept**: only accept a Tier-2
  keyword hit if `operator_overlap` is non-empty. This is stricter than
  necessary and carries a real, unverifiable-from-this-repo risk: it's not
  established that NRE's `Affects.Operators` list is always populated for
  every genuinely-affected operator on every incident, particularly
  infrastructure-caused incidents (a track/signal fault might plausibly be
  tagged sparsely, or not tag every downstream TOC that happens to run
  over the affected track). If that's ever true, requiring positive
  corroboration would silently convert real Cross-Country-Route incidents
  with an incomplete `operators` list into false negatives — trading one
  bug class for a worse, quieter one. This repo has no incident-feed
  sample data checked in to verify or refute that risk (see Open
  Questions).
- **(Chosen) Reject only on positive contradiction**: only suppress a
  Tier-2 keyword hit when `incident.operators` is **non-empty** (NRE
  positively asserted a specific, closed set of affected operators) *and*
  this line's own operator is **not** in that set. When
  `incident.operators` is empty (no structured claim at all — the status
  quo for the risk case above, or simply an incident where NRE didn't
  supply an operators list at all), Tier 2 behaves exactly as it does
  today: keyword hit alone is accepted, unconditionally.

  ```rust
  // Tier 2: keyword match, unless the incident's own structured operator
  // list positively excludes this line's operator.
  if !keyword_hits.is_empty() {
      let contradicted =
          !incident.operators.is_empty() && operator_overlap.is_empty();
      if !contradicted {
          return Some(Match {
              line,
              scope: MatchScope::KeywordOnly,
              evidence: Evidence {
                  stations: vec![],
                  segments: vec![],
                  operators: operator_overlap,
                  keywords: keyword_hits,
              },
          });
      }
      // else: fall through. Tier 3 also requires non-empty
      // operator_overlap, which is false here by construction of
      // `contradicted`, so match_one naturally returns None below --
      // no special-cased early return needed.
  }
  ```

  Traced against the real incident: `incident.operators = ["SW"]`
  (non-empty), `cross-country.toml`'s `operator_overlap` is `[]` (`XC` not
  in `["SW"]`) → `contradicted = true` → the XC keyword hit is dropped,
  and Tier 3 also can't fire for `cross-country` (same empty overlap), so
  `match_one` returns `None` for the `cross-country` line entirely. XC
  never appears in `out` at all — not merely down-ranked. Combined with
  Decision 1 (which is still needed independently, see below), the
  incident now correctly shows only the four SW lines, all as
  `OperatorOnly`, and nothing else.

  Note this rule is **generic over the whole catalogue**, not a hardcoded
  allow/deny-list keyed on the ~8-10 flagged "risky" line ids. It falls
  out of data every line already declares (`operators`), so it also
  protects the un-flagged bulk of the catalogue for free (e.g. it would
  equally catch a spurious "Gatwick Express" mention in an incident whose
  structured operators list excludes both `SN` and `GX`) without needing
  to maintain a bespoke list as the catalogue grows.

  **This also does not regress the case the rule exists to protect.**
  Traced against `cross-country.toml:10-12`'s own documented case: a
  genuine "Cross Country Route" infrastructure incident that also lists
  `XC` in `incident.operators` (expected, since NRE's operators field is
  specifically "which TOCs are affected", not "which entity caused it") —
  `operator_overlap` is non-empty, `contradicted` is false, keyword match
  accepted exactly as today.

**Existing tests already validate the `incident.operators = []` fallback
path this rule preserves**, without needing new coverage for it:
`keyword_only_match` (`matcher.rs:242-255`, WCML, `operators: &[]`) and
`grand_central_unrelated_birmingham_mention_does_not_veto_match`
(`matcher.rs:2044-2076`, Grand Central, `operators: &[]`, asserts
`MatchScope::KeywordOnly`) both pass an empty operators slice and would be
unaffected by this rule (`contradicted` is always false when
`incident.operators` is empty). Read both directly to confirm; neither
needs modification.

## Decision 3: No new `MatchScope` variant

The task asked whether the fix needs a new variant to distinguish
"keyword hit corroborated by structured operator data" from "keyword hit
alone," for downstream reasoning/testability. **Recommendation: do not add
one**, for this fix. Reasoning:

- Decision 2's rule is a **gate on acceptance** ("was this Tier-2 hit
  contradicted, yes/no"), not a **confidence gradient** that needs to
  survive past `match_one` for later stages to reason about. Once a hit
  clears the gate, nothing downstream currently needs to know whether it
  was corroborated or merely un-contradicted — both are "keyword evidence
  the structured data doesn't rule out," which is exactly what
  `MatchScope::KeywordOnly` already means.
- Adding a variant has real blast radius for essentially no behavior
  change: `crates/aggregator/src/aggregation.rs`'s `demote_for_scope`
  (`aggregation.rs:284-293`) is an **exhaustive** match on `MatchScope`
  (no wildcard arm) — a new variant is a compile-time-forcing change
  there, and the `reason` text match (`aggregation.rs:132-136`, which
  *does* have a `_ => {}` wildcard) would silently give the new variant no
  special reason text unless someone remembers to add it. Neither cost
  buys anything under Decision 2 as designed.
- If a future need arises to expose "was this a strong, operator-backed
  keyword match" in the UI (e.g. a confidence badge), that's a clean,
  additive follow-up: thread `Evidence.operators.is_empty()` into the
  existing `Match` a caller already holds, no new enum variant required
  ­— `Evidence.operators` already carries exactly this bit today.

This keeps the fix to the two behavioral changes (Decisions 1 and 2) with
no enum surface change, no new match arms elsewhere in the codebase to
audit, and no serialization/frontend contract to check (confirmed by grep:
`MatchScope` is referenced only in `matcher.rs`, `aggregation.rs`, and one
comment in `segments.rs` — it is not serialized or exposed to the
frontend).

## Decision 4: Tests

**Confirmed test-coverage gap.** `crates/aggregator/src/matcher.rs`'s test
module (`mod tests`, line 165 onward, 247 `#[test]` functions total) has
**zero** assertions on `MatchScope::OperatorOnly` anywhere, and the only
two assertions on `MatchScope::KeywordOnly` (`keyword_only_match`,
`matcher.rs:254`; `grand_central_unrelated_birmingham_mention_does_not_veto_match`,
`matcher.rs:2075`) both use an empty `operators` slice. **No existing test
exercises the cross-operator retain interaction (`matcher.rs:56-62`) at
all** — this exact bug class was previously untested, not merely missed by
one test.

Tests to add, all in `crates/aggregator/src/matcher.rs`'s existing `mod
tests`, using the existing `incident()`/`load_line()`/`load_all_lines()`
helpers (`matcher.rs:169-205`):

1. **Primary regression guard, models the real incident verbatim.**
   `incident.operators = &["SW"]`, `affected_stations = &[]` (matches
   production reality per Decision-irrelevant Non-goal below), description
   including the real ticket-acceptance clause naming "CrossCountry
   services between Reading and Bournemouth" alongside genuine New
   Malden/Raynes Park disruption text. Assert: `matched_ids` does **not**
   contain `"cross-country"`, and **does** contain all of
   `swr-south-west-main`, `swr-kingston-loop`, `swr-chessington`,
   `swr-portsmouth-direct`, `swr-alton`, each with
   `scope == MatchScope::OperatorOnly`.
2. **Decision 1 in isolation, generalized (not tied to real incident
   text).** Two unrelated operators in one incident, e.g. operator A gets
   a precise (keyword) match on one of its lines while operator B's lines
   only ever get `OperatorOnly` matches (`incident.operators` containing
   both A and B's codes). Assert operator B's `OperatorOnly` matches
   survive. This is the general form of bug the real-incident test above
   only proves once.
3. **Decision 1 must not regress the intended same-operator
   suppression.** An SW incident whose text names a specific SW route
   keyword (e.g. "Portsmouth Direct") alongside a system-wide SW
   `operators` tag with no other route keywords present. Assert the
   *other* SW lines' `OperatorOnly` matches are still stripped — i.e. this
   proves Decision 1 only removed the cross-operator leak, not the
   original same-operator intent.
4. **Decision 2 in isolation.** `incident.operators = &["SW"]`, no XC
   station/route text at all, just an incidental "Grand Central" (or
   "Hull Trains") brand mention unrelated to the actual disruption. Assert
   the brand-only line does not match at all (`match_one` returns `None`
   for it — confirm via absence from `matched_ids`, not merely a demoted
   scope).
5. **Decision 2 must not regress genuine brand-only incidents.**
   `incident.operators = &["HT"]`, description genuinely about a Hull
   Trains delay. Assert `hull-trains` matches with
   `scope == MatchScope::KeywordOnly` (this is the corroborated case;
   Decision 3 keeps it the same scope as the uncorroborated case, so the
   assertion is the same regardless of corroboration — only the earlier
   "does it match at all" test needs to distinguish them).
6. **Decision 2 must not regress the `cross-country.toml`-documented
   "Cross Country Route" case.** `incident.operators = &["XC"]`,
   description mentioning "delays on the Cross Country Route between
   Birmingham and Bristol". Assert `cross-country` still matches via
   `KeywordOnly` — this is the exact scenario `cross-country.toml:10-12`'s
   comment says must keep working, verified as not broken by this fix.

Existing tests requiring no change (verify, don't touch):
`keyword_only_match` and
`grand_central_unrelated_birmingham_mention_does_not_veto_match`, both
already covering the `incident.operators = []` fallback path Decision 2
preserves unconditionally.

## Non-goal: Tier 1 (station-based matching) is confirmed always dead in production today

Re-verified directly in this worktree, 2026-09-05:
`crates/poller-incidents/src/schema.rs:106` hard-codes
`affected_stations: vec![]` when constructing every `IncidentMessage`, and
`crates/common/src/lib.rs:557`'s own field comment states why: *"left
empty by pollers — no CRS field exists in the Incidents schema, only
free-text RoutesAffected."* `crates/poller-incidents/src/schema.rs:177`
even asserts this in its own test
(`assert_eq!(message.affected_stations, Vec::<String>::new())`). This
means Tier 1 (`matcher.rs:92-125`, `ExclusiveSegment`/`SharedSegment`/
`StationHit`) never fires on any real, live incident today — only Tiers 2
and 3 (keyword and operator matching) are reachable in production, which
is exactly the tier pair this document's fix touches.

**Explicitly out of scope for this fix.** Making Tier 1 reachable would
require either (a) NRE providing a CRS-bearing field this schema doesn't
have, or (b) a free-text `RoutesAffected` station-name-extraction pass —
a materially different, larger feature (fuzzy text parsing against a
station-name gazetteer, or similar) with its own false-positive/negative
surface, not a small addition to bundle into a bug-fix PR. It is not
"cheap enough to bundle in": Decisions 1 and 2 above are self-contained,
narrowly-scoped changes to already-reachable code paths; standing up
Tier-1 station extraction is a separate, standalone piece of work with
its own design questions (which of `RoutesAffected`'s free text counts as
a station name reliably enough to trust, how ambiguous multi-word station
names are disambiguated, etc.) that deserve their own spec rather than
being decided as a side effect of this one. Tracked here only as
confirmed-still-true context, not as a task this document schedules.

## Open Questions / biggest risk

**Does NRE's `Affects.Operators.AffectedOperator[]` list reliably include
every genuinely-affected operator for infrastructure-caused incidents, or
can it be sparse/incomplete?** This is the single biggest open question,
and it is why Decision 2 was deliberately designed as *contradiction-based
suppression* rather than *corroboration-required acceptance*: the chosen
rule only ever removes a match when the structured data actively
disagrees (non-empty list, explicit exclusion), never merely because the
structured data is silent. If it turns out NRE's operators list is in
practice always complete and accurate for TOC attribution (which its
schema's stated purpose — "which operators are affected" — suggests it
should be), a stricter corroboration-required rule would be strictly
safer and simpler than the current design and could be revisited. This
repo has no sampled/archived raw incident-feed payloads checked in to
settle this empirically; verifying it would mean either (a) requesting a
batch of real historical Knowledgebase incident payloads and checking how
often `Affects.Operators` is empty/sparse on incidents that textually name
a specific brand, or (b) simply monitoring in production after this fix
ships for any Tier-2 match that stops appearing and manually checking
whether it was a real loss. Recommend the latter as a cheap, low-risk
follow-up rather than blocking this fix on the former.

A secondary, smaller open question: whether `incident.operators` and
`line.operators` ATOC codes are guaranteed case- and format-consistent
(e.g. no whitespace, no case variance) for the `.contains()` comparison
Decision 2 reuses from the already-existing `operator_overlap` computation
at `matcher.rs:73-78` — not a new risk introduced by this fix (Tier 3
already depends on the same comparison working correctly today), but
worth a quick sanity check of a few real incident payloads during
implementation rather than assuming.

## Summary of the fix

1. Rescope `lines_affected_by`'s `OperatorOnly`-drop retain
   (`matcher.rs:56-62`) to only drop an `OperatorOnly` match when a
   different line **sharing at least one of the same operator codes** has
   a more precise match — not any line on any operator anywhere in the
   incident. This is what the existing comment already claims to do.
2. In `match_one`'s Tier 2 (`matcher.rs:127-139`), reject a keyword hit
   when `incident.operators` is non-empty and does not include the line's
   own operator (i.e. the incident's own structured data positively
   contradicts the keyword hit) — not merely when it's silent on the
   matter. No new field is needed; `operator_overlap` is already computed.
3. No new `MatchScope` variant — the fix is a pure acceptance/rejection
   gate, not a new confidence tier that needs to survive downstream.
4. Add six new tests (one an exact-incident regression guard, five
   isolating each decision and its "must not regress" counterpart);
   confirm two existing tests are unaffected.
