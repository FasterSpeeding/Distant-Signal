import { Group, Stack, Title } from '@mantine/core';
import { getAllLines, getAllTocs, getLineStatusForMode, getPreferences } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
import type { Preferences } from '@/lib/types';
import { TextLink } from '@/components/TextLink';
import { AllLinesTable } from './AllLinesTable';

export const revalidate = 0;

// The exact shape getPreferences() already returns for a 401, named so the
// fallback below is typed as `Preferences` rather than inferred with
// `never[]` members. Per-user data fails closed during an outage (design
// spec Decision 5) instead of being stale-served.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [] };

export default async function AllLinesPage() {
  const [lines, preferences, reports, tocs] = await Promise.all([
    withStaleFallback('allLines', () => getAllLines()),
    // Per-user, so it fails closed to "nothing pinned" (the shape a 401
    // already returns) rather than being stale-served -- design spec
    // Decision 5.
    getPreferences().catch(() => NO_PREFERENCES),
    // Deliberately the same cache key as app/page.tsx: it is the same
    // request, so the two pages should share one entry.
    withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () =>
      getLineStatusForMode(DISPLAYED_MODES_PARAM),
    ),
    // Hour-cached reference data used only to label rows; an empty list
    // degrades the table's operator column rather than the whole page.
    getAllTocs().catch(() => []),
  ]);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Group justify="space-between" align="baseline">
          <Title order={1}>All Lines</Title>
          <TextLink href="/lines/new">New custom line</TextLink>
        </Group>
        <AllLinesTable lines={lines} reports={reports} pinnedLineIds={preferences.pinnedLines} tocs={tocs} />
      </Stack>
    </Stack>
  );
}
