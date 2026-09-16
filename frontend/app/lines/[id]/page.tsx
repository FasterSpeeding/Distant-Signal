import { Suspense } from 'react';
import { notFound } from 'next/navigation';
import { Stack, Title, Text, Group, Button, Skeleton } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { ApiNotFoundError, getLineStatus, getCustomLine, getLineDefinition, getAllLines } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { DeleteLineButton } from '@/components/DeleteLineButton';
import { LineDefinitionTooltip } from '@/components/LineDefinitionTooltip';
import { ShareButton } from '@/components/ShareButton';
import { TextLink } from '@/components/TextLink';
import { worstStatus, severityLabel } from '@/lib/severity';
import { resolveHalfHourlyRange } from '@/lib/history';
import type { LineGroupRef } from '@/lib/types';
import { HalfHourlyTrendsResults } from './history/HalfHourlyTrendsResults';
import { HalfHourlyCoverageTrendsResults } from './history/HalfHourlyCoverageTrendsResults';

// Same `revalidate = 0` rationale as `/lines/[id]/history` -- this page now
// also computes a range off `Date.now()` (`resolveRange` below), so it must
// stay dynamic rather than be eligible for build-time prerendering.
export const revalidate = 0;

/** Per-page Open Graph/Twitter/`<title>` metadata for a shared line link.
 * Fetches the same `getLineStatus([id], true)` call (via the same
 * `withStaleFallback` key) the page component itself makes -- Next's fetch
 * request memoization dedupes the two into one network call per request
 * (see the equivalent, more detailed comment on
 * `app/train/[uid]/[date]/page.tsx`'s own `generateMetadata`; the
 * reasoning is identical here), so no extra caching wrapper is needed.
 * Same `notFound()`-on-`ApiNotFoundError` handling as the page component,
 * since `generateMetadata` runs independently of it and needs its own
 * equivalent try/catch. */
export async function generateMetadata({
  params,
}: {
  params: Promise<{ id: string }>;
}): Promise<Metadata> {
  const { id } = await params;

  let reports;
  try {
    reports = await withStaleFallback(`lineStatus:${id}`, () => getLineStatus([id], true));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  const report = reports[0];
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

  // Composed with the existing ApiNotFoundError catch rather than
  // replacing it: withStaleFallback rethrows ApiNotFoundError
  // unconditionally (a deleted line is a real application state, not a
  // connectivity failure), so the notFound() branch keeps working.
  let reports;
  try {
    reports = await withStaleFallback(`lineStatus:${id}`, () => getLineStatus([id], true));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  const report = reports[0];
  const worst = worstStatus(report);

  // Category only exists on `LineSummary` (from `getAllLines`), not on the
  // `LineStatusReport` this page otherwise relies on -- fetched here, after
  // the notFound() check above, so an unknown line id still 404s cleanly.
  // Same 'allLines' key as /lines -- one shared entry for one shared request.
  const lines = await withStaleFallback('allLines', () => getAllLines());
  const category = lines.find((line) => line.id === id)?.category;

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
  try {
    const customLine = await getCustomLine(id);
    viewerOwnsLine = customLine.isOwner;
    sharedWithGroups = customLine.sharedWithGroups;
  } catch {
    isCustom = false;
  }

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

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Group gap="xs">
          <Title order={1}>{report.name}</Title>
          {definition && <LineDefinitionTooltip stations={definition.stations} operators={definition.operators} />}
        </Group>
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
          <ShareButton />
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
      </Group>
      {category && <Text c="dimmed">Category: {category}</Text>}
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
      <Text c="dimmed">Operators: {report.operators.join(', ')}</Text>
      <TextLink href={`/lines/${id}/history`} underline="always">
        View history
      </TextLink>
      <RepresentativeInfo statuses={report.lineStatuses} />
      {/* Every issue here belongs to the line already named in the heading,
          so no per-issue line attribution is needed — that's what the
          optional `lines` on IssueItem is for on the station page. */}
      <IssueList items={report.lineStatuses.map((status) => ({ status }))} now={now} />
      {report.tflStatus && report.tflStatus.length > 0 && (
        <Stack gap="xs">
          {/* This line has an NR counterpart merged into it (Elizabeth line
              today -- see docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
              Area 1) and this is TfL's own, separately-sourced view of the
              same railway. Kept visually distinct from the primary IssueList
              above rather than merged into one list, since only the primary
              side has real sampleStats and merging would blur that. */}
          <Text fw={500}>TfL also reports:</Text>
          <IssueList items={report.tflStatus.map((status) => ({ status }))} now={now} />
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
        <Suspense fallback={<Skeleton height={280} />}>
          <HalfHourlyTrendsResults id={id} from={trendsRange.from} to={trendsRange.to} />
        </Suspense>
        {/* The half-hourly full-coverage series (Decision 1 of
            docs/superpowers/specs/2026-09-03-half-hourly-coverage-trends-design.md)
            -- a second, separate section under the sample-derived one
            above, sharing the same trendsRange rather than computing its
            own "now" a few milliseconds later. Its own, separate Suspense
            boundary (Decision 4 of that design doc), so a slow coverage
            fetch never blocks the sample chart above it, and vice versa. */}
        <Suspense fallback={<Skeleton height={280} />}>
          <HalfHourlyCoverageTrendsResults id={id} from={trendsRange.from} to={trendsRange.to} />
        </Suspense>
      </Stack>
    </Stack>
  );
}
