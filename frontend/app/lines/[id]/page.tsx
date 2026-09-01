import { Suspense } from 'react';
import { notFound } from 'next/navigation';
import { Stack, Title, Text, Group, Button, Skeleton } from '@mantine/core';
import Link from 'next/link';
import { ApiNotFoundError, getLineStatus, getCustomLine, getLineDefinition, getAllLines } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { DeleteLineButton } from '@/components/DeleteLineButton';
import { LineDefinitionTooltip } from '@/components/LineDefinitionTooltip';
import { TextLink } from '@/components/TextLink';
import { worstStatus } from '@/lib/severity';
import { resolveRange } from '@/lib/history';
import { TrendsResults } from './history/TrendsResults';

// Same `revalidate = 0` rationale as `/lines/[id]/history` -- this page now
// also computes a range off `Date.now()` (`resolveRange` below), so it must
// stay dynamic rather than be eligible for build-time prerendering.
export const revalidate = 0;

export default async function LineDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  let reports;
  try {
    reports = await getLineStatus([id], true);
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
  const lines = await getAllLines();
  const category = lines.find((line) => line.id === id)?.category;

  // `getCustomLine` 404s for a catalogue-line id (the endpoint only ever
  // reads the `custom_lines` table) — that expected 404 is how this page
  // tells a custom line apart from a catalogue one, without needing a
  // second "is this custom" field on the status endpoint itself.
  //
  // `isCustom` alone is now also the *ownership* gate below, not just the
  // catalogue/custom distinction — `getCustomLine` collapses a 401
  // (not logged in) and a 404 (logged in but not the owner, or truly
  // unknown id) into the same `ApiNotFoundError` (Task 10 Step 2 /
  // Decision 8 of docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md).
  // So by the time this catch block finishes, either `isCustom` is `false`
  // (never render Edit/Delete), or it's `true` *and* the call above
  // actually succeeded for this specific caller with this specific
  // cookie — which, given that collapse, only happens for the real owner.
  // There's deliberately no separate "please log in, this might be yours"
  // prompt here the way the tracked-train pages have (see
  // frontend/app/train/[uid]/[date]/page.tsx,
  // frontend/app/train/by-id/[trackingId]/page.tsx, and their own
  // comments): unlike a single train someone is tracking, a line id is, by
  // far, most often a public catalogue line that a random visitor has no
  // reason to think they own, so folding both "anonymous" and "not the
  // owner" into one plain 404-shaped "no controls for you" is the better
  // default here, not an inconsistency to fix.
  let isCustom = true;
  try {
    await getCustomLine(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      isCustom = false;
    } else {
      throw err;
    }
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

  // The default (no query params) 7-day window `/lines/[id]/history` itself
  // falls back to -- same `resolveRange` call, so this embedded preview and
  // "View history"'s own default page land on identical numbers rather than
  // two independently-computed "last 7 days". Passing `{}` (this page has
  // no range-picker query params of its own) always resolves the 7d preset.
  const trendsRange = resolveRange({}, now);

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Group gap="xs">
          <Title order={1}>{report.name}</Title>
          {definition && <LineDefinitionTooltip stations={definition.stations} operators={definition.operators} />}
        </Group>
        <Group gap="sm">
          {/* Gated on `isCustom` alone -- no separate `isOwner` check needed
              here. By the time this line is reached, `getCustomLine` has
              already either thrown (so `isCustom` is `false`) or succeeded
              for this exact caller/cookie, and its 401-collapses-into-404
              behavior (Task 10 Step 2 / Decision 8, see the comment on the
              `getCustomLine` call above) means the only way it can succeed
              is for the real owner. There's no remaining path where
              `isCustom` is `true` and the viewer isn't the owner, so
              `isCustom` is now the whole gate. */}
          {isCustom && (
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
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
      </Group>
      {category && <Text c="dimmed">Category: {category}</Text>}
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
          Recent trends (last 7 days)
        </Title>
        {/* Reuses `/lines/[id]/history`'s own Trends-tab component wholesale
            -- same fetch, same sparse-data-floor gap handling, same honesty
            copy about what the rates actually mean -- rather than
            re-deriving any of that here. `View history` above remains the
            way to reach the full range picker, the Timeline tab, and
            ranges longer than this fixed 7-day preview.

            Wrapped in its own Suspense boundary, same rationale as the
            history page's: `getLineDailyStats` is comparatively slow, and
            without this boundary it would block the whole page -- status,
            issues, everything above -- behind a chart a visitor may not
            even scroll down to see. A brand-new line with no daily-stats
            rows yet (Task 1's "stuck loading" fix made that reachable
            moments after creation) still resolves fast: `TrendsResults`
            renders its own "Not enough sampled data yet" text rather than
            leaving this section hanging. */}
        <Suspense fallback={<Skeleton height={280} />}>
          <TrendsResults id={id} from={trendsRange.from} to={trendsRange.to} />
        </Suspense>
      </Stack>
    </Stack>
  );
}
