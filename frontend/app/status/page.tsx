import { Badge, Card, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { getLineStatusForMode } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { DISPLAYED_MODES_PARAM, type Country } from '@/lib/modes';
import { buildNetworkStatusOverview } from '@/lib/networkStatusOverview';
import { isGoodSeverity, SEVERITY_GROUP_LABELS, SEVERITY_GROUPS_BY_RANK, worstStatus } from '@/lib/severity';
import { StatusBadge } from '@/components/StatusBadge';
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

export default async function NetworkStatusPage() {
  // Deliberately the SAME cache key `app/page.tsx`/`app/lines/page.tsx`
  // already use (`lineStatusForMode:${DISPLAYED_MODES_PARAM}`) -- it is the
  // same request, so all three pages share one stale-fallback cache entry.
  const allReports = await withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () =>
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
  );
  const overview = buildNetworkStatusOverview(allReports);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="xs">
        <Title order={1}>Network Status</Title>
        <Text c="dimmed">
          {overview.totalLines} line{overview.totalLines === 1 ? '' : 's'} tracked across National Rail
          and TfL right now.
        </Text>
      </Stack>

      <SimpleGrid cols={{ base: 2, sm: 3, lg: 5 }} spacing="md">
        {SEVERITY_GROUPS_BY_RANK.map((group) => (
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
 * staying local until a second page wants the same shape. */
function SeverityCounterTile({
  group,
  count,
}: {
  group: (typeof SEVERITY_GROUPS_BY_RANK)[number];
  count: number;
}) {
  return (
    <Link href={`/lines?statusGroup=${group}`} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder shadow="sm" padding="lg">
        <Stack gap={4} align="center">
          <Text size="xl" fw={700}>
            {count}
          </Text>
          <Text size="sm" c="dimmed" ta="center">
            {SEVERITY_GROUP_LABELS[group]}
          </Text>
        </Stack>
      </Card>
    </Link>
  );
}

/** The unbounded worst-lines-first list -- the same "affected lines, worst
 * first" content `app/page.tsx`'s `RightNowModule` shows, but without its
 * `RIGHT_NOW_LIMIT` cap (this plan's Judgment Call 7): this page's whole
 * point is the full picture, not a five-row teaser. */
function WorstLinesSection({ worstFirst }: { worstFirst: LineStatusReport[] }) {
  return (
    <Stack gap="md">
      <Title order={2}>Lines to watch</Title>
      {worstFirst.length === 0 ? (
        <Text>Every line is running a Good Service.</Text>
      ) : (
        <Stack gap="xs">
          {worstFirst.map((report) => (
            <Link key={report.id} href={`/lines/${report.id}`} style={{ textDecoration: 'none', color: 'inherit' }}>
              <Card withBorder>
                <Group justify="space-between">
                  <Text fw={600}>{report.name}</Text>
                  <StatusBadge severity={worstStatus(report).statusSeverity} />
                </Group>
              </Card>
            </Link>
          ))}
        </Stack>
      )}
    </Stack>
  );
}

/** One row of the mode/country breakdown: a label plus how many of its
 * lines are affected (not Good Service) out of how many total. Reused for
 * both the mode breakdown (National Rail/TfL) and the country breakdown
 * (GB/NI/RoI) -- both answer the exact same question ("how is this slice of
 * the network doing"), just sliced a different way. */
function ModeCard({ label, reports }: { label: string; reports: LineStatusReport[] }) {
  const affected = reports.filter((report) => !isGoodSeverity(worstStatus(report).statusSeverity)).length;
  return (
    <Card withBorder padding="lg">
      <Stack gap={4}>
        <Group justify="space-between">
          <Text fw={600}>{label}</Text>
          <Badge color={affected === 0 ? 'green' : 'yellow'} variant="light">
            {affected === 0 ? 'All Good Service' : `${affected} affected`}
          </Badge>
        </Group>
        <Text size="sm" c="dimmed">
          {reports.length} line{reports.length === 1 ? '' : 's'} tracked
        </Text>
      </Stack>
    </Card>
  );
}
