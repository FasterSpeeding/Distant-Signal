# Scoping: Is There a Safe-to-Build-Now First Slice of Option B's Consumer?

**Status: scoping verdict, not an implementation plan for Option B itself.**
Written to the same rigor as
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`
("the design spec") and
`docs/superpowers/specs/2026-09-03-full-coverage-per-station-stats-design.md`
("the per-station doc"), the closest precedent for this exact kind of
question ("should scaffolding be built ahead of an unbuilt/unvalidated
producer, or is that a real reason to wait"). This document answers one
question only: given that
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
("the findings doc") still verdicts **"not yet"** as of its 2026-09-03
re-run — blocked on production SSO access, not a broken mechanism, with an
honest empirical sample of **1 of 1** real spot-checked disruption
instances — is there a genuine, honest, smaller piece of Option B's actual
*consumer* (not more presentation scaffolding) that is safe and valuable to
build now, without waiting for validation to reach a real "go"? It reaches a
concrete, split verdict: **yes for one specific, narrow, pure piece; no for
everything else**, and draws the line precisely, with reasoning for exactly
why the line falls where it does.

Required reading, all consumed in full before this document was written:
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md`,
`docs/superpowers/plans/2026-08-29-trust-schedule-delay-validation.md`,
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
(all four dated sections), `docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md`
("the line-level doc"), `docs/superpowers/plans/2026-09-03-full-coverage-metrics-scaffolding-plan.md`,
and `docs/superpowers/specs/2026-09-03-full-coverage-per-station-stats-design.md`.

## The tension, stated precisely

The repo owner has twice overridden a "defer until Option B validates"
recommendation to build presentation scaffolding
(`common::FullCoverageAvailability`, `LineStatus.full_coverage_stats`,
`LineDefinition.full_coverage_enabled`, the sibling rollup tables) that is
real, merged, and **permanently inert**: its only integration point,
`crates/aggregator::merge_full_coverage`, is hard-wired to an empty
`HashMap::new()` at its only call site
(`crates/aggregator/src/main.rs:192`). That precedent is real, but it does
not automatically transfer to "therefore build the consumer too" — the
precedent's own safety property was that the scaffolding is inert *because*
nothing produces real data for it, and its cost was type/schema/route
plumbing, not correlation logic. A consumer is different in kind: its
entire reason to exist is correlation logic — matching real TRUST movement
events against a real CIF schedule to decide whether a real line's status
should say something different than it says today. That correlation logic's
*value* (does it actually catch/attribute more than sampling does) is
exactly what the validation plan was built to measure, and it has not
reached a stated N-of-M with any statistical weight — currently 1 of 1,
explicitly flagged by the findings doc itself as "too small to carry that
weight," gated now on a genuine "human needs to hand over a production SSO
session" blocker outside this task's own reach to fix.

## Two different things hide inside "build the consumer" — and they carry different risk

Reading the design spec's own "Feasibility" section
(`2026-08-29-trust-schedule-delay-inference-design.md`, "Three sub-problems,
assessed independently") and the findings doc's repeated real-data runs
side by side surfaces a split the brief for this task anticipated but that
is worth stating with evidence, not just asserted:

**1. Correlating real, live TRUST movement events against a resolved
schedule, at national-feed volume, to produce a per-line delay/cancellation
signal that gets fed into `merge_full_coverage` and can change what a real
line's status says.** This is genuinely what validation exists to justify.
Its risk is not implementation risk (STANOX↔CRS translation is fixed and
empirically confirmed working, `crates/trust-consumer/src/stanox_crs.rs`,
commit `6adf64f`) — it is **product-value risk and operational-cost risk**:
a second full-national-feed Kafka consumer group (the design spec's Option
B, "doubling the ingest-side read cost"), a new file-push ingestion shape
this app has never operated in production before schedule-ingest's recent
work, and — the part that actually touches users — writing real
severity-affecting data into a real, live `LineStatus`. Every one of those
costs is only justified by the coverage/segment-precision value validation
was designed to measure, and that measurement has not happened at any
statistically meaningful scale. **This is squarely gated on "go."**

**2. Reading a CIF SCHEDULE extract and resolving, for a given service
identity (a CIF `UID`) and date, its STP-overlay-correct booked
calling-point schedule — or, symmetrically, for a set of TIPLOCs and a
date, every service that calls there.** This is a pure parsing/matching
problem: fixed-width record decoding, `(UID, start date, STP indicator)`
uniqueness, "lowest STP letter wins for a given day," a 7-character
space-padded TIPLOC field, a `C`-indicator cancellation carrying no body.
**Its correctness was never the open question validation was measuring.**
It was, in fact, independently and repeatedly re-derived and confirmed
correct against real bytes across all four validation sessions, quoted
directly in the findings doc:

- 2026-08-29: a real multi-station join across WCML's five sample TIPLOCs
  produced 504 real end-to-end services from 488,798 scanned schedules, e.g.
  `UID C01370 STP=P [260523-261212]: EUS@0716 -> MKC@0750H -> CRE@1006H ->
  CAR@1200H` — a service pattern a real passenger would recognize
  (findings doc, "Task 5" section, 2026-08-29).
- 2026-08-31/09-01: the STP-overlay resolution correctly identified that
  most Mon–Fri weekday base schedules at Euston/Aldershot/Alton/Farnham
  carried a real `STP=C` override specifically for the UK August Bank
  Holiday (`260831`), and correctly found the real `STP=N` replacement
  schedules running under different UIDs (`F26094`, `Q98537`, `Q97575`,
  `Q97539`) — then a *live TRUST pin* against one of those replacement
  UIDs (`F26094`) tracked real calling points (`HRW` → `BSH`, Harrow &
  Wealdstone → Bushey) that lined up exactly against that UID's real CIF
  body (findings doc, "2026-08-31/09-01 re-run", "Step 3" and "Task 5"
  sections).

Both of these are the mechanism the design spec and validation plan set out
to test being *correct*, not the mechanism's *value*, and both have already
been demonstrated correct against real production CIF data, more than
once, independently. This is the piece with a genuine safe-to-build-now
slice: **promoting this proven-but-always-scratch schedule-resolution logic
into a real, tested, pure library**, with zero production data-path wiring,
is not "building Option B's consumer ahead of validation" in the sense the
brief is rightly cautious about — it is de-risking one already-settled
technical question ahead of a decision that hasn't been made about a
completely different, still-open question (does this add real value).

## A second, independent reason this specific piece is worth building regardless of Option B's fate

This is not invented for this document — it is already stated, in the
running codebase, as a real, current limitation of a feature that has
**already shipped and is already in production**, unrelated to Option B:

> `crates/trust-consumer/src/matching.rs:1-7` (module doc, quoted in
> full): "Best-effort resolution of a user's pin (origin CRS + scheduled
> departure time, date -- no train_uid) against the live TRUST feed... A
> heuristic, not a guaranteed join... this app has no CIF schedule lookup
> to bridge Activation's `train_uid` to a departure time."

Individual train tracking — a real, shipped, validated feature, not a
research effort — is *itself* held back today by the exact gap this
document proposes closing: there is no code anywhere in this repo that
takes a CIF `UID` (which TRUST's own `Activation` message hands over
directly, `train_uid`, per the design spec's own "Matching a TRUST message
to a scheduled service" section) and resolves it to that service's real
booked schedule. `crates/schedule-reference` (the only crate that reads
`RJTTF*MCA.txt`/`RJTTF*MSN.txt` today) parses only `TI`/`A` reference
records for the STANOX↔CRS table — nothing in this codebase parses `BS`/
`BX`/`LO`/`LI`/`CR`/`LT`, the actual Basic Schedule body records — confirmed
directly by reading `crates/schedule-reference/src/parser.rs` in full (it
implements exactly two functions, `parse_ti_lines`/the `MSN` `A`-record
equivalent, nothing else) and by grepping for any `BS`/`LO`/`LI`/`LT`
record-type-prefix match against real CIF line bytes anywhere under
`crates/`: the only hits are unrelated string literals (operator/matcher
code in `crates/aggregator/src/matcher.rs`), not CIF parsing. **This
library has value independent of whether Option B's
validation ever reaches "go,"** because it closes an already-documented gap
in an already-shipped feature — a genuine, separate justification from "get
a head start on Option B," even though it happens to also be exactly the
first building block Option B's own consumer would need.

The per-station doc independently arrived at the identical conclusion,
worth citing directly since it is the closest prior reasoning in this repo
to this exact question:

> `2026-09-03-full-coverage-per-station-stats-design.md`, "Current relevant
> state": "What does *not* yet exist... and is the real bottleneck under
> both line- and station-level full coverage: a `train_uid` → full
> booked-schedule bridge... This gap is not new information this document
> is introducing — it is the concrete shape of why Option B 'stays gated
> on its own future validation/planning pass.'"

That document named the gap and explicitly declined to build anything
against it, reasoning (correctly, at the time) that a *station-level wire
shape* built against it would be "scaffolding against a guess of a guess"
since no settled consumer contract existed. This document is not proposing
that wire shape — it is proposing the one piece underneath it that is not
a guess at all: the schedule-resolution logic itself, independently
re-derived and confirmed correct four separate times against real data,
with no consumer contract, no wire shape, and no production data-path
dependency required to build or test it.

## Verdict

**Split, not unanimous — and the line is drawn precisely, not by
convenience.**

1. **Building or wiring anything that reads live TRUST data, opens a
   second Kafka consumer group, computes a real per-line/per-station
   `SampleStats` from that live data, or populates the `full_coverage:
   &HashMap<String, SampleStats>` argument to `merge_full_coverage` in
   production: stays gated on validation reaching "go."** This is not a
   reflexive caution — it is because this is *specifically* the thing
   validation exists to justify, and its cost (a second full-feed Kafka
   consumer, a new file-push ingestion shape at scale, real writes to real
   line statuses users see) is not offset by anything this document found
   that changes the calculus validation itself already laid out. The
   presentation-scaffolding precedent does not transfer here, because that
   scaffolding's entire safety property — genuinely inert, zero blast
   radius, cheap to build ahead of its producer — does not hold for a real
   consumer, whose core value and risk *are* the correlation logic
   validation was built to test.

2. **A pure CIF SCHEDULE parsing/matching library — STP-overlay
   resolution, TIPLOC-body decoding, and two read-only queries
   (`schedule_for_uid`, `schedules_touching`) — is a genuine, safe,
   valuable first slice, buildable now.** Its correctness is not an open
   question (proven four times against real bytes, quoted above); it
   touches no live data path (no Kafka, no HTTP call, no database write, no
   wiring into `trust-consumer`'s live matching or `merge_full_coverage`'s
   production call site); and it has independent value today, closing a
   documented gap in a feature that has already shipped
   (`matching.rs`'s own module doc). See
   `docs/superpowers/plans/2026-09-03-option-b-consumer-first-slice-plan.md`
   for the concrete plan for exactly this slice, and nothing bigger.

## Explicitly out of scope (for both this document and the first-slice plan)

- **Any new or widened Kafka consumer group.** The first-slice plan adds no
  network I/O of any kind.
- **Wiring the new library into `trust-consumer`'s live pin-matching
  (`matching.rs`).** Even though this library closes exactly the gap that
  module's own doc names, *using* it there is a live-production-data-path
  change to an already-shipped, already-working feature — its own kind of
  blast-radius decision (more precise matching could also mean a
  differently-shaped failure mode) deserving its own review, not a
  drive-by side effect of building the library. Flagged as a natural
  follow-up, not committed to here.
- **Populating `merge_full_coverage`'s `full_coverage` map in production,
  for any line.** Stays exactly as inert as the presentation scaffolding
  left it.
- **Any change to `LineDefinition.full_coverage_enabled`'s default, or
  flipping it on for any real line.**
- **CIF `AA` (Association) records, freight-specific fields, or any record
  type this document's four cited validation runs did not already
  independently exercise against real data.** Per this repo's "no invented
  API details" convention — the first-slice plan only implements what has
  already been proven against real bytes.
- **A decision about whether Option B's eventual consumer lives as an
  extension of this new crate, a new `trust-line-aggregator`-shaped crate,
  or something else.** The design spec's own Option B recommendation
  (a dedicated consumer, separate from `trust-consumer`) is unchanged and
  not revisited here; this document only proposes one reusable building
  block that whichever future consumer gets built would need regardless of
  its own shape.

## Open questions / risks

1. **This verdict does not shorten the path to Option B's own "go."** The
   first-slice plan produces a tested library, not evidence toward the
   N-of-M question — that remains genuinely blocked on a human obtaining a
   real production SSO session, per the findings doc's own final section,
   unrelated to anything this document proposes building.
2. **Whether the library's two query shapes (`schedule_for_uid`,
   `schedules_touching`) are the right shape for whatever consumer
   eventually gets built is a reasoned guess, not a settled contract** — no
   consumer design exists yet to validate the shape against (the same
   caution the per-station doc raised about its own speculative sketches).
   This is accepted as a real, bounded risk: even if the eventual
   consumer's needs differ, the underlying parsing/STP-resolution logic
   (the expensive, error-prone part, per the validation findings' own
   repeated re-derivation) is reusable regardless of the exact query
   surface shape on top of it.
3. **This library will need real, larger-than-fixture CIF data to be
   exercised meaningfully beyond unit tests** (the untracked
   `timetable_full.zip`, ~711MB uncompressed, already used by every
   validation session but not present in this worktree and never
   committed to the repo). The first-slice plan's own tests use small, real
   fixture excerpts quoted directly from the findings doc's already-quoted
   real bytes — sufficient for correctness, not for a performance/memory
   check against the full national extract, which is named but not
   performed by this plan.
