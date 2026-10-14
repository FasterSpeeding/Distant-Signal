import { Group, Stack, Text } from '@mantine/core';
import { CardLink } from '@/components/CardLink';
import { SectionTitle } from '@/components/SectionTitle';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import { MERGED_TFL_LINE_IDS } from '@/lib/modes';
import { severityRank, worstStatus } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

/** How many affected lines the "Right now" module renders as cards. Named
 * rather than inlined into the `.slice()` because the module now also tells
 * the reader how many affected lines it is NOT showing, and a literal `5`
 * sitting in one place and a hand-written "5" in the copy somewhere else is
 * exactly how that number goes stale. Nothing outside this module reads it:
 * the overflow count is derived as `count - worst.length` (see
 * `RightNowModule`), not recomputed from this constant, so the rendered
 * card count and the "N more" figure cannot disagree even if the slice
 * changes. */
const RIGHT_NOW_LIMIT = 5;

/** Anonymous-visitor "right now" widget data (§Home page redesign). Built
 * entirely from `allReports`, already fetched unconditionally by this page
 * for the pinned-lines section -- no new endpoint. Excludes
 * `MERGED_TFL_LINE_IDS` the same way `pinnedLineReports` already does,
 * since those ids are folded into their National Rail counterpart's row
 * everywhere a line list is built directly from reports rather than from
 * `/public/lines` -- counting them separately would double-count the same
 * real-world line. */
export function notGoodServiceSummary(reports: LineStatusReport[]) {
  const affected = reports
    .filter((report) => !MERGED_TFL_LINE_IDS.includes(report.id))
    .filter((report) => severityRank(worstStatus(report).statusSeverity) > severityRank(10))
    // Same worst-first-then-alphabetical sort the pinned section already
    // uses (§Home page redesign: "same sort the pinned section already
    // does").
    .sort((a, b) => {
      const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });
  return { count: affected.length, worst: affected.slice(0, RIGHT_NOW_LIMIT) };
}

// Beside the home page, not under `components/`: only the home page uses
// it, twice (the anonymous branch and the authenticated branch with zero
// pinned lines), and both uses are server-rendered.
export function RightNowModule({ summary }: { summary: ReturnType<typeof notGoodServiceSummary> }) {
  const { count, worst } = summary;
  // Derived from what was actually rendered, not from RIGHT_NOW_LIMIT: the
  // heading above states the true total while the list below is capped, so
  // for a bad morning it read "12 lines not at Good Service right now:"
  // over five cards and simply stopped -- the remaining seven were
  // unreachable and unmentioned. Same family of problem as a "Load more"
  // button that vanishes without saying why (see
  // `components/LoadMoreControl.tsx`), but a fixed truncation rather than
  // pagination, so the fix is an overflow line plus a way out, not a
  // button.
  const hidden = count - worst.length;
  return (
    <Stack gap="md">
      <SectionTitle>Right now</SectionTitle>
      {count === 0 ? (
        <Text>Every line is running a Good Service.</Text>
      ) : (
        <>
          <Text>
            {count} line{count === 1 ? '' : 's'} not at Good Service right now:
          </Text>
          <Stack gap="xs">
            {worst.map((report) => (
              <CardLink key={report.id} href={`/lines/${report.id}`}>
                <Group justify="space-between">
                  <Text fw={600}>{report.name}</Text>
                  <StatusBadge severity={worstStatus(report).statusSeverity} />
                </Group>
              </CardLink>
            ))}
          </Stack>
          {hidden > 0 && (
            // `/lines` has no status filter and no filter query params at
            // all today -- `AllLinesTable` holds its operator/country/sort
            // state in `useState`, and `app/lines/page.tsx` takes no
            // `searchParams` -- so this cannot hand the destination the
            // "not at Good Service" context the way `/incidents` links can
            // hand over their filters. It links to the full list anyway:
            // every affected line is reachable and status-badged there (its
            // Status column can be sorted worst-first, though that takes two
            // clicks -- `toggleSort` starts a fresh column at `asc`, and
            // `severityRank` ranks Good Service lowest), which beats the
            // remaining lines being counted and then unreachable. The copy
            // promises the full list, not a filtered view.
            //
            // "the first N", not "the N most disrupted": the sort ranks by
            // `severityRank`'s five groups and tiebreaks alphabetically, so
            // two lines in the same group are ordered by name -- a Suspended
            // line can sit below a Rail Replacement one. "First" is what the
            // module can actually promise.
            //
            // Deliberately reuses this page's existing label for `/lines`
            // ("Browse all lines", beside the anonymous intro and the "Your
            // Lines" heading) rather than inventing a second name for the
            // same destination on the same page.
            // `inline`/`underline="always"`: this link sits in the flow of
            // a sentence rather than in a nav or beside a heading, so (a)
            // colour must not be the only thing marking it (WCAG 1.4.1) and
            // (b) it must render as a `<span>`, not `TextLink`'s default
            // `<p>`, or the rest of the sentence gets forced onto its own
            // line. See TextLink's own doc comment.
            <Text size="sm" c="dimmed">
              Showing the first {worst.length}. {hidden} more {hidden === 1 ? 'line is' : 'lines are'} not at Good
              Service.{' '}
              <TextLink href="/lines" underline="always" inline>
                Browse all lines
              </TextLink>
            </Text>
          )}
        </>
      )}
    </Stack>
  );
}
