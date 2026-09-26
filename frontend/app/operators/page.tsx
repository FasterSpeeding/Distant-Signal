import { Stack, Text, SimpleGrid, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { getAllOperators, getPreferences, getSessionOrLoggedOut } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { OperatorStatusCard } from '@/components/OperatorStatusCard';
import { severityRank } from '@/lib/severity';
import type { OperatorSummary, Preferences } from '@/lib/types';

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

/** Worst-first, then alphabetical -- the same sort `app/page.tsx`'s "Your
 * Operators" section already applies to the caller's own pinned subset
 * (review M7/§2.6: this list was alphabetical only, unlike every other
 * worst-first surface in the app, "fine at nine operators, not at the
 * 25-40 the homepage source describes"). Kept here as this page's own copy
 * rather than a shared export, since `app/page.tsx`'s version is typed
 * against its own local sort callback shape and there is no third caller
 * yet to justify factoring one out. */
function worstFirst(operators: OperatorSummary[]): OperatorSummary[] {
  return [...operators].sort((a, b) => {
    const rankDiff = severityRank(b.worstSeverity) - severityRank(a.worstSeverity);
    return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
  });
}

export default async function OperatorsPage() {
  const [operators, preferences, viewerIsAnonymous] = await Promise.all([
    withStaleFallback('allOperators', () => getAllOperators()),
    getPreferences().catch(() => NO_PREFERENCES),
    // `getSessionOrLoggedOut()` (`lib/api.ts`) already degrades a failed
    // check to the logged-out `SessionInfo` shape (logging it first) --
    // `!session.authenticated` on that result is `true`, so this needs no
    // `.catch()` of its own the way the old direct `getSession()` call did.
    getSessionOrLoggedOut().then((session) => !session.authenticated),
  ]);

  const pinnedSet = new Set(preferences.pinnedOperators);
  const sortedOperators = worstFirst(operators);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap={4}>
        <Title order={1}>Operators</Title>
        {/* Review M7/§2.6: `/stations` opens with a one-sentence
            description under its own `<h1>`; this page went straight from
            title to cards. Echoes this page's own `<meta description>`
            rather than inventing separate wording, so the two can't drift
            apart. */}
        <Text c="dimmed">
          Every operator this app tracks, with its worst current line status and today&apos;s delay and
          cancellation figures — worst first.
        </Text>
      </Stack>
      {sortedOperators.length === 0 ? (
        <Text c="dimmed">No operator status data available right now.</Text>
      ) : (
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
          {sortedOperators.map((operator) => (
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
