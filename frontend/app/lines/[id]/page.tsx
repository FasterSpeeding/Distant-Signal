import { Suspense } from 'react';
import { notFound } from 'next/navigation';
import { Badge, Stack, Title, Text, Group, Button, Paper } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import {
  ApiNotFoundError,
  getLineStatus,
  getCustomLine,
  getLineDefinition,
  getAllLines,
  getAllTocs,
} from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { categoryLabel, operatorLabel, tocNameLookup } from '@/lib/displayLabels';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { DeleteLineButton } from '@/components/DeleteLineButton';
import { LineDefinitionTooltip } from '@/components/LineDefinitionTooltip';
import { ShareButton } from '@/components/ShareButton';
import { TextLink } from '@/components/TextLink';
import { worstStatus, severityLabel } from '@/lib/severity';
import { resolveHalfHourlyRange } from '@/lib/history';
import { londonDayKey } from '@/lib/dateFormat';
import type {
  CustomLineDetail,
  LineDefinitionSummary,
  LineGroupRef,
  LineStatusReport,
  LineSummary,
} from '@/lib/types';
import { HalfHourlyTrendsResults } from './history/HalfHourlyTrendsResults';
import { HalfHourlyCoverageTrendsResults } from './history/HalfHourlyCoverageTrendsResults';
import { LineTrainsResults } from './LineTrainsResults';

/** `Suspense` fallback for both "Recent trends" boundaries below (review
 * §2.11). Sized to the *empty* state both `HalfHourlyTrendsResults` and
 * `HalfHourlyCoverageTrendsResults` actually resolve to on a line with no
 * sampled data yet -- a `Paper withBorder p="md"` around one line of
 * `Text c="dimmed"` -- rather than to a chart's height, so a line with no
 * data yet doesn't jump from a tall placeholder down to a short message.
 * `role="status"`/`aria-busy` gives assistive tech something to announce
 * while a slower line's real chart is still loading, where the old bare
 * `Skeleton` gave a screen reader nothing at all. */
function TrendsLoadingFallback() {
  return (
    <Paper withBorder p="md" role="status" aria-busy="true">
      <Text c="dimmed">Loading trends…</Text>
    </Paper>
  );
}

/** `Suspense` fallback for the "Trains running today" boundary below, same
 * shape as `TrendsLoadingFallback` above (own copy rather than a shared one
 * -- the two boundaries' fallback copy is allowed to diverge independently,
 * and a shared helper would tempt someone to parameterise the wording
 * instead of just writing a second four-line function). */
function TrainsLoadingFallback() {
  return (
    <Paper withBorder p="md" role="status" aria-busy="true">
      <Text c="dimmed">Loading today&apos;s trains…</Text>
    </Paper>
  );
}

// Same `revalidate = 0` rationale as `/lines/[id]/history` -- this page now
// also computes a range off `Date.now()` (`resolveRange` below), so it must
// stay dynamic rather than be eligible for build-time prerendering.
export const revalidate = 0;

/** Whether this line has a `line_status` row yet, kept as a two-state
 * result instead of being collapsed into "found / not found".
 *
 * `GET /Line/{id}/Status` 404s when no `line_status` row matches the id
 * (`crates/api/src/routes/line_status.rs`'s `get_line_status`: an empty
 * `rows` after the private-custom-line filter is a `404`). That single
 * status code covers two genuinely different facts:
 *
 *   1. the id names no line this caller may read at all -- a typo, a
 *      deleted line, or someone else's private custom line; and
 *   2. the line exists and the caller can read it, but the aggregator has
 *      not written a status row for it yet.
 *
 * (2) is routine, not an error: `line_status` is populated on the
 * aggregator's own cycle, so every freshly-created custom line has a real
 * gap between "the line exists" and "the line has a status". Treating the
 * 404 as (1) unconditionally is what made a brand-new custom line's own
 * detail page 404 for its owner, even though `GET /public/lines/{id}`
 * returned the whole definition happily.
 *
 * So the 404 is answered here with `'not-computed'` and the page decides
 * between (1) and (2) using the sources that actually know whether the
 * line exists (`getCustomLine`/`getAllLines`, both of which already run on
 * this page for other reasons). Structurally this is the same
 * "row-missing is a state, not a failure" split
 * `app/stations/[crs]/page.tsx` already draws for its three station
 * coverage questions (`fetchStationDisruptions`,
 * `fetchStationSampleStats`, `fetchStationAccessibility`) -- see those
 * helpers' comments.
 *
 * A `200 []` is folded into the same `'not-computed'` state. The backend
 * cannot currently produce one (it 404s instead of returning an empty
 * array), but the previous code indexed `reports[0]` and then read
 * `.name` off it, so if it ever did, the page died on a `TypeError`
 * instead of rendering.
 *
 * Only `ApiNotFoundError` is absorbed. `withStaleFallback` still gets the
 * first crack at every other failure (a 5xx or a dropped connection is
 * served from the stale cache if there's a fresh-enough entry) and
 * anything it rethrows keeps propagating to `app/error.tsx`, exactly as
 * before -- an outage must not be rendered as "no status computed yet". */
type LineStatusResult =
  | { coverage: 'not-computed' }
  | { coverage: 'present'; report: LineStatusReport };

async function fetchLineStatusResult(id: string): Promise<LineStatusResult> {
  let reports;
  try {
    reports = await withStaleFallback(`lineStatus:${id}`, () => getLineStatus([id], true));
  } catch (err) {
    if (err instanceof ApiNotFoundError) return { coverage: 'not-computed' };
    throw err;
  }
  const report = reports[0];
  return report === undefined ? { coverage: 'not-computed' } : { coverage: 'present', report };
}

/** The line's display name, from whichever source actually knows it, or
 * `undefined` when none of them do -- which is the page's real
 * "this line does not exist, or you may not see it" signal now that a
 * missing status row no longer is one.
 *
 * The three sources, in order:
 *
 *   - the status report, when there is one (the pre-existing behaviour,
 *     unchanged for every line that has a status row);
 *   - `GET /public/lines/{id}`, which serves a custom line's full detail
 *     to its owner AND to a member of a group it's shared into, and 404s
 *     for everyone else (including anonymous visitors) -- so it can never
 *     leak a name the caller isn't entitled to;
 *   - `GET /public/lines`, which always lists every catalogue line
 *     (they come from config, not from `line_status`) plus the caller's
 *     own custom lines.
 *
 * Pure, so the "which name wins" rule is testable without a fetch, and
 * shared by the page component and `generateMetadata` so the two cannot
 * disagree about whether a line exists. */
function resolveLineName(
  status: LineStatusResult,
  customLine: CustomLineDetail | null,
  summary: LineSummary | undefined,
): string | undefined {
  if (status.coverage === 'present') return status.report.name;
  return customLine?.name ?? summary?.name;
}

/** Companion to `resolveLineName` for the operator list, same sources in
 * the same order, plus `GET /public/lines/{id}/definition` last (it has no
 * name to contribute, so it plays no part in the existence check above,
 * but it does know the operators for a catalogue line). `[]` when nothing
 * knows -- the caller renders no operators line at all rather than an
 * empty one. */
function resolveLineOperators(
  status: LineStatusResult,
  customLine: CustomLineDetail | null,
  summary: LineSummary | undefined,
  definition: LineDefinitionSummary | null,
): string[] {
  if (status.coverage === 'present') return status.report.operators;
  return customLine?.operators ?? summary?.operators ?? definition?.operators ?? [];
}

/** The three pieces of no-status-yet copy, kept together so they stay
 * consistent with each other. Deliberately none of them says "Good
 * Service": `worstStatus` synthesises a severity-10 Good Service reading
 * for a report with no statuses, which is the right answer for a line the
 * aggregator has looked at and found nothing wrong with, and exactly the
 * wrong one for a line it has never looked at. */
const NO_STATUS_BADGE = 'No status yet';
const NO_STATUS_SUMMARY = 'no status computed yet';
const NO_STATUS_BODY =
  'No status has been computed for this line yet. It appears here once the aggregator has run a cycle covering it.';

/** Per-page Open Graph/Twitter/`<title>` metadata for a shared line link.
 * Fetches the same `getLineStatus([id], true)` call (via the same
 * `withStaleFallback` key) the page component itself makes -- Next's fetch
 * request memoization dedupes the two into one network call per request
 * (see the equivalent, more detailed comment on
 * `app/train/[uid]/[date]/page.tsx`'s own `generateMetadata`; the
 * reasoning is identical here), so no extra caching wrapper is needed.
 * Same existence test as the page component, via the same
 * `fetchLineStatusResult`/`resolveLineName` helpers, since
 * `generateMetadata` runs independently of it and its `notFound()` would
 * 404 the route on its own -- a page fix alone would have been silently
 * undone from here.
 *
 * The extra `getCustomLine`/`getAllLines` calls only happen on the
 * no-status-row path, so the overwhelmingly common case (a line with a
 * status) still costs exactly the one deduped status fetch it did before.
 * A bogus `/lines/{id}` does now cost more than the single status call it
 * used to, here and in the page component both -- that is the price of
 * telling "no status yet" apart from "no such line" at all, and both are
 * indexed single-row/whole-list reads, but it is worth knowing about if
 * this route ever needs rate limiting. */
export async function generateMetadata({
  params,
}: {
  params: Promise<{ id: string }>;
}): Promise<Metadata> {
  const { id } = await params;

  const statusResult = await fetchLineStatusResult(id);

  if (statusResult.coverage === 'not-computed') {
    // Exactly the page component's own two probes, with exactly its two
    // failure policies, so the two halves of this route cannot disagree
    // about whether the line exists:
    //   - `getCustomLine` swallows every failure (it is an
    //     ownership/existence probe whose 401 and 404 are already
    //     indistinguishable -- see the page's own long comment on it);
    //   - `getAllLines` does NOT. If the list is unreachable we genuinely
    //     do not know whether this id is a catalogue line, and a 404 would
    //     be a confident answer we don't have. Letting it throw sends the
    //     route to app/error.tsx's retrying state instead, which is what
    //     the page component does with the same failure.
    let customLine: CustomLineDetail | null = null;
    try {
      customLine = await getCustomLine(id);
    } catch {
      // swallowed -- see above
    }
    const summary = (await withStaleFallback('allLines', () => getAllLines())).find((line) => line.id === id);
    const name = resolveLineName(statusResult, customLine, summary);
    if (name === undefined) {
      notFound();
    }
    const title = `${name} — Distant Signal`;
    const description = `${name}: ${NO_STATUS_SUMMARY}`;
    return {
      title,
      description,
      openGraph: { title, description, type: 'website' },
      twitter: { card: 'summary', title, description },
    };
  }

  const report = statusResult.report;
  const worst = worstStatus(report);
  const title = `${report.name} — Distant Signal`;
  const description = worst.reason
    ? `${report.name}: ${severityLabel(worst.statusSeverity)} — ${worst.reason}`
    : `${report.name}: ${severityLabel(worst.statusSeverity)}`;

  return {
    title,
    description,
    openGraph: { title, description, type: 'website' },
    twitter: { card: 'summary', title, description },
  };
}

export default async function LineDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  // No CIF-derived schedule population is ever published for a TfL line
  // (`schedule-reference`'s own `lines_to_publish` only ever iterates the
  // catalogue TOML, `app.config.lines` -- TfL lines come from
  // `queries::tfl_line_summaries` instead, never fed into that writer at
  // all). A guaranteed 404 from `getLineTrains` is harmless (LineTrainsResults
  // renders its own honest "not available" state for it) but pointless --
  // skip the fetch and the panel entirely for this id shape. Same literal
  // `'tfl-'` prefix convention `lib/modes.ts` already uses for its own
  // TfL-adjacent line list; there is no shared Rust->TypeScript constant to
  // import `common::TFL_LINE_ID_PREFIX` from.
  const isTflLine = id.startsWith('tfl-');

  // A 404 here no longer 404s the page by itself -- see
  // `fetchLineStatusResult`'s comment. `withStaleFallback` still rethrows
  // `ApiNotFoundError` unconditionally (a deleted line is a real
  // application state, not a connectivity failure) and every other failure
  // still propagates; all that changed is who decides what the 404 means.
  const statusResult = await fetchLineStatusResult(id);

  // Category only exists on `LineSummary` (from `getAllLines`), not on the
  // `LineStatusReport` this page otherwise relies on.
  // Same 'allLines' key as /lines -- one shared entry for one shared request.
  const lines = await withStaleFallback('allLines', () => getAllLines());
  const summary = lines.find((line) => line.id === id);
  const category = summary?.category;

  // Hour-cached reference data used only to resolve the "Operators" row's
  // ATOC codes to names below; an empty list degrades to bare codes rather
  // than the whole page -- same pattern as `app/lines/page.tsx`'s and
  // `app/stations/[crs]/page.tsx`'s own `getAllTocs()` calls (review §2.9:
  // a raw ATOC code like "GR" is exactly the kind of internal token this
  // page must not surface unresolved).
  const tocs = tocNameLookup(await getAllTocs().catch(() => []));

  // `getCustomLine` 404s for a catalogue-line id (the endpoint only ever
  // reads the `custom_lines` table) — that expected 404 is how this page
  // tells a custom line apart from a catalogue one, without needing a
  // second "is this custom" field on the status endpoint itself.
  //
  // `isCustom` is ONLY the catalogue/custom distinction again. It used to
  // double as the ownership gate, on the (then correct) grounds that
  // `getCustomLine` collapses a 401 and a 404 into one `ApiNotFoundError`,
  // so a success could only ever be the real owner. Custom-line group
  // sharing
  // (docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md)
  // ended that: a member of a group the owner shared the line into now
  // gets a `200` here too, with the same full detail. Ownership is
  // therefore read from the response's own `isOwner` flag below, and that
  // -- not "the fetch succeeded" -- is what gates Edit/Delete.
  // There's deliberately no separate "please log in, this might be yours"
  // prompt here the way the tracked-train pages have (see
  // frontend/app/train/[uid]/[date]/page.tsx,
  // frontend/app/train/by-id/[trackingId]/page.tsx, and their own
  // comments): unlike a single train someone is tracking, a line id is, by
  // far, most often a public catalogue line that a random visitor has no
  // reason to think they own, so folding both "anonymous" and "not the
  // owner" into one plain 404-shaped "no controls for you" is the better
  // default here, not an inconsistency to fix.
  //
  // Every failure mode collapses to `isCustom = false`, not just
  // ApiNotFoundError. `getCustomLine` maps only 401/404 to that class
  // (lib/api.ts) -- a rejected fetch or a 5xx arrives as a plain Error, and
  // rethrowing it here sent this page straight to app/error.tsx, blanking
  // it during exactly the backend outage this feature exists to survive.
  // Failing closed is also the safe direction on its own terms: both
  // `isCustom` and `viewerOwnsLine` stay `false` when we could not confirm
  // anything, so owner controls are never rendered on a guess.
  let isCustom = true;
  let viewerOwnsLine = false;
  let sharedWithGroups: LineGroupRef[] = [];
  let customLine: CustomLineDetail | null = null;
  try {
    customLine = await getCustomLine(id);
    viewerOwnsLine = customLine.isOwner;
    sharedWithGroups = customLine.sharedWithGroups;
  } catch {
    isCustom = false;
  }

  // A custom line has exactly the same "no schedule population, ever"
  // property as a TfL line, per the same get_line_schedule doc-comment
  // finding above ("an unknown, custom, or not-yet-published catalogue id
  // alike simply 404s") -- skip the panel for both, for the same reason.
  const showTrainsPanel = !isCustom && !isTflLine;

  // A tooltip showing stations/operators is a nice-to-have, not core page
  // functionality — if this fails for any reason, just don't show it
  // rather than breaking the whole page over it. Works for both catalogue
  // and custom lines (unlike `getCustomLine` above, `getLineDefinition`
  // doesn't 404 for a catalogue id — see its backend doc comment for why
  // that endpoint stays separate from this one).
  let definition = null;
  try {
    definition = await getLineDefinition(id);
  } catch {
    // swallowed — see comment above
  }

  // THE existence check for this page, and the only one. It deliberately
  // sits after all three fetches above rather than straight after the
  // status fetch, because "does this line exist, for this viewer" is a
  // question `/Line/{id}/Status` alone cannot answer -- see
  // `fetchLineStatusResult` and `resolveLineName`.
  //
  // What each viewer of a status-less custom line gets, therefore:
  //   - its owner: a name from `getCustomLine` (and from `getAllLines`) --
  //     the whole page, with the no-status state below;
  //   - a member of a group it's shared into: a name from `getCustomLine`
  //     -- the same page, minus the owner controls, as always;
  //   - anyone else, signed in or not: `getCustomLine` 404s (it collapses
  //     401 into the same `ApiNotFoundError`) and `getAllLines` omits it,
  //     so no name, so `notFound()` -- a private line stays as invisible
  //     as it was before this change.
  // A genuinely unknown id fails every source the same way, and 404s.
  const name = resolveLineName(statusResult, customLine, summary);
  if (name === undefined) {
    notFound();
  }
  const operators = resolveLineOperators(statusResult, customLine, summary, definition);

  // Stamped server-side so IssueList's buckets don't depend on a
  // `Date.now()` that differs between the SSR pass and hydration. Fresh on
  // every request (this route is dynamic) and re-stamped by AutoRefresh.
  const now = Date.now();

  // A fixed rolling 24-hour window, not a URL-driven preset -- this embed
  // has no range picker of its own (Decision 11 of
  // docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md).
  // "View history" below remains the way to reach the full range picker
  // and the daily Trends tab.
  const trendsRange = resolveHalfHourlyRange(now);

  // The same rail day used both to fetch LineTrainsResults' data and to
  // build every one of its rows' `/train/{uid}/{date}` links -- computed
  // once so the two can never disagree (see LineTrainsResults' own doc
  // comment). London-day, not a bare UTC day: this app's stated
  // network-time convention (lib/dateFormat.ts's own module doc).
  const trainsDate = londonDayKey(new Date(now));

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>{name}</Title>
        {/* Task 3.4.6: the ⓘ tooltip used to sit next to the title in its
            own `Group`, which on mobile wraps independently of this outer
            `Group` -- a long line name alone could already fill the row,
            leaving the small ⓘ button to wrap onto its own orphaned line
            underneath. Moved in with Edit/Delete/Share/the status badge
            instead: that cluster is already right-aligned and already
            wraps as one unit, so ⓘ now wraps down together with it rather
            than alone. */}
        <Group gap="sm">
          {/* Gated on the response's own `isOwner`, NOT on `isCustom`.
              Those were the same thing until custom-line group sharing
              made a `200` from `getCustomLine` reachable for a granted
              non-owner too (see the comment on that call above). A
              granted member who saw Edit/Delete here would be offered
              controls whose only possible outcome is a 404 -- on someone
              else's private line, which also reads as "this is mine". The
              backend refuses them regardless (`update_line`/`delete_line`
              are owner-only and completely grant-blind); this is the
              never-render-a-control-that-can-only-fail half. */}
          {isCustom && viewerOwnsLine && (
            <>
              {/* Plain `<Link>` wrapping `Button`, not `component={Link}`
                  on a Mantine polymorphic prop — this page is a Server
                  Component, and that pattern previously broke
                  `next build`'s Server/Client boundary check (see
                  LineStatusCard's fix). */}
              <Link href={`/lines/${id}/edit`} style={{ textDecoration: 'none' }}>
                <Button variant="outline" size="xs">Edit</Button>
              </Link>
              <DeleteLineButton id={id} />
            </>
          )}
          {definition && <LineDefinitionTooltip stations={definition.stations} operators={definition.operators} />}
          <ShareButton />
          {/* `StatusBadge` only ever renders a real, computed severity.
              With no `line_status` row there is no severity to show, and
              `worstStatus`'s synthetic Good Service stand-in would be an
              outright false claim about a line nothing has assessed yet --
              so the badge is replaced, not fed a default.

              Gray/light rather than a filled severity colour, so it reads
              as the absence of a status rather than as one more severity;
              it is also the one `variant="light"` pairing `app/globals.css`
              measured as needing no correction at all (gray 9 on gray 1,
              13.87:1). `data-status-badge` is the same `app/globals.css`
              ellipsis-opt-out hook `StatusBadge` carries. */}
          {statusResult.coverage === 'present' ? (
            <StatusBadge severity={worstStatus(statusResult.report).statusSeverity} />
          ) : (
            <Badge color="gray" variant="light" data-status-badge>
              {NO_STATUS_BADGE}
            </Badge>
          )}
        </Group>
      </Group>
      {category && <Text c="dimmed">Category: {categoryLabel(category)}</Text>}
      {/* Explains to a granted group member why they can see a line that
          isn't theirs and has no edit controls -- without this the page
          just silently lacks the buttons an owner would have. Deliberately
          names no group: which of the owner's groups this line reaches the
          viewer through is the owner's business (see
          `CustomLineDetail.sharedWithGroups`), and the home page's "Lines
          shared with you" section already carries the "from <group>" tag
          for the groups this viewer is actually in. */}
      {isCustom && !viewerOwnsLine && (
        <Text c="dimmed">Shared with you through a group. Only its owner can edit it.</Text>
      )}
      {isCustom && viewerOwnsLine && sharedWithGroups.length > 0 && (
        <Text c="dimmed">
          Shared with:{' '}
          {sharedWithGroups.map((group, index) => (
            <span key={group.id}>
              {index > 0 && ', '}
              <Link href={`/groups/${group.id}`}>{group.name}</Link>
            </span>
          ))}
        </Text>
      )}
      {/* Hidden rather than rendered empty: with no status row, operators
          come from the line's own definition instead, and a line whose
          definition is also unreachable has nothing honest to put here. */}
      {operators.length > 0 && (
        <Text c="dimmed">Operators: {operators.map((code) => operatorLabel(code, tocs)).join(', ')}</Text>
      )}
      <TextLink href={`/lines/${id}/history`} underline="always">
        View history
      </TextLink>
      {/* The status section proper. Everything above and below it -- name,
          category, sharing, operators, owner controls, history link, the
          embedded trend charts -- describes the line's definition and
          renders whether or not a status exists; only this part depends on
          a `line_status` row, so only this part degrades when there isn't
          one yet. The "no status" branch is deliberately one plain
          sentence, not a disabled-looking empty IssueList: there is
          nothing to filter, expand, or come back to here. */}
      {statusResult.coverage === 'present' ? (
        <>
          <RepresentativeInfo statuses={statusResult.report.lineStatuses} />
          {/* Every issue here belongs to the line already named in the
              heading, so no per-issue line attribution is needed — that's
              what the optional `lines` on IssueItem is for on the station
              page. */}
          <IssueList items={statusResult.report.lineStatuses.map((status) => ({ status }))} now={now} />
        </>
      ) : (
        <Text c="dimmed">{NO_STATUS_BODY}</Text>
      )}
      {statusResult.coverage === 'present' && statusResult.report.tflStatus && statusResult.report.tflStatus.length > 0 && (
        <Stack gap="xs">
          {/* This line has an NR counterpart merged into it (Elizabeth line
              today -- see docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
              Area 1) and this is TfL's own, separately-sourced view of the
              same railway. Kept visually distinct from the primary IssueList
              above rather than merged into one list, since only the primary
              side has real sampleStats and merging would blur that. */}
          <Text fw={500}>TfL also reports:</Text>
          <IssueList items={statusResult.report.tflStatus.map((status) => ({ status }))} now={now} />
        </Stack>
      )}
      {/* Skipped entirely (no heading, no fetch) for a TfL line or a custom
          line -- both guarantee a 404 from `getLineTrains` (see
          `isTflLine`/`showTrainsPanel` above), so there is nothing honest
          this panel could show for either. Its own Suspense boundary, same
          rationale as the trend charts' below: `getLineTrains` is a
          separate, comparatively slow fetch that must not block the rest
          of the page behind it. */}
      {showTrainsPanel && (
        <Stack gap="xs">
          <Title order={2} size="h4">
            Trains running today
          </Title>
          <Suspense fallback={<TrainsLoadingFallback />}>
            <LineTrainsResults id={id} date={trainsDate} now={new Date(now)} />
          </Suspense>
        </Stack>
      )}
      <Stack gap="xs">
        <Title order={2} size="h4">
          Recent trends (last 24 hours)
        </Title>
        {/* Half-hourly (30-minute buckets), not the dedicated history
            page's daily rollup -- Decision 1/2 of
            docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md
            (written for the original 1-hour bucket; the reasoning is
            unchanged at 30 minutes): a rolling 24-hour window needs
            intra-day resolution the daily table can't provide, so this
            renders through a new, separate half-hourly fetch/component
            (HalfHourlyTrendsResults, formerly HourlyTrendsResults) rather
            than TrendsResults. It still shares TrendsCharts -- the actual
            chart rendering, legend, dash patterns, gap bands, edge padding
            -- with the dedicated history page's daily Trends tab; only the
            fetch, sparse-data floor, and copy are half-hourly-specific.
            `View history` above remains the way to reach the full range
            picker, the Timeline tab, and the daily Trends tab.

            Wrapped in its own Suspense boundary, same rationale as before:
            `getLineHalfHourlyStats` is comparatively slow, and without this
            boundary it would block the whole page behind a chart a visitor
            may not even scroll down to see. A brand-new line with no
            half-hourly-stats rows yet still resolves fast:
            `HalfHourlyTrendsResults` renders its own "Not enough sampled
            data yet" text rather than leaving this section hanging. */}
        <Suspense fallback={<TrendsLoadingFallback />}>
          <HalfHourlyTrendsResults id={id} from={trendsRange.from} to={trendsRange.to} />
        </Suspense>
        {/* The half-hourly full-coverage series (Decision 1 of
            docs/superpowers/specs/2026-09-03-half-hourly-coverage-trends-design.md)
            -- a second, separate section under the sample-derived one
            above, sharing the same trendsRange rather than computing its
            own "now" a few milliseconds later. Its own, separate Suspense
            boundary (Decision 4 of that design doc), so a slow coverage
            fetch never blocks the sample chart above it, and vice versa. */}
        <Suspense fallback={<TrendsLoadingFallback />}>
          <HalfHourlyCoverageTrendsResults id={id} from={trendsRange.from} to={trendsRange.to} />
        </Suspense>
      </Stack>
    </Stack>
  );
}
