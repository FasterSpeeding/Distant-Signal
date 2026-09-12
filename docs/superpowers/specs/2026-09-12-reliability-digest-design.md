# Design: Personal Reliability / Delay Repay Digest on `/track/mine`

**Status: design proposal, not approved.** No implementation in this pass —
this document is the entire deliverable. Written from direct investigation
of `crates/api/src/data/delay_repay_rules.rs`,
`crates/api/src/data/train_tracking.rs`, `crates/api/src/routes/train.rs`,
`frontend/components/DelayRepayEstimate.tsx`,
`frontend/app/track/mine/page.tsx`, and
`crates/api/migrations/20260829090000_journey_ticket_tracking.sql`, not
assumed from the brief.

## Goal

Add a retrospective summary to `/track/mine` — a logged-in user's own
tracked-journey punctuality (on-time %, average delay, most-delayed
journeys) plus an aggregate "how many of your tracked journeys may have
qualified for Delay Repay" rollup, reusing the *output* of the existing
per-ticket Delay Repay estimate rather than introducing a second call site
into `delay_repay_rules.rs`. Both halves are computed entirely from data
`/track/mine` already fetches — no new migration, no new backend route, no
new SQL.

## Corrections to the brief's assumptions

Six findings from direct investigation that change the shape of this
feature versus how the brief described it:

1. **A rollup £ amount is not buildable, ever, under this codebase's own
   legal/privacy constraint — this is not a v1 simplification, it is a
   hard ceiling.** `tracked_train_tickets`' migration
   (`20260829090000_journey_ticket_tracking.sql`) carries an explicit
   audit note: this table "must NEVER gain a column for payment/price
   data... Diff any future migration touching this table against this
   list before merging it." No fare amount is stored anywhere in this
   app, for any ticket. `DelayRepayEstimate` only ever carries a
   `percentage` of an unknown fare (`estimate_delay_repay`,
   `delay_repay_rules.rs:26-31`). The brief's own example phrasing — *"you
   could have claimed £X across N journeys"* — cannot be built without a
   fare figure this app deliberately never collects, and adding one now
   would be new, separately-reviewable scope this document does not take
   on. The rollup below reports **counts and percentage-of-fare figures
   only**, never a currency total, and says so explicitly in its own copy
   (see Decision 4).

2. **The eligibility rollup cannot honestly cover "ALL tracked trains, not
   just ones with a ticket attached," because operator data doesn't exist
   for most ticket-less tracked trains.** `estimate_delay_repay(operator,
   delay_minutes)` needs an operator string. Tracing every read model:
   - `TrackedTrainListItem`/`TrackedTrainState` (the shapes `GET
     /Train/mine` and the detail routes return) carry **no operator field
     at all** — confirmed by reading every `SELECT` in
     `train_tracking.rs` (`TRACKED_TRAIN_STATE_SELECT`,
     `list_tracked_trains_for_user`'s query).
   - `train_subscriptions.pin_operator` exists as a column
     (`20260828120000_train_tracking.sql:64`) and IS written by the
     legacy CRS+time pin flow (`create_pin`, sourced from
     `TrackTrainForm.tsx`'s LDBWS-suggestion `operator` field, still the
     form behind the main `/track` page) — but it is **never selected by
     any query in this file**, and is **never written at all** by the
     newer NR-primary "track this exact train" flow
     (`create_subscription_for_train`, backing `TrackThisTrainButton.tsx`
     and search-result "track this train" actions), which has no operator
     concept anywhere in its `INSERT ... SELECT`.
   - The shared `trains` table (`20260906100000_trains.sql`) has **no
     operator column at all** — schedule-matched data only gives calling
     points/termini, never a TOC code.

   The only operator source that is both populated and already surfaced
   over the wire today is an attached ticket's own `operator` field
   (`TicketListItem.operator`, `tracked_train_tickets.operator`). The
   rollup's population is therefore **tracked trains with an attached
   ticket that has a non-null operator** — the same population the
   existing per-ticket UI already serves, just counted and summarized
   rather than a coincidental restriction this document invents. See
   Decision 3's Open Question for the (out-of-scope-here) fix that would
   widen this.

3. **"Worst days" doesn't map onto this app's data model — this document
   reframes it as "your most delayed tracked journeys."** Nothing in this
   codebase groups a user's tracked trains by calendar day, and a user can
   (and does, per the group-sharing/multi-leg-commute use cases already
   supported) track more than one train on the same date. Building a
   day-level rollup would need a new grouping this document has no
   concrete need for. The digest instead surfaces a short list of the
   individual journeys with the highest recorded delay — the same grain
   every other list on this page (`TrackedTrainListRow`) already uses.

4. **There is no reliable "journey has actually finished" signal to gate
   on — this is a pre-existing, already-documented gap, not new to this
   feature.** `docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md`'s
   own Finding 1 established that `train_current_state.status` can reach
   `'completed'` in the schema but never actually does in practice
   (`crates/trust-consumer/src/journey.rs`'s own gap), and that page's own
   list deliberately does not attempt a completed/active split as a
   result. This digest inherits the same constraint and works around it
   with an explicit `serviceDate` cutoff instead of a status check — see
   Decision 1.

5. **`delay_repay_rules.rs`'s own module doc ("called from exactly one
   place in the whole codebase") is already slightly stale, and this
   feature is designed to avoid making that more true, not less.** By the
   time `GET /Train/tickets/mine` shipped, `estimate_delay_repay` gained a
   second read call site (`train_tracking.rs::build_ticket_list_item`,
   alongside `routes/train.rs::build_delay_repay_response`) — the
   comment's "exactly one place" claim predates that route. This document
   does not fix that stale comment (out of scope — see Explicitly out of
   scope), but it deliberately does **not** add a third call site: the
   digest's Delay Repay rollup consumes the already-serialized `estimate`
   field on `TicketListItem` (computed once, server-side, by the existing
   route), never calls `estimate_delay_repay` itself. This keeps the
   file's hard invariant — "pure, read-only, no write path" — exactly as
   easy to audit as it is today.

6. **The two existing "on time" conventions in this codebase disagree, and
   this document picks the one already visible on this exact page.**
   `common::Defaults::delay_threshold_minutes` (default `5`) is the
   backend's line/station-level "delayed" cutoff
   (`crates/common/src/lib.rs:1217-1219`), used nowhere in the tracked-train
   read path. `/track/mine`'s own `RowStatusBadge` (this file,
   `page.tsx:299-303`) already renders a per-train "Xm late"/"On time"
   badge using a bare `delayMinutes > 0` cutoff, with no threshold at all.
   Introducing the 5-minute backend threshold for the digest, directly
   above rows using the 0-minute frontend threshold, would show a train
   badged "3m late" while being counted "on time" in the summary above
   it. Decision 1 uses the same `delayMinutes > 0` cutoff `RowStatusBadge`
   already uses, for consistency on this one page — not
   `Defaults::delay_threshold_minutes`, and this divergence from the
   line-status feature's own definition is deliberate and named here, not
   accidental.

## Current relevant state (verified 2026-09-12)

- `frontend/app/track/mine/page.tsx` already fetches both
  `getMyTrackedTrains()` (→ `TrackedTrainListItem[]`, capped at
  `MINE_LIST_LIMIT = 100`, most-recently-tracked first) and
  `getMyTickets()` (→ `TicketListItem[]`, capped at `MINE_TICKETS_LIMIT`,
  most-recently-created first) in one `Promise.all`, on every request
  (`revalidate = 0`). Both return `null` on `401`, handled once at the top
  of the page.
- `TrackedTrainListItem` already carries `resolutionStatus`,
  `serviceDate`, `status` (`JourneyStatus | null`), and `delayMinutes`
  (`number | null`) for **every** tracked train, ticket or no ticket —
  this is the punctuality half's entire data source.
- `TicketListItem` already carries `trackedTrainId`, `operator`,
  `delayMinutes`, and `estimate` (`DelayRepayEstimate | null`,
  pre-computed server-side by `build_ticket_list_item`) — this is the
  Delay Repay rollup's entire data source.
- `DelayRepayEstimate.tsx`'s hedging discipline (verbatim, load-bearing):
  the top-level `disclaimer` string is rendered "in full, every time...
  never shortened, never hardcoded as an equivalent-sounding sentence;"
  `estimate.disclaimer` (a second, textually different string) is
  deliberately never rendered alongside it, "two near-duplicate-but-not-
  identical caveats on screen at once would read as inconsistent, not
  doubly cautious;" the claim link is always phrased as leaving this app,
  never as this app performing a claim. Every one of these constraints
  carries forward into Decision 4's rollup component, which is new copy,
  not a reuse of this component's JSX — see Decision 4 for why a literal
  reuse doesn't fit an aggregate.
- `resolution_status` values in practice: `pending`, `schedule_matched`,
  `unresolved`, `resolved`. Only `resolved` rows have ever had a real
  train identity resolved and can carry live/historical delay data.
- No retention or pruning job exists for `train_subscriptions` (per
  `MINE_LIST_LIMIT`'s own doc comment) — the two existing list caps are
  the only bound on how much history this page, and therefore this
  digest, ever sees.

## Decisions

### 1. Punctuality population: `resolutionStatus === 'resolved'`, `delayMinutes !== null`, `serviceDate` strictly before today

```ts
function isEligibleForPunctuality(train: TrackedTrainListItem, today: string /* YYYY-MM-DD */): boolean {
  return train.resolutionStatus === 'resolved' && train.delayMinutes !== null && train.serviceDate < today;
}
```

- `resolutionStatus === 'resolved'` excludes `pending`/`schedule_matched`
  (no train identity was ever confirmed — nothing to measure) and
  `unresolved` (confirmed never to have been identifiable at all).
- `delayMinutes !== null` excludes a resolved train that has had no
  `train_current_state` write yet (a real, if narrow, gap: resolution and
  the first movement report don't always land in the same instant).
- `serviceDate < today` is this document's own addition, not inherited
  from any existing query, and exists specifically because of Correction
  4: since `status` can never reliably be observed reaching
  `'completed'`, the only honest proxy for "this journey has actually
  happened" is that its calendar service date has fully elapsed. A train
  still running *today* keeps a `delayMinutes` value that can still
  change before the day is out; counting it now would silently revise
  itself later with no visible indication anything changed. `today` is
  computed once per request, server-side, in the same Next.js request
  that already sets `revalidate = 0` for this exact reason (see the
  file's own comment on that export).
- **Cancelled journeys are excluded from the delay/on-time arithmetic
  entirely, and reported as a separate count.** `status === 'cancelled'`
  rows may still have a non-null `delayMinutes` left over from before
  cancellation, and folding that number into an "average delay" would
  misrepresent a cancellation as a large delay, or vice versa. This
  mirrors `DESIGN.md` §5.5/§6's own established pattern of treating delay
  rate and cancellation rate as two independent axes, never blended into
  one figure — this document extends that same posture to the per-user
  digest rather than inventing a new blended metric.
- **On time**: `delayMinutes <= 0`, matching `RowStatusBadge`'s existing
  convention on this exact page (Correction 6) — not
  `common::Defaults::delay_threshold_minutes`.
- **No rolling 30/90-day window.** Rejected in favor of "however far back
  the existing `MINE_LIST_LIMIT`/`MINE_TICKETS_LIMIT` caps already reach,"
  for a concrete reason: those caps are **count-based** (last 100 tracked,
  last N tickets), not calendar-based. Filtering that already-truncated
  set down further by calendar date would produce a window that looks
  like "your last 30 days" but is silently also bounded by "and also
  whichever of your last 100 tracked trains happen to fall in it" — a
  compound, confusing definition for zero benefit, since nothing about
  Delay Repay windows or punctuality reporting has a natural 30/90-day
  cadence anyway (unlike a subscription-renewal or billing metric, where
  that cadence would mean something). The digest's own copy says "your
  last N tracked journeys" (N = the actual eligible count, computed live)
  rather than implying a fixed time range it doesn't really have. If real
  usage later shows `MINE_LIST_LIMIT` itself is wrong for this purpose,
  that is the same "revisit once real usage exists" posture the limit's
  own doc comment already takes — not a reason to invent a second,
  independent windowing rule for just this feature today.

### 2. "Most delayed journeys": top 5 eligible trains by `delayMinutes` descending, ties broken by most recent `serviceDate`

Reuses the exact same eligibility filter as Decision 1 (no separate
population). Capped at 5 — small enough to render as a plain list under
the summary numbers without its own pagination, consistent with this
page's existing "no pagination anywhere" posture (`tracked-trains-list-
design.md`'s own Open Questions 1-2, still open, still un-addressed by
this document). Each entry links to the same `/train/{uid}/{date}` or
`/train/by-id/{id}` href `TrackedTrainListRow` already computes — no new
routing.

### 3. Delay Repay rollup population: attached tickets with a non-null `estimate`, out of attached tickets with a non-null `operator`

```ts
interface DelayRepayRollup {
  attachedTicketsWithOperator: number; // denominator: tickets we could even attempt an estimate for
  eligibleCount: number;               // numerator: estimate !== null
  bandCounts: Record<'DR15' | 'DR30', { band15?: number; band30?: number; band60?: number }>;
  // (exact shape: a count per (scheme, bandMinutes) pair actually observed
  // — see Decision 4 for why this is shown instead of an average percentage)
}

function computeDelayRepayRollup(tickets: TicketListItem[]): DelayRepayRollup {
  const attached = tickets.filter((t) => t.trackedTrainId !== null && t.operator !== null);
  const eligible = attached.filter((t) => t.estimate !== null);
  // bandCounts: tally `${estimate.scheme}-${estimate.bandMinutes}` over `eligible`
  ...
}
```

- Standalone tickets (`trackedTrainId === null`) are excluded from both
  numerator and denominator — there is no journey yet to have been
  delayed on, per `TicketListItem`'s own doc comment ("estimate/
  delayMinutes are already nullable and stay null for the same row, by
  construction").
- An attached ticket with `operator === null` is excluded from the
  denominator too, not counted as a "0% eligible" journey — this app has
  no idea whether that operator would have paid out at all, and folding
  "we don't know" into the same bucket as "we know and it's zero" would
  misstate the denominator's own meaning.
- **No average/blended percentage is computed or shown.** `percentage` is
  a percentage of an unknown fare (Correction 1) — averaging "25% of an
  unknown amount" and "100% of a different unknown amount" produces a
  number with no unit anyone can act on. The rollup instead reports a
  plain count per band actually observed ("2 journeys hit the 60+ minute,
  100%-of-fare band; 1 journey hit the 15-29 minute, 25% band"), which
  stays honest about what is and isn't knowable.
- **Open question, explicitly not resolved here**: surfacing
  `pin_operator` (currently written-but-never-read for legacy CRS+time
  pins, per Correction 2) on `TrackedTrainListItem` would let a
  ticket-less legacy-flow tracked train also enter this rollup, closing
  part of the "not really ALL tracked trains" gap the brief assumed was
  already closed. This is a small, isolated backend change (one column
  added to one `SELECT` and one struct field) but is explicitly out of
  scope for this document — see Explicitly out of scope — because it
  does not change this document's own decisions if done later, and NR-
  primary-tracked trains (the increasingly common "track this exact
  train" flow) would still have nothing to show either way.

### 4. Exact rollup copy — matching `DelayRepayEstimate.tsx`'s hedging discipline, not softened for the aggregate case

New component, `frontend/components/ReliabilityDigest.tsx` — new copy, not
a reuse of `DelayRepayEstimate`'s JSX (that component is written for a
single ticket's `DelayRepayEstimateResponse` shape and a single-estimate
narrative; forcing an aggregate through it would either lose the "N
journeys" framing or require changing a component three other call sites
already depend on). The **wording discipline** carries over exactly,
verbatim in spirit:

```tsx
function DelayRepaySection({ rollup }: { rollup: DelayRepayRollup }) {
  if (rollup.attachedTicketsWithOperator === 0) {
    return (
      <Text size="sm" c="dimmed">
        Attach a ticket to one of your tracked trains to see whether any of your journeys may
        have qualified for Delay Repay.
      </Text>
    );
  }

  return (
    <Stack gap={4}>
      <Alert color="blue" title="Possible Delay Repay eligibility, across your tracked journeys" variant="light">
        Of the {rollup.attachedTicketsWithOperator} tracked journey
        {rollup.attachedTicketsWithOperator === 1 ? '' : 's'} with a ticket attached, {rollup.eligibleCount} may
        have qualified for a partial or full refund of that journey&apos;s fare under the operator&apos;s Delay
        Repay scheme.
      </Alert>
      <Text size="sm">
        This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled
        — the same estimate already shown against each ticket below, just added up. It is a count of journeys,
        not a total amount: this app never stores ticket prices, so it has no fare figure to add up into a total
        refund value, and never will. Always verify eligibility and submit any claim directly with each operator
        — this app never submits a claim on your behalf, for one ticket or for all of them at once.
      </Text>
    </Stack>
  );
}
```

Point-by-point mapping back to `DelayRepayEstimate.tsx`'s own established
constraints (see Current relevant state):

- The top-level disclaimer sentence ("not a guarantee... not proof you
  travelled") is carried forward **verbatim**, same as the per-ticket
  component renders `response.disclaimer` verbatim — not paraphrased,
  not shortened, so a future backend wording change to `ROUTE_DISCLAIMER`
  should prompt updating this hardcoded copy too (flagged as a residual
  drift risk in Open questions, since this text is not read from the API
  the way the per-ticket component's is).
- No second, near-duplicate disclaimer is layered on top of it — same
  "two near-duplicate-but-not-identical caveats read as inconsistent, not
  doubly cautious" reasoning `DelayRepayEstimate.tsx`'s own doc comment
  gives for never rendering both of its two disclaimer strings at once.
- No claim-performing language ("claim now," "get your refund") anywhere
  in the aggregate — same posture the per-ticket component's own test
  (`DelayRepayEstimate.test.tsx`'s "never claim-performing language"
  case) already locks in for the single-ticket case.
- The "not a total amount, we never store fare data" sentence is **new**
  copy with no single-ticket precedent, because the single-ticket
  component was never in a position to imply a running total in the
  first place — this is the aggregate-specific hedge the brief asked for
  ("needs AT LEAST equally careful wording... don't soften it").
- No outbound claim link is rendered at the rollup level at all —
  deliberately: an aggregate has no single operator to link to, and
  fabricating one, or listing every distinct operator's claim URL a
  second time here, would duplicate the per-ticket links already
  rendered below in `TrackedTrainListRow`/`UnattachedTicketRow` for no
  benefit. The rollup's job is to say "go look at the tickets below,"
  which the existing page layout (Decision 5) already makes true by
  construction.

### 5. Placement on `/track/mine`: one new `Card` between the page header and the trains list, always both fetched arrays

```tsx
<Stack p="lg" gap="lg">
  <Group justify="space-between" align="baseline">{/* existing header, unchanged */}</Group>
  {!nothingToShow && <ReliabilityDigest trains={trains} tickets={tickets ?? []} />}
  {nothingToShow ? (/* existing empty state, unchanged */) : (/* existing trains/tickets lists, unchanged */)}
</Stack>
```

- Gated on the same `nothingToShow` the page already computes — a user
  with literally nothing tracked gets the existing empty-state prose, not
  a digest card reporting all zeroes above it.
- `ReliabilityDigest` itself further degrades internally: if
  `trains.filter(isEligibleForPunctuality)` is empty, the punctuality
  half renders "Track a train and check back once it's finished running
  to see your punctuality here" instead of a 0%/0-minute figure that
  would misleadingly read as "perfect record" rather than "no data yet."
  Same reasoning, independently, for the Delay Repay half (Decision 4's
  own zero-state).
- No new fetch, no new `Promise.all` member — `trains`/`tickets` are the
  exact same arrays the rest of the page already renders from. This
  directly answers the brief's "does it need a new backend aggregation
  route" question: **no** — every number in this feature is derivable,
  today, from `GET /Train/mine` + `GET /Train/tickets/mine`'s existing
  response shapes, both already fetched by this page on every request.

### 6. Naming

- Frontend: `frontend/lib/reliabilityDigest.ts` (new, pure — 
  `isEligibleForPunctuality`, `computePunctualitySummary`,
  `computeDelayRepayRollup`, all plain functions over already-typed
  arrays, no fetch of their own, mirroring `lib/dateFormat.ts`'s/
  `lib/trackingName.ts`'s existing "pure helper module, no I/O" shape),
  `frontend/components/ReliabilityDigest.tsx` (new component, composing
  a `PunctualitySection` and a `DelayRepaySection`).
- No new Rust module, no new wire type, no new migration.

## Architecture

```
/track/mine (existing Server Component, revalidate = 0)
        │
        ├── getMyTrackedTrains()  ──▶ TrackedTrainListItem[]   (existing fetch, unchanged)
        ├── getMyTickets()        ──▶ TicketListItem[]          (existing fetch, unchanged)
        │
        ▼
<ReliabilityDigest trains tickets today={todayIsoDate()} />   (new component)
        │
        ├── computePunctualitySummary(trains, today)
        │     filters isEligibleForPunctuality → { onTimePct, avgDelayMinutes,
        │     cancelledCount, worstJourneys: top 5 by delayMinutes }
        │
        └── computeDelayRepayRollup(tickets)
              filters attached+operator → { attachedTicketsWithOperator,
              eligibleCount, bandCounts }
```

No new backend route, no new SQL, no new migration. Everything left of
`<ReliabilityDigest>` in this diagram already exists and is unchanged.

## Non-goals

- **Does not auto-submit any Delay Repay claim.** Every existing claim
  link stays an outbound, same-tab-leaving link to the operator's own
  page (`claim_url_for`/`GENERIC_CLAIM_URL`), exactly as today. The
  rollup adds no new claim mechanism of any kind.
- **Does not store payment, refund, or fare-price data of any kind.**
  Correction 1 explains why this is a hard ceiling, not a preference —
  `tracked_train_tickets`'s migration-level audit note already forbids
  it, and this document introduces no new table or column that could
  violate it.
- **Does not track claim outcomes** (submitted / accepted / rejected /
  paid). This is a distinct, separate idea from the same research batch
  that produced this brief, explicitly called out by the brief itself as
  something not to fold in here without a specific reason — this document
  has none, and does not attempt it. A future "claim tracker" feature
  would need its own migration, its own legal/privacy review (it would be
  the first place in this app that could plausibly want to store
  claim-adjacent data), and its own design document.
- **Does not change `delay_repay_rules.rs`.** No new function, no new
  call site into that file — Correction 5 explains why the rollup instead
  consumes `TicketListItem.estimate`'s already-serialized output.
- **Does not add pagination, filtering, or a date-range picker** to the
  digest. It reports over whatever `MINE_LIST_LIMIT`/`MINE_TICKETS_LIMIT`
  already return, full stop — consistent with this page's own existing
  "no pagination anywhere" posture.

## Testing approach

**Backend**: none needed — this feature adds no Rust code. If Decision
3's open question (surfacing `pin_operator`) is ever picked up separately,
that follow-up would need its own `#[sqlx::test]`-backed (or `#[ignore]`d,
matching this codebase's existing convention for tests needing a real
`PgPool`) coverage of the widened `SELECT`, but that is out of scope here.

**Frontend** (`frontend/lib/reliabilityDigest.test.ts`, plain Vitest, no
DOM):
- `isEligibleForPunctuality`: excludes `pending`/`schedule_matched`/
  `unresolved`; excludes `resolved` with `delayMinutes: null`; excludes
  `serviceDate === today` and `serviceDate` in the future; includes a
  `resolved`, non-null-delay, strictly-past row.
- `computePunctualitySummary`: on-time count uses `delayMinutes <= 0`
  (matching `RowStatusBadge`, not the 5-minute backend threshold);
  cancelled journeys are excluded from `avgDelayMinutes`/`onTimePct` and
  counted separately; `worstJourneys` returns at most 5, sorted descending
  by `delayMinutes`, ties broken by most recent `serviceDate`; an empty
  eligible set returns a explicit "no data" shape, never `NaN`/`0` dressed
  up as a real average.
- `computeDelayRepayRollup`: a standalone ticket (`trackedTrainId: null`)
  is excluded from both numerator and denominator; an attached ticket with
  `operator: null` is excluded from the denominator (not counted as
  ineligible); `bandCounts` tallies match a hand-built fixture with one
  ticket in each of the three known bands (15/30/60-minute).

`frontend/components/ReliabilityDigest.test.tsx` (Vitest +
Testing Library, mirroring `DelayRepayEstimate.test.tsx`'s own structure):
- Zero eligible punctuality data → renders the "check back later" prose,
  never a 0%/0-minute figure.
- Zero attached-tickets-with-operator → renders the "attach a ticket to
  see this" prose, never a 0-of-0 figure.
- **Hedged-copy assertions, matching `DelayRepayEstimate.test.tsx`'s own
  pattern exactly**: given a rollup with `eligibleCount > 0`, assert the
  rendered text contains the carried-forward disclaimer sentence in full
  ("not a guarantee of compensation and not proof you travelled"); assert
  it also contains the aggregate-specific "not a total amount... never
  stores ticket prices" sentence; assert no claim-performing string
  ("claim now," "submit," "get your refund") appears anywhere in the
  rendered output; assert no outbound `<a>` is rendered by this component
  at all (Decision 4's "no rollup-level claim link" choice).
- A regression test asserting the two carried-forward disclaimer
  sentences here are *substring-equal* to the literal strings currently
  hardcoded in `DelayRepayEstimate.test.tsx`'s own
  `TOP_LEVEL_DISCLAIMER`/`ROUTE_DISCLAIMER` fixtures — since this
  component's copy is hardcoded TS, not fetched from the API (Decision
  4's named residual drift risk), this is the one mechanical guard against
  the two silently drifting apart if `delay_repay_rules::ROUTE_DISCLAIMER`
  is ever reworded.

## Open questions / risks

1. **The hardcoded-copy drift risk named in Decision 4/Testing.** Unlike
   the per-ticket component, this rollup's disclaimer text is not fetched
   from the API (there is no aggregate API response to fetch it from,
   per Decision 5 — everything is computed client-side from data that
   itself already carries its own per-ticket disclaimer). A future wording
   change to `ROUTE_DISCLAIMER`/`DISCLAIMER` in `delay_repay_rules.rs`
   will not automatically propagate here. The regression test above
   catches this at the string level but does not prevent it; a
   more robust fix (e.g., exposing the disclaimer text via a small
   `GET /public/delay-repay/disclaimer`-shaped read, or a shared constant
   in `common`) is real, separate work this document flags but does not
   do, since it would be the first `common`-crate item deliberately shared
   between Rust and TypeScript literal text with no existing precedent for
   how this codebase does that.
2. **Whether `pin_operator` should be surfaced** (Decision 3's Open
   question) to widen the Delay Repay rollup's population beyond
   ticket-attached trains, for the subset of legacy CRS+time pins that
   captured an operator at track-time. Not resolved here — a small,
   isolated, separately-reviewable backend change if picked up.
3. **Whether `MINE_LIST_LIMIT`/`MINE_TICKETS_LIMIT`'s count-based caps are
   the right lens for a "personal history" feature**, versus this page's
   original "recent activity" framing they were sized for. Same
   "revisit once real usage exists" posture as those constants' own doc
   comments already take (Decision 1) — not resolved here, named as a
   dependency this feature inherits rather than one it introduces.
4. **Sample-size framing for a very new account.** A user with exactly one
   eligible journey gets a 100%-or-0% on-time figure and a single-journey
   "average" delay — both true, but easy to over-read as a trend from one
   data point. This document does not add a `min_sample_size`-style gate
   (per-user data isn't a statistical inference over noisy samples the way
   `SampleAvailability`'s station/line-level threshold is — one real
   journey is one real, fully-known fact about that one journey), but the
   UI should show the eligible count `N` alongside every figure so a
   reader can judge this themselves. Not treated as blocking, flagged for
   whoever builds this next.
