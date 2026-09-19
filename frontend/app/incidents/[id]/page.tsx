import { notFound } from 'next/navigation';
import { Badge, Divider, Group, Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { ApiNotFoundError, getAllTocs, getIncident, getStationName } from '@/lib/api';
import { sanitizeDescription } from '@/lib/sanitizeHtml';
import { ShareButton } from '@/components/ShareButton';
import { TextLink } from '@/components/TextLink';
import { formatDateTime, TIMES_IN_UK_LOCAL_TIME } from '@/lib/dateFormat';
import { operatorLabel, tocNameLookup } from '@/lib/displayLabels';
import { stationLabel } from '@/lib/stationLabel';
import type { IncidentDetail, IncidentHistoryEntry, ValidityPeriod } from '@/lib/types';

// Same rationale as every dynamic `[param]` route in this app: without
// this, `next build` may try to prerender against a database that only
// exists on the compose network at runtime. (Note: `/lines/[id]/page.tsx`
// and `/stations/[crs]/page.tsx` — the two structurally closest existing
// pages — do NOT declare this explicitly and still render dynamically, so
// this may be a no-op in practice; added anyway for explicitness, matching
// `/lines/[id]/history/page.tsx`'s and the dashboard's convention.)
export const revalidate = 0;

function formatValidityPeriod(period: ValidityPeriod): string {
  const from = formatDateTime(period.fromDate);
  return period.toDate ? `${from} – ${formatDateTime(period.toDate)}` : `${from} – ongoing`;
}

/** Which of a history entry's fields differ from the entry immediately
 * after it in the (newest-first) list — a short textual diff summary
 * rather than a full field dump every time, since most consecutive
 * snapshots differ in only one or two fields. `older` is `undefined` for
 * the oldest entry (nothing to diff against — it's the incident's
 * first-seen snapshot). */
function describeChanges(entry: IncidentHistoryEntry, older: IncidentHistoryEntry | undefined): string {
  if (!older) return 'First seen';
  const changes: string[] = [];
  if (entry.summary !== older.summary) changes.push('summary changed');
  if (entry.description !== older.description) changes.push('description changed');
  if (entry.priority !== older.priority) changes.push(`priority changed from ${older.priority} to ${entry.priority}`);
  if (JSON.stringify(entry.validityPeriods) !== JSON.stringify(older.validityPeriods)) changes.push('validity changed');
  if (entry.isPlanned !== older.isPlanned) changes.push(`isPlanned changed to ${entry.isPlanned}`);
  if (entry.isCleared !== older.isCleared) changes.push(`isCleared changed to ${entry.isCleared}`);
  return changes.length > 0 ? changes.join(', ') : 'Re-confirmed, no change';
}

/** Per-page Open Graph/Twitter/`<title>` metadata for a shared incident
 * link. Fetches the same `getIncident(id)` call the page component makes
 * -- Next's fetch request memoization dedupes the two into one network
 * call per request, same reasoning as the equivalent, more detailed
 * comment on `app/train/[uid]/[date]/page.tsx`'s `generateMetadata`. Same
 * `notFound()`-on-`ApiNotFoundError` handling as the page component. */
export async function generateMetadata({
  params,
}: {
  params: Promise<{ id: string }>;
}): Promise<Metadata> {
  const { id } = await params;

  let incident: IncidentDetail;
  try {
    incident = await getIncident(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  const title = `${incident.summary} — Distant Signal`;
  const kind = incident.isPlanned ? 'Planned Work' : 'Real-Time';
  const affectedLines = incident.currentlyAffectsLines.map((line) => line.name);
  const description =
    affectedLines.length > 0
      ? `${kind} incident affecting ${affectedLines.join(', ')}.`
      : `${kind} incident: ${incident.summary}.`;

  return {
    title,
    description,
    openGraph: { title, description, type: 'website' },
    twitter: { card: 'summary', title, description },
  };
}

export default async function IncidentDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;

  let incident: IncidentDetail;
  try {
    incident = await getIncident(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  // Both reference-data lookups are hour-cached (`getAllTocs`) or per-code
  // hour-cached (`getStationName`) -- see their own doc comments in
  // `lib/api.ts` -- and both degrade to the bare code on failure rather
  // than taking the whole page down, same posture as every other caller of
  // either (`app/lines/[id]/page.tsx`, `app/page.tsx`'s pinned-station
  // names). Fetched in parallel with each other; stations are also fetched
  // in parallel with one another, since a single incident can carry several.
  const [tocs, stationNames] = await Promise.all([
    getAllTocs().catch(() => []),
    Promise.all(
      incident.affectedStations.map((crs) => getStationName(crs).catch(() => null)),
    ),
  ]);
  const tocLookup = tocNameLookup(tocs);
  const stationNamesByCrs = new Map(incident.affectedStations.map((crs, i) => [crs, stationNames[i]]));

  return (
    <Stack p="lg" gap="md">
      {/* Review §3.3: the detail page was a dead end -- no back link or
          breadcrumb, and the global nav was the only way out. This is the
          page most likely to be reached from a shared URL with no browser
          history to go back to. */}
      <TextLink href="/incidents" underline="always">
        ← Incident Archive
      </TextLink>

      {/* Review §3.3: a ten-word Knowledgebase summary used to wrap to four
          lines at full `h1` size before the badges even appeared, consuming
          ~40% of the first mobile viewport. `size="h2"` plus a 3-line clamp
          shrinks that footprint; `title` keeps the full text one hover (or
          screen-reader "title" exposure) away for the rare case a summary
          is clamped. The badges/share row is intentionally its own `Group`
          below the title rather than sharing the title's -- see that
          `Group`'s own comment for why. */}
      <Title order={1} size="h2" lineClamp={3} title={incident.summary}>
        {incident.summary}
      </Title>

      {/* Review §3.3 ("small things"): the share `ActionIcon` used to sit
          alone on its own row between the badge and the body, because the
          old `Group justify="space-between"` wrapped once the `h1` spanned
          the container -- the title lived in that same `Group`. Moving the
          title out to its own element above (rather than reordering this
          `Group`'s children) removes the thing that was forcing the wrap in
          the first place, so this `justify="space-between"` (unchanged from
          before) now reliably keeps the badges and the share button on one
          row instead of wrapping the button onto its own. */}
      <Group justify="space-between">
        <Group gap="sm">
          <Badge color={incident.isPlanned ? 'blue' : 'orange'}>{incident.isPlanned ? 'Planned Work' : 'Real-Time'}</Badge>
          {/* Review §3.3's "at-a-glance strip": the archive rows' own
              Active/Cleared badge, reused verbatim (same colors, same
              copy) so a reader who has seen the archive recognises it
              immediately here. */}
          <Badge color={incident.isCleared ? 'gray' : 'green'}>{incident.isCleared ? 'Cleared' : 'Active'}</Badge>
        </Group>
        <ShareButton />
      </Group>

      {/* Review §3.3: operators were previously discoverable only by
          reading the free-text description -- `incident.operators` (in the
          API response, and already rendered on every archive row) never
          appeared on this page at all. `operatorLabel` resolves each ATOC
          code through the same TOC-name lookup `app/lines/[id]/page.tsx`
          already uses, degrading to the bare code for one the reference
          table doesn't (yet) know. Hidden rather than rendered empty: an
          incident with no attributed operator has nothing honest to put
          here, matching `app/lines/[id]/page.tsx`'s identical convention
          for this same field. */}
      {incident.operators.length > 0 && (
        <Text c="dimmed">Operators: {incident.operators.map((code) => operatorLabel(code, tocLookup)).join(', ')}</Text>
      )}

      {/* `data-rich-text`: see the identical hook and full rationale on
          `components/DisruptionDetail.tsx`'s sanitized-HTML container. */}
      <div data-rich-text dangerouslySetInnerHTML={{ __html: sanitizeDescription(incident.description) }} />

      {incident.affectedStations.length > 0 && (
        <Stack gap={4}>
          {/* Review §3.3: labelled with names *and* codes now, not just a
              bare CRS in a `title` tooltip nobody can see without hovering
              -- review §2.9's "unlabelled CRS pills ... floating in the
              page". `stationLabel` is the same "Name (CRS)"/bare-code
              fallback every other station reference on this app already
              uses. */}
          <Title order={2} size="h5">
            Affected stations
          </Title>
          <Group gap="xs">
            {incident.affectedStations.map((crs) => (
              <Badge key={crs} variant="outline" color="gray">
                {stationLabel(crs, stationNamesByCrs.get(crs))}
              </Badge>
            ))}
          </Group>
        </Stack>
      )}

      <Stack gap={4}>
        {/* Review §3.3 (a11y): "Validity"/"Currently affects"/"History" used
            to be `Text fw={500}`, not real headings, so the document
            outline was a lone `<h1>` and nothing else. `Title order={2}
            size="h5"` is visually identical to the old bold text. */}
        <Title order={2} size="h5">
          Validity
        </Title>
        {incident.validityPeriods.map((period, i) => (
          <Text key={i} size="sm" c="dimmed">
            {formatValidityPeriod(period)}
          </Text>
        ))}
      </Stack>

      <Divider />

      {/* Review §3.3's at-a-glance strip, second half: a cleared incident
          with an "ongoing" validity period (common -- see the stale-incident
          spec) used to still render "Currently affects" the same way a live
          one does, which reads as a contradiction next to a "REAL-TIME"
          badge nobody can reconcile with "cleared" unless they already know
          what "not currently reported on any tracked line" means. Once
          cleared, this whole section says so instead of listing lines that
          are, definitionally, no longer meaningfully "current". */}
      {incident.isCleared ? (
        <Text size="sm" c="dimmed">
          This incident has been cleared.
        </Text>
      ) : (
        <Stack gap={4}>
          <Title order={2} size="h5">
            Currently affects
          </Title>
          {incident.currentlyAffectsLines.length === 0 ? (
            <Text size="sm" c="dimmed">
              Not currently reported on any tracked line.
            </Text>
          ) : (
            <Group gap="md">
              {incident.currentlyAffectsLines.map((line) => (
                <TextLink key={line.id} href={`/lines/${line.id}`}>
                  {line.name}
                </TextLink>
              ))}
            </Group>
          )}
        </Stack>
      )}

      <Divider />

      <Stack gap="xs">
        <Title order={2} size="h5">
          History
        </Title>
        {/* Review §3.3: the detail-page spec assumed history "always has at
            least the first-seen snapshot", but a freshly ingested incident
            can be caught between ingest and its first change -- and the
            fixture demonstrates it does happen. */}
        {incident.history.length === 0 ? (
          <Text size="sm" c="dimmed">
            No changes recorded since this incident was first seen.
          </Text>
        ) : (
          incident.history.map((entry, i) => (
            <Stack key={i} gap={2}>
              <Text size="sm">{formatDateTime(entry.recordedAt)}</Text>
              <Text size="sm" c="dimmed">
                {describeChanges(entry, incident.history[i + 1])}
              </Text>
            </Stack>
          ))
        )}
      </Stack>

      <Divider />

      <Stack gap={2}>
        <Text size="xs" c="dimmed">
          First seen: {formatDateTime(incident.firstSeenAt)}
        </Text>
        <Text size="xs" c="dimmed">
          Last updated from National Rail: {formatDateTime(incident.fetchedAt)}
        </Text>
        <Text size="xs" c="dimmed">
          {TIMES_IN_UK_LOCAL_TIME}
        </Text>
      </Stack>
    </Stack>
  );
}
