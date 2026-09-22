import { Stack, Text, SimpleGrid, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { getAllOperators, getPreferences, getSession } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { OperatorStatusCard } from '@/components/OperatorStatusCard';
import type { Preferences } from '@/lib/types';

export const revalidate = 0;

const METADATA_TITLE = 'Operators — Distant Signal';
const METADATA_DESCRIPTION =
  'Every train operator this app tracks — National Rail TOCs and TfL — with its current worst status and aggregate delay/cancellation figures at a glance.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

// Fails closed to "nothing pinned" on a preferences-fetch failure, the
// exact shape a 401 already returns (design spec Decision 5) -- same
// posture as app/lines/page.tsx's own NO_PREFERENCES.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };

export default async function OperatorsPage() {
  const [operators, preferences, viewerIsAnonymous] = await Promise.all([
    withStaleFallback('allOperators', () => getAllOperators()),
    getPreferences().catch(() => NO_PREFERENCES),
    getSession()
      .then((session) => !session.authenticated)
      .catch(() => true),
  ]);

  const pinnedSet = new Set(preferences.pinnedOperators);

  return (
    <Stack p="lg" gap="xl">
      <Title order={1}>Operators</Title>
      {operators.length === 0 ? (
        <Text c="dimmed">No operator status data available right now.</Text>
      ) : (
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
          {operators.map((operator) => (
            <OperatorStatusCard
              key={operator.code}
              operator={operator}
              pinned={pinnedSet.has(operator.code)}
              needsAccountHint={viewerIsAnonymous}
            />
          ))}
        </SimpleGrid>
      )}
    </Stack>
  );
}
