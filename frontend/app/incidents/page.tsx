import { Stack, Title, Text } from '@mantine/core';
import { getAllLines, getAllTocs } from '@/lib/api';
import { IncidentSearchForm } from '@/components/IncidentSearchForm';

export const revalidate = 0;

/** `/incidents` -- the cross-network incident archive/search page. See
 * docs/superpowers/specs/2026-09-12-incident-archive-design.md. Thin
 * Server Component shell, mirroring `app/trains/page.tsx`'s structure:
 * reads initial filter values out of `searchParams` (so a filtered link is
 * shareable), fetches the two reference-data lists the filter dropdowns
 * need (both already used the same way on `app/lines/page.tsx`), and
 * passes everything down into the interactive `IncidentSearchForm`. Either
 * reference-data fetch failing degrades to an empty option list rather
 * than crashing the page -- same posture `AllLinesPage` already takes for
 * `getAllTocs()`. */
export default async function IncidentsPage({
  searchParams,
}: {
  searchParams: Promise<{
    operator?: string | string[];
    line?: string | string[];
    from?: string | string[];
    to?: string | string[];
  }>;
}) {
  const { operator, line, from, to } = await searchParams;
  const operatorParam = Array.isArray(operator) ? operator[0] : operator;
  const lineParam = Array.isArray(line) ? line[0] : line;
  const fromParam = Array.isArray(from) ? from[0] : from;
  const toParam = Array.isArray(to) ? to[0] : to;

  const [lines, tocs] = await Promise.all([getAllLines().catch(() => []), getAllTocs().catch(() => [])]);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Incident Archive</Title>
      <Text c="dimmed">
        Search Knowledgebase incidents across the whole network, independent of which line you were
        looking at. Defaults to the last 30 days — use &quot;All time&quot; to see everything this app
        has ever ingested.
      </Text>
      <IncidentSearchForm
        lines={lines}
        tocs={tocs}
        initialOperator={operatorParam}
        initialLine={lineParam}
        initialFrom={fromParam}
        initialTo={toParam}
      />
    </Stack>
  );
}
