# Outage Page Redesign + HTML Rendering Fix — Design

Sub-project 3 of 3. The "representative info" block depends on
`sample_stats` from sub-project 1
(`2026-07-09-custom-lines-and-blended-stats-design.md`); everything else
here is independent of both other sub-projects and can ship on its own.

## Goals

- Restructure `/lines/{id}` and `/stations/{crs}` (both render the same
  `LineStatusReport`/`LineStatus` shape) with: an overall status header,
  a representative-info block, and an issue list that's brief by default
  and expandable for detail.
- Let the issue list be filtered by severity, source type, and
  active-vs-upcoming.
- Fix `disruption.description` rendering as raw/escaped HTML instead of
  formatted content.

## Page structure

1. **Status header** — worst severity across the report's
   `lineStatuses`, as a prominent `StatusBadge`, plus operators. The
   worst-status logic already exists but is private to
   `LineStatusCard.tsx` (`worstStatus`); extract it to `lib/severity.ts`
   as a shared, tested function (`lib/severity.test.ts` already covers
   `severityRank` there) so the card and the new header share one
   implementation instead of two copies drifting apart.
2. **Representative info block** — shown only when at least one status
   on the report carries `sampleStats`: a compact stat row, e.g. "142 of
   160 sampled services delayed, avg 12 min late." Omitted entirely
   (not zeroed out) when no line on the report has sample stats.
3. **Issue list** — one entry per `LineStatus`. Collapsed by default:
   severity badge, `reason` text, data-quality tag, validity summary
   (e.g. "now" vs a date range). Expanding (Mantine `Accordion`) reveals:
   full sanitized-HTML `disruption.description`, affected stops
   (existing badge row), affected routes, `disruption.source`, full
   validity period dates.
4. **Filters** — controls above the issue list:
   - Severity (multi-select over the `Severity` values in play on this
     report)
   - Source type (`dataQuality`: Knowledgebase / LDBWS-inferred /
     Planned / Trust-inferred)
   - Active vs upcoming (`validityPeriods[].isNow` — active; `fromDate`
     in the future — upcoming)

   The full report is already fetched in one request server-side (no
   pagination), so filtering is client-side against already-loaded data
   — no re-fetch on filter change. The issue-list section becomes a
   client component (`IssueList.tsx`) taking `lineStatuses: LineStatus[]`
   as a prop and owning filter + expand/collapse state locally; the page
   itself stays a server component that just fetches and passes data
   down, matching the existing split (`LineDetailPage`/
   `StationDisruptionPage` server components → `DisruptionDetail` etc.).

   `/stations/{crs}` returns an *array* of `LineStatusReport` (a station
   can sit on several lines), unlike `/lines/{id}` which always resolves
   to one. Rather than flattening every line's statuses into one
   undifferentiated issue list (losing which line each issue belongs to),
   the station page keeps its existing per-line grouping (line name +
   divider) and repeats the status-header/representative-info/issue-list
   structure once per line.

## HTML rendering fix

Confirmed root cause: `disruption.description` contains real embedded
HTML (`<p>`, `<br>`, etc.) from the Darwin/Knowledgebase feed —
`quick_xml` fully entity-/CDATA-decodes it into a plain string at parse
time (`crates/poller-incidents/src/schema.rs`), and every layer between
there and the frontend (`IncidentMessage` → `aggregator`'s `Disruption`
→ `render.rs` → `frontend/lib/types.ts`) passes it through verbatim. The
frontend then renders it as an escaped React text node
(`<Text>{disruption.description}</Text>`), so the markup characters show
up literally instead of being interpreted. This is not
serialized/escaped XML needing re-parsing — it's already fully decoded
by the time it reaches the frontend; it just needs to be treated as HTML
content rather than opaque text.

Fix: sanitize with `isomorphic-dompurify` (not plain `dompurify` — these
pages are server-rendered, and plain DOMPurify assumes a browser
`window` it won't have during that first server-rendered pass;
`isomorphic-dompurify` falls back to jsdom when `window` is absent)
against an explicit allowlist — `p`, `br`, `strong`/`b`, `em`/`i`,
`ul`/`ol`/`li`, and `a` with only the `href` attribute, forcing
`target="_blank" rel="noopener"` on every link — then render via
`dangerouslySetInnerHTML`. `DisruptionDetail` becomes a client component
(`'use client'`) to keep the sanitize call colocated with the render
call; it takes no server data itself today, so this doesn't change how
its parent pages fetch anything.

## Testing

- `lib/severity.test.ts`: move/extend existing `worstStatus`-equivalent
  coverage after extraction.
- `DisruptionDetail.test.tsx`: extend with a fixture containing embedded
  `<p>`/`<br>`/`<script>` markup — assert the safe tags render as actual
  DOM elements and `<script>`/event-handler attributes are stripped.
- New `IssueList.test.tsx`: filter combinations (severity, source type,
  active/upcoming) narrow the rendered rows correctly; expand/collapse
  toggles detail visibility; representative-info block only renders when
  `sampleStats` is present on at least one status.
