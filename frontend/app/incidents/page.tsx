import { Stack, Title, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { getAllLines, getAllTocs } from '@/lib/api';
import { IncidentSearchForm } from '@/components/IncidentSearchForm';

export const revalidate = 0;

/** Per-page Open Graph/Twitter/`<title>` metadata, in the same four-field
 * shape every detail page in this app already emits (see
 * `app/train/[uid]/[date]/page.tsx`'s `generateMetadata` for the canonical
 * version, and `app/page.tsx`'s own static export for why these top-level
 * pages spell it as a plain `export const metadata` instead).
 *
 * Static rather than an async `generateMetadata()` even though this route
 * DOES read `searchParams`: `generateMetadata` is handed `searchParams`
 * too, so a filter-aware title ("Incidents on the South Western Main
 * Line — Distant Signal") is technically reachable -- but it would have to
 * re-resolve every `?operator=`/`?line=` code to a display name through
 * the same `getAllLines()`/`getAllTocs()` lookups the page makes, purely
 * to decorate a preview card, and a filtered archive URL is not the link
 * people paste. Deliberately left as one honest description of the page
 * itself; revisit only if shared filtered links become a real use.
 *
 * Title matches the page's own `<h1>` below ("Incident Archive"), which is
 * also this route's nav label, so the tab title and the heading a visitor
 * lands on agree. */
const METADATA_TITLE = 'Incident Archive — Distant Signal';
const METADATA_DESCRIPTION =
  'Search National Rail incident messages across the whole network, filtered by operator, line and date range — the last 30 days by default, or everything this app has ever ingested.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

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
    period?: string | string[];
    planned?: string | string[];
    cleared?: string | string[];
    priority_min?: string | string[];
    priority_max?: string | string[];
  }>;
}) {
  const { operator, line, from, to, period, planned, cleared, priority_min, priority_max } =
    await searchParams;
  const operatorParam = Array.isArray(operator) ? operator[0] : operator;
  const lineParam = Array.isArray(line) ? line[0] : line;
  const fromParam = Array.isArray(from) ? from[0] : from;
  const toParam = Array.isArray(to) ? to[0] : to;
  const periodParam = Array.isArray(period) ? period[0] : period;
  const plannedParam = Array.isArray(planned) ? planned[0] : planned;
  const clearedParam = Array.isArray(cleared) ? cleared[0] : cleared;
  const priorityMinParam = Array.isArray(priority_min) ? priority_min[0] : priority_min;
  const priorityMaxParam = Array.isArray(priority_max) ? priority_max[0] : priority_max;

  const [lines, tocs] = await Promise.all([getAllLines().catch(() => []), getAllTocs().catch(() => [])]);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Incident Archive</Title>
      <Text c="dimmed">
        Search National Rail incident messages across the whole network, independent of which line you
        were looking at. Defaults to the last 30 days — use &quot;All time&quot; to see everything this
        app has ever ingested.
      </Text>
      <IncidentSearchForm
        lines={lines}
        tocs={tocs}
        initialOperator={operatorParam}
        initialLine={lineParam}
        initialFrom={fromParam}
        initialTo={toParam}
        initialPeriod={periodParam}
        initialPlanned={plannedParam}
        initialCleared={clearedParam}
        initialPriorityMin={priorityMinParam}
        initialPriorityMax={priorityMaxParam}
      />
    </Stack>
  );
}
