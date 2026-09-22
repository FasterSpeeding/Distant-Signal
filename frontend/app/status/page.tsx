import { Badge, Card, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { getLineStatusForMode } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { DISPLAYED_MODES_PARAM, type Country } from '@/lib/modes';
import { buildNetworkStatusOverview } from '@/lib/networkStatusOverview';
import {
  isGoodSeverity,
  SEVERITY_GROUP_COLORS,
  SEVERITY_GROUP_LABELS,
  SEVERITY_GROUPS_BY_RANK,
  severityColor,
  severityRank,
  type SeverityGroup,
  worstStatus,
} from '@/lib/severity';
import { LineStatusCard } from '@/components/LineStatusCard';
import { LastUpdated } from '@/components/LastUpdated';
import type { LineStatusReport } from '@/lib/types';

// Same rationale as every other dynamic route in this app (see
// app/lines/page.tsx's own comment): without this, `next build` treats the
// route as eligible for static generation and tries to prerender it, which
// fails since the `api` service only exists on the compose network at
// runtime.
export const revalidate = 0;

const METADATA_TITLE = 'Network Status — Distant Signal';
const METADATA_DESCRIPTION =
  'A live, network-wide snapshot of every National Rail and TfL line this app tracks: how many are running a Good Service versus facing disruption or a planned closure right now, which lines need attention most, and how that breaks down by mode and by country.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

const COUNTRY_LABELS: Record<Country, string> = {
  Gb: 'GB',
  NorthernIreland: 'Northern Ireland',
  RepublicOfIreland: 'Republic of Ireland',
};

/** Worst-first — the order the tiles below render in (2026-09-22 UX review
 * §2.2: `SEVERITY_GROUPS_BY_RANK` is ascending, so the strip read Good ->
 * Informational -> Planned -> Minor -> Severe, and a traveller asking "is
 * anything wrong?" reached the answer last). Reversing the single shared
 * rank order (rather than hand-listing the five groups here) means this
 * can't drift from `SEVERITY_GROUPS_BY_RANK`/`AllLinesTable`'s own filter
 * chip order if a group is ever added or reordered there. */
const SEVERITY_GROUPS_WORST_FIRST: readonly SeverityGroup[] = [...SEVERITY_GROUPS_BY_RANK].reverse();

export default async function NetworkStatusPage() {
  // Deliberately the SAME cache key `app/page.tsx`/`app/lines/page.tsx`
  // already use (`lineStatusForMode:${DISPLAYED_MODES_PARAM}`) -- it is the
  // same request, so all three pages share one stale-fallback cache entry.
  const allReports = await withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () =>
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
  );
  const overview = buildNetworkStatusOverview(allReports);

  // Honest about which modes are actually present (2026-09-22 UX review
  // §2.3: the subtitle said "across National Rail and TfL" even when TfL
  // had zero lines tracked, directly contradicting the "By mode" card right
  // below it). Falls back to naming both when the snapshot is itself empty
  // (nothing to be dishonest about yet, and "across National Rail and TfL"
  // is still the accurate description of what this page WOULD show).
  const activeModes = [
    overview.byMode.nationalRail.length > 0 ? 'National Rail' : null,
    overview.byMode.tfl.length > 0 ? 'TfL' : null,
  ].filter((mode): mode is string => mode !== null);
  const modesDescription = activeModes.length > 0 ? activeModes.join(' and ') : 'National Rail and TfL';

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="xs">
        <Title order={1}>Network Status</Title>
        <Text c="dimmed">
          {overview.totalLines} line{overview.totalLines === 1 ? '' : 's'} tracked across {modesDescription} right
          now.
        </Text>
        {/* 2026-09-22 UX review §2.5: nothing on this page ever said when
            the data was last updated, despite "right now" and being served
            through `withStaleFallback` (so "right now" can legitimately be
            several minutes old). Omitted entirely for an empty snapshot --
            there is no real timestamp to show yet. */}
        {overview.lastUpdated && <LastUpdated timestamp={overview.lastUpdated} />}
      </Stack>

      <SimpleGrid cols={{ base: 2, sm: 3, lg: 5 }} spacing="md">
        {SEVERITY_GROUPS_WORST_FIRST.map((group) => (
          <SeverityCounterTile key={group} group={group} count={overview.counts[group]} />
        ))}
      </SimpleGrid>

      <WorstLinesSection worstFirst={overview.worstFirst} />

      <Stack gap="md">
        <Title order={2}>By mode</Title>
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
          <ModeCard label="National Rail" reports={overview.byMode.nationalRail} />
          <ModeCard label="TfL" reports={overview.byMode.tfl} />
        </SimpleGrid>
      </Stack>

      {/* Self-hides below two countries present -- see this plan's Judgment
          Call 6: `MODE_TO_COUNTRY` is still empty today (lib/modes.ts's own
          doc comment), so every report is 'Gb' and a one-entry breakdown
          would say nothing a visitor doesn't already know. Mirrors
          AllLinesTable's own identical self-hiding country filter. */}
      {Object.keys(overview.byCountry).length > 1 && (
        <Stack gap="md">
          <Title order={2}>By country</Title>
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {(Object.entries(overview.byCountry) as [Country, LineStatusReport[]][]).map(([country, reports]) => (
              <ModeCard key={country} label={COUNTRY_LABELS[country]} reports={reports} />
            ))}
          </SimpleGrid>
        </Stack>
      )}
    </Stack>
  );
}

/** One of the five top-strip counters. Links to `/lines` pre-filtered to
 * this bucket (Task 4/5 make that filter real) -- the query param name and
 * values are `SeverityGroup` itself, so this link and `AllLinesTable`'s own
 * filter chips can never drift on what a value means. Page-local, not a new
 * `components/` file: this is the only page that renders a "count + label"
 * tile today, mirroring `app/page.tsx`'s own `RightNowModule` precedent for
 * staying local until a second page wants the same shape.
 *
 * 2026-09-22 UX review §2.1: every tile was a `next/link` styled
 * `textDecoration: 'none'; color: 'inherit'` with no hover cue, no focus
 * ring, no chevron and no link colour -- five identical white cards on the
 * one page whose entire purpose is drill-down. `data-group-card-link`/
 * `data-group-card` reuse `/groups`' own established card-as-link idiom
 * (`app/globals.css`): a hover/focus border-colour change plus a visible
 * focus outline on the anchor itself, since Mantine's own focus ring only
 * ever applies to Mantine interactive components, not a bare `<a>`. §2.2's
 * colour cue (a coloured left rail matching `StatusBadge`'s own palette for
 * this bucket) rides along on the same `Card`. */
function SeverityCounterTile({ group, count }: { group: SeverityGroup; count: number }) {
  const label = SEVERITY_GROUP_LABELS[group];

  // A zero-count tile has nothing to drill into -- `/lines?statusGroup=x`
  // for an empty bucket is a real destination that shows nothing, and
  // giving it the exact same link affordance as a nonzero tile invites a
  // pointless click. Rendered muted, with "none" standing in for "0" (both
  // per §2.1's own recommendation).
  if (count === 0) {
    return (
      <Card withBorder shadow="sm" padding="lg" data-group-card>
        <Stack gap={4} align="center">
          <Text size="xl" fw={700} c="dimmed">
            none
          </Text>
          <Text size="sm" c="dimmed" ta="center">
            {label}
          </Text>
        </Stack>
      </Card>
    );
  }

  return (
    <Link
      href={`/lines?statusGroup=${group}`}
      style={{ textDecoration: 'none', color: 'inherit' }}
      data-group-card-link
      aria-label={`${count} line${count === 1 ? '' : 's'} with ${label} — view in All Lines`}
    >
      <Card
        withBorder
        shadow="sm"
        padding="lg"
        data-group-card
        style={{
          borderLeftWidth: 4,
          borderLeftColor: `var(--mantine-color-${SEVERITY_GROUP_COLORS[group]}-6)`,
        }}
      >
        <Stack gap={4} align="center">
          <Text size="xl" fw={700}>
            {count}
          </Text>
          <Text size="sm" c="dimmed" ta="center">
            {label}
          </Text>
          {/* `aria-hidden`: the link's own `aria-label` above already states
              the destination in full for assistive tech; this is the
              sighted-only visual affordance (grape link text + chevron)
              matching the app's established "this goes somewhere" cue. */}
          <Text size="xs" c="var(--mantine-color-anchor)" aria-hidden>
            View lines ›
          </Text>
        </Stack>
      </Card>
    </Link>
  );
}

/** The unbounded worst-lines-first list -- the same "affected lines, worst
 * first" content `app/page.tsx`'s `RightNowModule` shows, but without its
 * `RIGHT_NOW_LIMIT` cap (this plan's Judgment Call 7): this page's whole
 * point is the full picture, not a five-row teaser.
 *
 * 2026-09-22 UX review §2.1/§2.4: each row used to be a hand-rolled
 * `Group`+`Text`+`StatusBadge` wrapped in the same unstyled `<Link>` the
 * tiles above had, dropping `LineStatusCard`'s hover/focus affordance, its
 * shrink guard, its sample summary and its freshness line all at once. This
 * is the shared, documented primitive for exactly this "line status" card
 * shape (`app/page.tsx`'s "Your Lines"/`RightNowModule` both already use
 * it) -- reusing it here fixes both findings in one move rather than
 * re-implementing either by hand again. */
function WorstLinesSection({ worstFirst }: { worstFirst: LineStatusReport[] }) {
  return (
    <Stack gap="md">
      <Title order={2}>Lines to watch</Title>
      {worstFirst.length === 0 ? (
        <Text>Every line is running a Good Service.</Text>
      ) : (
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
          {worstFirst.map((report) => (
            <LineStatusCard key={report.id} report={report} />
          ))}
        </SimpleGrid>
      )}
    </Stack>
  );
}

/** The worst severity across a slice of reports, by true rank (not the raw
 * `statusSeverity` number -- see `severityRank`'s own doc comment on why
 * that matters for TfL codes). Starts from Good Service (10) so an empty
 * slice reads as "nothing wrong" rather than needing a separate branch;
 * `ModeCard` below never actually calls this on an empty slice (it renders
 * its own "no lines tracked" state first), but the fallback is still the
 * honest answer if it ever did. */
function worstSeverityAcross(reports: LineStatusReport[]): number {
  return reports.reduce((worst, report) => {
    const candidate = worstStatus(report).statusSeverity;
    return severityRank(candidate) > severityRank(worst) ? candidate : worst;
  }, 10);
}

/** One row of the mode/country breakdown: a label plus how many of its
 * lines are affected (not Good Service) out of how many total. Reused for
 * both the mode breakdown (National Rail/TfL) and the country breakdown
 * (GB/NI/RoI) -- both answer the exact same question ("how is this slice of
 * the network doing"), just sliced a different way.
 *
 * 2026-09-22 UX review §2.3, two separate fixes:
 *   - Zero lines in this slice no longer renders a green "All Good
 *     Service" badge -- an empty set is not "all good service", it is
 *     "nothing tracked", and the green badge was exactly the kind of false
 *     reassurance the app's copy elsewhere is careful to avoid.
 *   - The "N affected" badge is now coloured by the WORST severity actually
 *     present in the slice (`worstSeverityAcross`), not hard-coded yellow --
 *     previously a slice with two Severe lines out of five affected still
 *     showed a plain yellow "5 affected", understating the list it
 *     summarises. (The badge's `variant="light"` contrast is a separate,
 *     already-tracked fix -- not changed here.) */
function ModeCard({ label, reports }: { label: string; reports: LineStatusReport[] }) {
  if (reports.length === 0) {
    return (
      <Card withBorder padding="lg">
        <Stack gap={4}>
          <Text fw={600}>{label}</Text>
          <Text size="sm" c="dimmed">
            No lines tracked.
          </Text>
        </Stack>
      </Card>
    );
  }

  const affected = reports.filter((report) => !isGoodSeverity(worstStatus(report).statusSeverity));
  const badgeColor = affected.length === 0 ? 'green' : severityColor(worstSeverityAcross(reports));

  return (
    <Card withBorder padding="lg">
      <Stack gap={4}>
        <Group justify="space-between">
          <Text fw={600}>{label}</Text>
          <Badge color={badgeColor} variant="light">
            {affected.length === 0 ? 'All Good Service' : `${affected.length} affected`}
          </Badge>
        </Group>
        <Text size="sm" c="dimmed">
          {reports.length} line{reports.length === 1 ? '' : 's'} tracked
        </Text>
      </Stack>
    </Card>
  );
}
