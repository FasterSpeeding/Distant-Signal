# RDM Attribution Wording Correction — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Task 1 of this plan is a legal sign-off gate, not an engineering step.**
> Do not implement Task 2 until Task 1's confirmation has actually happened
> — see that task for why.

## Resolved since this plan was written

**Question closed: no, no other document independently requires NRE's
fixed attribution wording on top of a feed's own blank Schedule 1 §8.**

The RDM "Platform Agreement (data consumer)" — the marketplace-platform-
level agreement covering account setup, fees/invoicing, SLAs, IP in the
platform itself (not the data), confidentiality, liability, and dispute
resolution — has since been obtained and read in full (all 25 clauses and
both schedules: a Service Level Agreement, and a list of policy links —
privacy/cookie/security/refund). It contains **no attribution clause of
any kind**, anywhere, and twice explicitly disclaims any role in
content-licensing terms:

- **Clause 6.3**: "RDG is not a party to the Data Sharing Agreement and
  the Data Consumer acknowledges and agrees that it will not hold RDG
  responsible for any liabilities arising out of or connected to the Data
  Sharing Agreement."
- **Clause 22.3**: "RDG is not a party to the Data Sharing Agreements
  entered into between the Data Publisher and the Data Consumer... the
  Data Consumer... shall resolve any disputes arising out of or in
  connection with any Data Sharing Agreements directly with the relevant
  Data Publisher."

Neither "National Rail Enquiries," nor "Developer Guidelines," nor any
attribution standard is mentioned anywhere in the Platform Agreement,
incorporated by reference or otherwise. **Conclusion: each feed's own DSA
Schedule 1 §8 is the complete and exclusive source of that feed's
attribution requirement — nothing else layers on top.** A blank
Schedule 1 §8 (Knowledgebase Incidents, Knowledgebase TOC data, and — if
that turns out to be the governing agreement — "Stations Reference Data")
really does mean "no specific wording required, general 'any reasonable
manner' clause applies," full stop.

This closes out one dimension of Task 1's sign-off gate below. It does
**not** touch the two questions that are still genuinely open in Task 1 —
whether the general "any reasonable manner" clause already makes today's
single umbrella line legally sufficient regardless of Schedule 1 §8's more
specific wording, and which of two similarly-named Stations products this
app's subscription actually is. Those are unrelated to this finding and
still require real human sign-off.

**Goal:** `frontend/components/OpenDataAttribution.tsx` currently renders a
single line — "Powered by National Rail Enquiries" — and its doc comment
claims this satisfies attribution for all four Rail Data Marketplace (RDM)
feeds this app consumes, under "one NRE licence family." A licence-compliance
audit of the actual signed RDM Data Sharing Agreements (completed earlier
this session; the source PDFs no longer exist in this repo — see the
"Audited facts" section below, which is this plan's record of that audit's
findings) found that claim does not hold: each agreement's own Schedule 1 §8
"ATTRIBUTION" field, where non-blank, names a specific required
organisation/wording more specific than — and better read as overriding —
the general "any reasonable manner" clause every RDM agreement also shares.
This plan corrects `OpenDataAttribution.tsx`'s rendering and doc comment to
reflect the real, per-feed sourcing, without inventing certainty the audit
didn't produce.

**Architecture:** No new component, no new data flow — this is a rendering
and copy change to one existing Server Component, plus its colocated test
file. The component stays a plain, non-interactive Server Component
rendered once by the root layout.

**Tech Stack:** Next.js App Router + TypeScript + Mantine v9, Vitest +
`@testing-library/react` + this repo's `renderWithMantine` helper
(`frontend/test/render.tsx`).

**Spec:** none — per the task that produced this plan, the design reasoning
below is done inline rather than in a separate spec document. There is no
`docs/superpowers/specs/*.md` to cross-reference.

---

## Audited facts (ground truth for this plan — do not re-derive)

The source PDFs for this audit were deleted after the audit that produced
this plan completed; the table below is this plan's only remaining record
of it and must be treated as authoritative, not re-fetched or re-verified
against the (now-absent) PDFs.

Every RDM Data Sharing Agreement shares a general clause (paraphrased):
*"give an appropriate credit to the Data Publisher by identifying the Data
Publisher... as the source... in any reasonable manner"* — no fixed wording
at this general level. Each agreement's Schedule 1 §8 "ATTRIBUTION" field,
when non-blank, names a specific required wording for that one agreement:

| Feed (product covered) | Data Publisher | Schedule 1 §8 (exact required wording, or blank) |
|---|---|---|
| Darwin Real Time Train Information (Push) — the LDBWS/live-departure-boards data source | NRE | **`powered by NationalRail`** (verbatim: lowercase "powered", one word "NationalRail") |
| NationalRail Knowledgebase Stations (JSON) | NRE | **`NationalRail (Train Information Services Ltd)`** |
| Live Departure Board (a separate RDM product from the Darwin push feed above) | RDG | `Rail Delivery Group` |
| Reference Data (generic-named product) | RDG | `Rail Delivery Group` |
| Stations Reference Data (two versions found, v1 and v1.2) | — | blank — general clause only |
| Knowledgebase TOC data | — | blank — general clause only |
| Knowledgebase Incidents (confirmed via its own product-ID-matching Schedule 1 §3 as one of this app's four core feeds) | **Rail Delivery Group**, not NRE | blank — general clause only |

None of the seven agreements checked names "National Rail Enquiries" as the
Schedule 1 §8 wording. The current single "Powered by National Rail
Enquiries" line is, at best, relying on the general "any reasonable manner"
clause for every feed — arguable, since NRE is a real, recognisable umbrella
brand for this category of data even where it isn't literally "the Data
Publisher" on a given contract — but where Schedule 1 §8 *does* specify
fixed wording, the more specific contractual term is the safer, more
defensible reading to follow exactly.

### Mapping the audit rows onto this app's four consumed feeds

This app's doc comment (pre-fix) names its four RDM feeds as "Knowledgebase
Incidents, LDBWS/Darwin live departure boards, Stations, TOCs." Matching
those against the table above:

- **LDBWS/Darwin live departure boards → "Darwin Real Time Train Information
  (Push)."** Unambiguous — Darwin is the only Push/LDBWS-named row in the
  table, and matches the existing doc comment's own "LDBWS/Darwin" framing
  exactly. Required wording: `powered by NationalRail`.
- **Knowledgebase Incidents → the "Knowledgebase Incidents" row.**
  Unambiguous — the audit confirmed this one specifically via product-ID
  matching, not just name matching. Schedule 1 §8 blank; Data Publisher is
  RDG, not NRE.
- **Knowledgebase TOC data → the "Knowledgebase TOC data" row.**
  High-confidence by name match (both the existing doc comment's "TOCs" and
  the audit's "Knowledgebase TOC data" carry the same "Knowledgebase"
  branding as the confirmed Incidents row), but — unlike Incidents — this
  match was **not** independently product-ID-confirmed by the audit.
  Schedule 1 §8 blank either way, so this ambiguity doesn't change what
  code needs to render, only the confidence behind "no line needed here."
- **Stations → ambiguous between two different rows.** The audit found
  *two* distinct Stations-shaped products: "NationalRail Knowledgebase
  Stations (JSON)" (Schedule 1 §8 = `NationalRail (Train Information
  Services Ltd)`) and "Stations Reference Data" (v1/v1.2, Schedule 1 §8
  blank). The existing doc comment's "Knowledgebase Incidents... Stations...
  TOCs" phrasing groups Stations alongside the two other Knowledgebase-branded
  feeds, which points toward "NationalRail Knowledgebase Stations (JSON)"
  being the one this app actually subscribes to — but, unlike Incidents,
  the audit did not product-ID-confirm this, and a same-topic product
  existing under a *different*, blank-attribution agreement ("Stations
  Reference Data") is a real, live alternative reading, not a hypothetical
  one. **This plan cannot resolve which agreement governs this app's actual
  Stations subscription from the audit record alone — see Task 1.**

The two RDG-attributed rows ("Live Departure Board," "Reference Data") don't
match any of this app's four confirmed feeds by name and are not addressed
by this plan — they're recorded here only because the audit checked them as
adjacent/candidate feeds. If this app ever subscribes to a product under
either of those exact names, add its `Rail Delivery Group` line then.

**Net effect on rendering:** of the four feeds, only two (Darwin/LDBWS,
and Stations *if* the Knowledgebase-Stations reading holds) plausibly need
a new, non-blank attribution line at all. The other two (Incidents, TOC)
have blank Schedule 1 §8 fields regardless of which reading applies to
Stations, so nothing about them changes what this plan implements — only
what legal judgment the "no line needed" choice for them rests on (see
Task 1).

---

## Design: one line per required exact string, not a merged/paraphrased line

**The concrete question this plan has to settle:** does `powered by
NationalRail` (Darwin) read as "close enough" to `NationalRail (Train
Information Services Ltd)` (Knowledgebase Stations) to combine into one
shared line, saving a row of footer clutter?

**Decision: no — render each as its own separate, verbatim line, if both
apply.** Reasoning:

1. **This codebase already has a governing precedent for this exact
   question**, in this same file: TfL's doc comment states "The wording is
   fixed — do not paraphrase it," a rule the existing NRE line already
   claims (correctly, just applied too broadly) to follow. A merged phrase
   that isn't verbatim to *either* Schedule 1 field is a paraphrase of both,
   satisfying neither's exact-string requirement. There's no textual overlap
   to exploit anyway — `NationalRail (Train Information Services Ltd)`
   doesn't contain `powered by`, and `powered by NationalRail` doesn't name
   "Train Information Services Ltd" at all; any single sentence covering
   both would have to invent wording that appears in neither agreement.
2. **They're independently negotiated agreements** (Darwin Push vs.
   Knowledgebase Stations (JSON) are different RDM products with their own
   Schedule 1), so nothing in the general "any reasonable manner" clause —
   which only kicks in where §8 is *blank* — implies one satisfies the
   other's non-blank, specific requirement.
3. **The clutter cost is small and the compliance cost of guessing wrong is
   not.** The footer already renders three lines (TfL, NRE, Network Rail);
   this change grows the middle "NRE-family" group from one line to at most
   two. That's a modest, not a structural, UX change — nothing here is
   remotely close to the "four extra lines" worst case the brainstorming
   question raised, because two of the four feeds (Incidents, TOC) need no
   line at all (blank Schedule 1 §8; see below).

**The two blank-Schedule-1-§8 feeds (Incidents, TOC) get no dedicated line.**
Per the general clause, "any reasonable manner" is satisfied without a
literal string match — but this plan flags, rather than asserts, that the
existing NRE/NationalRail branding elsewhere in the footer reasonably
identifies these feeds' data as sourced from the same family. This is
weakest for Incidents specifically, since its confirmed Data Publisher is
**Rail Delivery Group, not NRE** — nothing in the footer, before or after
this plan, names "Rail Delivery Group" anywhere. See Task 1's second open
question.

**Net structure after this plan** (pending Task 1's sign-off):

```
Powered by TfL Open Data                                    <- unchanged
powered by NationalRail                          (linked)   <- NEW, replaces the old NRE line, Darwin/LDBWS-specific
NationalRail (Train Information Services Ltd)               <- NEW, conditional on Task 1 confirming the Stations reading
Live train movement data from Network Rail's open data feeds <- unchanged
```

If Task 1's sign-off instead confirms the *other* Stations reading (the
blank "Stations Reference Data" agreement governs), the third line above is
simply not added, and the footer stays at three lines, two of them NRE-family.

---

## Global Constraints

- **Do not paraphrase any required string.** `powered by NationalRail` and
  `NationalRail (Train Information Services Ltd)` must appear byte-identical
  to the audited table above, including capitalisation — `NationalRail` is
  one word in both, `powered` is lowercase in the Darwin string specifically
  (unlike the old line's "Powered by National Rail Enquiries," which is not
  being kept anywhere in the new copy).
- **Task 1 (sign-off) gates Task 2 (implementation).** Do not merge/land
  Task 2 or Task 3 until Task 1's two open questions have an actual answer
  from whoever owns legal sign-off for this feature — matching this file's
  pre-existing TODO's posture for the Network Rail wording (unresolved
  legal judgment calls get flagged and parked, not silently resolved by
  whoever happens to be implementing).
- **The TfL line and the Network Rail line are out of scope.** Task 2 must
  not reword, move, or restyle either — this plan touches only the NRE/RDM
  paragraph of the doc comment and the NRE/RDM `<Text>` line(s) in the
  render.
- **No backend changes.** Confirmed by repo-wide grep (see "Verification
  already done during planning," below) that `"Powered by National Rail
  Enquiries"` and the string `NRE` appear nowhere else that renders live UI
  copy — only in `OpenDataAttribution.tsx` itself, its test file, and one
  historical/explanatory paragraph in
  `docs/superpowers/specs/2026-08-28-train-tracking-design.md` (line 287,
  quoting the *old* pattern as a cautionary example for Network Rail's
  different licence — that prose remains accurate as a historical
  description and needs no edit). No `cargo test` run is required by this
  plan.
- **Testing:** colocated `OpenDataAttribution.test.tsx`, Vitest,
  `@testing-library/react`, `renderWithMantine`. Run via `npm test` from
  `frontend/` (this repo's `package.json` defines `"test": "vitest run"`,
  so `npm test` and `npx vitest run` are equivalent here — either is fine).
- **File scope:** only `frontend/components/OpenDataAttribution.tsx` and
  `frontend/components/OpenDataAttribution.test.tsx` are modified by this
  plan. No other file changes.

### Verification already done during planning (do not redo)

Repo-wide grep for `"Powered by National Rail Enquiries"` and `\bNRE\b`
(excluding `node_modules`, `target/`) turned up exactly:
`frontend/components/OpenDataAttribution.tsx`,
`frontend/components/OpenDataAttribution.test.tsx`, and design-doc prose in
`docs/superpowers/specs/*.md` / `docs/superpowers/plans/*.md` / `DESIGN.md`
/ `crates/poller-incidents/src/config.rs` (a config field literally named
`nre_...`, unrelated to the attribution string) / `plans/01-poller-microservices.md`.
None of those besides the two `OpenDataAttribution.*` files render live UI
copy — they're either historical narrative (safe to leave) or an unrelated
identifier. This confirms the fix is fully contained to the two files this
plan modifies.

---

### Task 1: Confirm the attribution-per-Schedule-1-§8 approach with whoever owns legal sign-off — before implementing

**Files:** none — this is a sign-off checkpoint, not a code task.

This plan's own posture, matching this file's pre-existing TODO ("this
exact wording has not been through the dedicated legal sign-off pass this
feature's design doc calls for"): the engineering approach below is as
close to demonstrably correct as the audit record supports, but two real
judgment calls remain open and should not be silently resolved by whoever
implements this plan.

*(A third question that would otherwise have belonged here — whether some
other document, such as the RDM Platform Agreement, might independently
require NRE's fixed attribution wording on top of a feed's own blank
Schedule 1 §8 — has since been closed; see "Resolved since this plan was
written" at the top of this document. Nothing else layers on top of
Schedule 1 §8. The two steps below remain genuinely open and still need
real sign-off.)*

- [ ] **Step 1: Get an explicit answer on whether the general "any
  reasonable manner" clause makes today's single umbrella "Powered by
  National Rail Enquiries" line legally sufficient regardless of Schedule 1
  §8's more specific wording.** This plan's position — that the more
  specific contractual term should govern where one exists — is the more
  conservative, defensible-by-default reading, not a settled legal
  conclusion. Whoever owns sign-off may reasonably decide the general
  clause's flexibility already covers this and decline the added footer
  lines below. Do not treat this plan's approach as pre-approved.

- [ ] **Step 2: Get an explicit answer on which agreement governs this
  app's actual Stations RDM subscription** — "NationalRail Knowledgebase
  Stations (JSON)" (Schedule 1 §8 = `NationalRail (Train Information
  Services Ltd)`, required) or "Stations Reference Data" v1/v1.2 (blank).
  Unlike Knowledgebase Incidents, the audit did not product-ID-confirm this
  match; the "Design" section above's Knowledgebase-branding argument is
  circumstantial, not documentary. If sign-off can't determine this from
  the (now-deleted) source PDFs either, check the actual RDM account/API
  credentials this app's Stations ingestion uses for the product name it
  was provisioned under — that's live, checkable evidence the deleted PDFs
  no longer are.

- [ ] **Step 3: Record the answers** (e.g. as a short addendum to this plan
  file, or in whatever tracker this team uses for legal sign-off) before
  starting Task 2. If Step 1 comes back "the umbrella line is sufficient,"
  this plan's Task 2/3 should not proceed as written — re-scope to a
  no-op or a much smaller change per whatever sign-off actually decided.
  If Step 2 comes back "Stations Reference Data (blank)," Task 2 below
  should be implemented with the Stations line omitted (see Task 2, Step 1's
  note on this).

---

### Task 2: Rewrite `OpenDataAttribution.tsx`'s doc comment and render

**Files:**
- Modify: `frontend/components/OpenDataAttribution.tsx`

**Depends on:** Task 1's sign-off having actually happened.

- [ ] **Step 1: Replace the doc comment's NRE paragraph**

The current doc comment (`frontend/components/OpenDataAttribution.tsx`
lines 18–31) reads:

```
 * National Rail Enquiries is the same kind of condition, covering all four
 * RDM feeds this app consumes (Knowledgebase Incidents, LDBWS/Darwin live
 * departure boards, Stations, TOCs — one NRE licence family covers all
 * four, so one line suffices here the same way one TfL line covers TfL's
 * feed). Per NRE Terms & Conditions v3.0's Requirements clause and NRE
 * Developer Guidelines v06.01 §4 "Attribution": acknowledge NRE as the
 * source, with a link to the NRE website where possible, by displaying
 * "Powered by National Rail Enquiries" — fixed wording, same rule as TfL's.
 * Since this feed isn't the standalone/predominant information on the page
 * (it's combined with TfL data in this shared footer), the Guidelines'
 * "combined feeds" case applies: an attribution-page mention is sufficient,
 * rather than needing to sit directly alongside every individual piece of
 * NRE-derived content.
```

This is factually wrong per a since-completed licence audit — replace it
with (adjust the bracketed Stations sentence to match whichever reading
Task 1 confirmed):

```
 * The four RDM feeds this app consumes are NOT one shared licence family --
 * a Data Sharing Agreement audit (docs/superpowers/plans/2026-09-01-rdm-attribution-wording.md
 * has the full record; the source PDFs no longer exist in this repo) found
 * each agreement's own Schedule 1 Section 8 "ATTRIBUTION" field independently
 * either names a specific required wording or is blank (general "give
 * appropriate credit... in any reasonable manner" clause only). Per feed:
 *   - Darwin Real Time Train Information (Push), the LDBWS/live-departure-
 *     boards source: Schedule 1 requires "powered by NationalRail" verbatim
 *     (lowercase "powered", one word "NationalRail") -- rendered below,
 *     linked to nationalrail.co.uk as a courtesy (not itself required by
 *     Schedule 1's short field, but consistent with linking to the source
 *     where possible).
 *   - NationalRail Knowledgebase Stations (JSON): Schedule 1 requires
 *     "NationalRail (Train Information Services Ltd)" verbatim -- rendered
 *     below as plain text (no link required or added). [NOTE: confirm this
 *     is the actual product this app's Stations subscription is
 *     provisioned under before shipping -- the audit also found a
 *     differently-scoped "Stations Reference Data" product (v1/v1.2) whose
 *     Schedule 1 is blank; see the plan doc above, Task 1, Step 2.]
 *   - Knowledgebase Incidents: Schedule 1 blank; Data Publisher is Rail
 *     Delivery Group, NOT National Rail Enquiries. No line of its own here
 *     -- resting on the general "any reasonable manner" clause, which is a
 *     judgment call this plan's own sign-off task left open, not a settled
 *     conclusion (see the plan doc above, Task 1, Step 1).
 *   - Knowledgebase TOC data: Schedule 1 blank. Same "any reasonable
 *     manner" reasoning as Incidents applies; no line of its own here.
 * Two required strings that both name a National Rail entity are NOT
 * merged/paraphrased into one combined line -- see the plan doc's "Design"
 * section for why: they're independently negotiated Schedule 1 fields with
 * no textual overlap, and this file's own TfL precedent above ("wording is
 * fixed -- do not paraphrase it") already rules out inventing a hybrid
 * string that's verbatim to neither.
```

If Task 1 Step 2 confirmed the *blank* "Stations Reference Data" reading
instead, drop the Knowledgebase Stations bullet's requirement and the
`[NOTE: ...]` aside entirely, and fold Stations into the same "no line,
general clause" bullet already used for Incidents/TOC.

- [ ] **Step 2: Update the render — replace the single NRE `<Text>` block**

Current (lines 59–68):

```tsx
      <Text size="xs" c="dimmed">
        <a
          href="https://www.nationalrail.co.uk"
          target="_blank"
          rel="noopener noreferrer"
          style={{ color: 'inherit' }}
        >
          Powered by National Rail Enquiries
        </a>
      </Text>
```

Replace with (Darwin's line, always present per Task 1 Step 1's outcome
being "yes, implement the specific wording"):

```tsx
      <Text size="xs" c="dimmed">
        <a
          href="https://www.nationalrail.co.uk"
          target="_blank"
          rel="noopener noreferrer"
          style={{ color: 'inherit' }}
        >
          powered by NationalRail
        </a>
      </Text>
```

...followed by the Stations line, **only if Task 1 Step 2 confirmed the
Knowledgebase Stations (JSON) reading**:

```tsx
      <Text size="xs" c="dimmed">
        NationalRail (Train Information Services Ltd)
      </Text>
```

No line is added for Knowledgebase Incidents or Knowledgebase TOC data —
per the Design section above, their blank Schedule 1 §8 means no new
render output for them.

- [ ] **Step 3: Leave the TfL `<Text>` block and the Network Rail `<Text>`
  block untouched** — verify the diff touches only the doc comment's NRE
  paragraph and the one (or two) `<Text>` block(s) between the TfL block
  and the Network Rail block.

- [ ] **Step 4: Run the frontend build**

Run (from `frontend/`): `npm run build`
Expected: PASS, no new warnings.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/OpenDataAttribution.tsx
git commit -m "Correct RDM attribution wording: per-feed Schedule 1 §8 strings, not one NRE umbrella line"
```

---

### Task 3: Update `OpenDataAttribution.test.tsx`

**Files:**
- Modify: `frontend/components/OpenDataAttribution.test.tsx`

**Depends on:** Task 2.

- [ ] **Step 1: Replace the NRE test**

Current (lines 15–23):

```tsx
  it("carries National Rail Enquiries' required attribution verbatim, linked to their site", () => {
    // Same posture as the TfL line above: NRE Developer Guidelines v06.01
    // §4 fixes this exact wording for all four RDM feeds this app consumes
    // (Incidents, LDBWS/Darwin, Stations, TOCs).
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByText('Powered by National Rail Enquiries');
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', 'https://www.nationalrail.co.uk');
  });
```

Replace with:

```tsx
  it("carries the Darwin (LDBWS) feed's required attribution verbatim, linked to nationalrail.co.uk", () => {
    // Not decoration: the Darwin Real Time Train Information (Push)
    // Data Sharing Agreement's Schedule 1 §8 fixes this exact string --
    // lowercase "powered", one word "NationalRail" -- see
    // docs/superpowers/plans/2026-09-01-rdm-attribution-wording.md.
    // This wording is specific to the Darwin/LDBWS feed, not an umbrella
    // NRE claim covering every RDM feed this app consumes.
    renderWithMantine(<OpenDataAttribution />);
    const link = screen.getByText('powered by NationalRail');
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', 'https://www.nationalrail.co.uk');
  });
```

Add (only if Task 1 Step 2 confirmed the Knowledgebase Stations (JSON)
reading — otherwise skip this test and its corresponding render output per
Task 2 Step 2):

```tsx
  it("carries the Knowledgebase Stations feed's required attribution verbatim", () => {
    // NationalRail Knowledgebase Stations (JSON)'s Schedule 1 §8 fixes
    // this exact string, distinct from and not merged with the Darwin
    // line above -- see the plan doc's "Design" section for why a single
    // combined line isn't used.
    renderWithMantine(<OpenDataAttribution />);
    expect(screen.getByText('NationalRail (Train Information Services Ltd)')).toBeInTheDocument();
  });
```

- [ ] **Step 2: Leave the TfL test and the landmark test untouched** — the
  file's first (`"carries TfL's required attribution verbatim"`) and third
  (`"is a landmark..."`) tests need no changes.

- [ ] **Step 3: Run the test file**

Run (from `frontend/`): `npm test -- OpenDataAttribution`
Expected: all tests PASS (3 tests if Task 1 Step 2 confirmed the blank
Stations reading, 4 if it confirmed the Knowledgebase Stations reading).

- [ ] **Step 4: Run the full frontend suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS, no other test file references the old
`'Powered by National Rail Enquiries'` string (confirmed during planning —
see Global Constraints' "Verification already done during planning").

- [ ] **Step 5: Commit**

```bash
git add frontend/components/OpenDataAttribution.test.tsx
git commit -m "Update OpenDataAttribution tests for per-feed RDM attribution wording"
```

---

### Task 4: Final verification

**Files:** none — verification only.

- [ ] **Step 1: Re-grep the whole repo for the retired string**

```bash
grep -rn "Powered by National Rail Enquiries" --include="*" . | grep -v node_modules
```

Expected: zero matches (the string should no longer exist anywhere after
Task 2/3 land — it's not being kept as a fallback or an alias anywhere).

- [ ] **Step 2: Full frontend suite**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 3: No backend test run required**

Per Global Constraints, this plan makes no backend changes — `cargo test`
is not part of this plan's verification (confirmed during planning by
grepping the whole repo for the retired string and for `NRE`; nothing
outside the two frontend files needed changing).
