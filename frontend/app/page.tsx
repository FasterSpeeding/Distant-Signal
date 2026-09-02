import { Badge, Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
import Link from 'next/link';
import {
  getLineStatusForMode,
  getMyTrackedTrains,
  getPreferences,
  getSession,
  getStationName,
  getStopPointDisruption,
} from '@/lib/api';
import { DISPLAYED_MODES_PARAM, MERGED_TFL_LINE_IDS } from '@/lib/modes';
import { LineStatusCard } from '@/components/LineStatusCard';
import { LoginLink } from '@/components/LoginLink';
import { NotificationsToggle } from '@/components/NotificationsToggle';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { severityRank, worstStatus } from '@/lib/severity';
import { formatSampleSummary, representativeStatus } from '@/lib/sampleStats';
import { formatDate, formatTime } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import type { LineStatus, LineStatusReport, TrackedTrainListItem } from '@/lib/types';

// See app/lines/[id]/page.tsx-adjacent history page and this repo's other
// dynamic routes for the same `revalidate = 0` rationale: without it,
// Next.js treats this route as eligible for static generation and tries to
// prerender it during `next build`, which fails since the `api` service
// only exists on the compose network at runtime.
export const revalidate = 0;

function worstSeverityAcrossReports(reports: LineStatusReport[]): number {
  let worst = 10; // Good Service
  for (const report of reports) {
    for (const status of report.lineStatuses) {
      if (severityRank(status.statusSeverity) > severityRank(worst)) {
        worst = status.statusSeverity;
      }
    }
  }
  return worst;
}

/** The first status carrying real stats across every affected line's
 * report, if any does, else the first status overall — mirrors
 * `representativeStatus`'s own fallback, extended across a station's
 * several affected lines the same way `sampleStatsAcrossReports` extended
 * `firstSampleStats`. */
function representativeStatusAcrossReports(reports: LineStatusReport[]): LineStatus | undefined {
  const withStats = reports.map((r) => representativeStatus(r.lineStatuses)).find((s) => s?.sampleStats);
  return withStats ?? reports[0]?.lineStatuses[0];
}

/** Anonymous-visitor "right now" widget data (§Home page redesign). Built
 * entirely from `allReports`, already fetched unconditionally by this page
 * for the pinned-lines section -- no new endpoint. Excludes
 * `MERGED_TFL_LINE_IDS` the same way `pinnedLineReports` already does,
 * since those ids are folded into their National Rail counterpart's row
 * everywhere a line list is built directly from reports rather than from
 * `/public/lines` -- counting them separately would double-count the same
 * real-world line. */
function notGoodServiceSummary(reports: LineStatusReport[]) {
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
  return { count: affected.length, worst: affected.slice(0, 5) };
}

export default async function DashboardPage() {
  // Same defensive fallback as app/layout.tsx and TicketPanel.tsx: an
  // auth-status glitch degrades to "treat as anonymous", not a broken
  // homepage. See docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md
  // §Home page redesign.
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));

  // Concurrent, independent fetches -- getPreferences()/getMyTrackedTrains()
  // have no data dependency on each other or on line status.
  // getMyTrackedTrains()'s own null-on-401 return is the complete "not
  // logged in" signal (mirroring /track/mine's own established reasoning),
  // so it's safe to fire unconditionally alongside the other two rather
  // than gating it on `session.authenticated` first -- same treatment
  // `preferences` already gets below (fetched even for the anonymous
  // branch, which simply doesn't use it). Per
  // docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md
  // Decision 3.
  const [preferences, allReports, myTrackedTrains] = await Promise.all([
    getPreferences(),
    // Every displayed mode, not just national-rail: a pinned TfL line would
    // otherwise be silently missing from "Your Lines".
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
    getMyTrackedTrains(),
  ]);

  // Hoisted above the anonymous/authenticated branch so both can read it:
  // it's a pure function (see its own doc comment) of `allReports`, which
  // is already fetched unconditionally above regardless of auth state, so
  // this costs nothing extra -- no new fetch, no new endpoint, no extra
  // latency. Previously computed only inside the anonymous branch, which is
  // what left a logged-in user with zero pinned lines staring at a blank
  // dashboard instead of this live module
  // (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F2).
  const rightNow = notGoodServiceSummary(allReports);

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="xl">
        <Stack gap="xs">
          <Group justify="space-between" align="flex-start">
            <Title order={1}>Distant Signal</Title>
            {/* Single global toggle (Decision 6), not per-line -- renders
                for every visitor (Tier 2) regardless of pinned lines, so it
                lives beside the page's own header rather than nested inside
                either the anonymous or logged-in section below. */}
            <NotificationsToggle />
          </Group>
          <Text c="dimmed">
            Live UK rail line status, train tracking, and Delay Repay support — pin the lines and
            stations you care about once you&apos;re logged in.
          </Text>
        </Stack>

        <RightNowModule summary={rightNow} />

        <Group gap="lg">
          <TextLink href="/lines">Browse all lines</TextLink>
          <TextLink href="/stations">Look up a station</TextLink>
          {/* Proactive, not reactive -- session is already in hand on this
              page (see this task's own Step 1), so there's no reason to
              wait for a failed pin click the way PinToggle does elsewhere.
              §Policy's Tier-2 "proactive where session is already fetched"
              refinement. */}
          <LoginLink underline="always">Log in to pin your lines and stations</LoginLink>
        </Group>
      </Stack>
    );
  }

  // Logged-in branch: pinnedLineReports/pinnedStationEntries computation and
  // rendering as before, plus the new Your Tracked Trains section below,
  // reusing the `preferences`/`allReports`/`myTrackedTrains` already fetched
  // concurrently above.
  // The pinned set came out in whatever order `/Line/Mode/…/Status`
  // happened to return, which visibly differed between two captures minutes
  // apart. Worst first, then alphabetical: a dashboard should lead with
  // what needs attention, and must not reshuffle under the user.
  const pinnedLineReports = allReports
    .filter((report) => preferences.pinnedLines.includes(report.id) && !MERGED_TFL_LINE_IDS.includes(report.id))
    .sort((a, b) => {
      const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });

  const pinnedStationEntries = await Promise.all(
    preferences.pinnedStations.map(async (crs) => ({
      crs,
      // The station detail page already shows "London Kings Cross (KGX)";
      // there is no reason for the dashboard to show a bare code. Hour-cached
      // reference data (see `getStationName`), and a failure here falls back
      // to the code rather than taking the dashboard down.
      name: await getStationName(crs).catch(() => null),
      reports: await getStopPointDisruption(crs),
    })),
  );

  // null (not logged in) collapses to [] -- the same "hide entirely"
  // treatment a logged-in user with zero tracked trains gets (Decision 4 of
  // the design spec). slice(0, 5) of an already trackedAt-DESC-ordered
  // response is "5 most recently tracked" with no client-side re-sort
  // needed (Decision 1/3) -- the backend query is already ordered that way.
  const trackedTrains = (myTrackedTrains ?? []).slice(0, 5);

  return (
    <Stack p="lg" gap="xl">
      <Group justify="flex-end">
        <NotificationsToggle />
      </Group>
      <Stack gap="md">
        <Group justify="space-between">
          <Title order={1}>Your Lines</Title>
          <TextLink href="/lines">Browse all lines</TextLink>
        </Group>
        {pinnedLineReports.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any lines yet. <Link href="/lines">Browse all lines</Link> to pin some.
          </Text>
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {pinnedLineReports.map((report) => (
              <LineStatusCard key={report.id} report={report} />
            ))}
          </SimpleGrid>
        )}
      </Stack>

      <Stack gap="md">
        <Group justify="space-between">
          <Title order={2}>Your Stations</Title>
          <TextLink href="/stations">Look up a station</TextLink>
        </Group>
        {pinnedStationEntries.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any stations yet. <Link href="/stations">Look up a station</Link> to pin one.
          </Text>
        ) : (
          <Stack gap="xs">
            {pinnedStationEntries.map(({ crs, name, reports }) => {
              const representative = representativeStatusAcrossReports(reports);
              return (
                <Link key={crs} href={`/stations/${crs}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                  <Card withBorder>
                    <Stack gap={4}>
                      <Group justify="space-between">
                        <Text fw={600}>{name ? `${name} (${crs})` : crs}</Text>
                        <StatusBadge severity={worstSeverityAcrossReports(reports)} />
                      </Group>
                      {representative && (
                        <Text size="xs" c="dimmed">
                          {formatSampleSummary(representative)}
                        </Text>
                      )}
                    </Stack>
                  </Card>
                </Link>
              );
            })}
          </Stack>
        )}
      </Stack>

      {/* The anonymous home gives a visitor a genuinely useful live-status
          module; logging in used to REMOVE it, so a user's reward for the
          single action this app most wants them to take was a blank page with
          two "you haven't pinned anything" lines
          (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F2).
          `2026-08-31-anonymous-user-ux-design.md` called that case "arguably
          fine"; the rendered pages settled the argument the other way, and this
          deliberately overrides that spec decision.

          Gated on pinned LINES only, not on pins of any kind: a user with
          pinned stations but no pinned lines still has a line-shaped hole here,
          and this is a lines module. Costs nothing -- `allReports` is fetched
          unconditionally above and `notGoodServiceSummary` is pure. */}
      {pinnedLineReports.length === 0 && <RightNowModule summary={rightNow} />}

      {trackedTrains.length > 0 && (
        <Stack gap="md">
          <Group justify="space-between">
            <Title order={2}>Your Tracked Trains</Title>
            <TextLink href="/track/mine">View all</TextLink>
          </Group>
          <Stack gap="xs">
            {trackedTrains.map((train) => (
              <TrackedTrainSummaryRow key={train.id} train={train} />
            ))}
          </Stack>
        </Stack>
      )}
    </Stack>
  );
}

// Local, not a new file under `components/`: it is used twice, in one file
// (the anonymous branch and, as of Task 7, the authenticated branch with
// zero pinned lines), and both uses are server-rendered. A new shared
// component would be the right call only if a third page wanted it. Moved
// verbatim out of the anonymous branch's inline JSX -- this extraction step
// is not meant to restyle anything.
function RightNowModule({ summary }: { summary: ReturnType<typeof notGoodServiceSummary> }) {
  const { count, worst } = summary;
  return (
    <Stack gap="md">
      <Title order={2}>Right now</Title>
      {count === 0 ? (
        <Text>Every line is running a Good Service.</Text>
      ) : (
        <>
          <Text>
            {count} line{count === 1 ? '' : 's'} not at Good Service right now:
          </Text>
          <Stack gap="xs">
            {worst.map((report) => (
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
        </>
      )}
    </Stack>
  );
}

// Home-page-local mirror of /track/mine's own row shape
// (frontend/app/track/mine/page.tsx's TrackedTrainListRow/RowStatusBadge/
// STATUS_LABELS) -- same fields, same resolutionStatus-vs-status+
// delayMinutes branching, same words and colors. Deliberately NOT imported
// from that file or extracted into a shared component: per the design
// spec's Testing/Explicitly-out-of-scope sections, that extraction is a
// reasonable but non-mandated implementation-time choice, and this page
// having no import dependency on /track/mine's file keeps this change
// scoped to one file.
function TrackedTrainSummaryRow({ train }: { train: TrackedTrainListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- same logic as /track/mine's own row. The
  // resolved-with-null-trainUid fallback is defensive: the backend's own
  // resolution invariant means this shouldn't happen, but this component
  // doesn't assume it.
  const href =
    train.resolutionStatus === 'resolved' && train.trainUid
      ? `/train/${train.trainUid}/${train.serviceDate}`
      : `/train/by-id/${train.id}`;

  const route = routeLabel(
    train.pinOriginCrs,
    train.pinOriginName,
    train.pinDestinationCrs,
    train.pinDestinationName,
  );

  return (
    <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder>
        <Stack gap={4}>
          <Group justify="space-between" wrap="nowrap">
            <Text fw={500}>{route}</Text>
            <TrackedTrainStatusBadge train={train} />
          </Group>
          <Text size="sm" c="dimmed">
            {formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}
          </Text>
        </Stack>
      </Card>
    </Link>
  );
}

// Short, human badge words -- copied verbatim from /track/mine's own
// STATUS_LABELS so the two pages never disagree about wording for the same
// underlying tokens. Falls back to the raw token itself for anything
// unlisted, so an unexpected value never disappears from the badge.
const STATUS_LABELS: Record<string, string> = {
  pending: 'Pending match',
  unresolved: 'Unmatched',
  awaiting_activation: 'Not yet started',
  en_route: 'En route',
  completed: 'Completed',
  cancelled: 'Cancelled',
};

function TrackedTrainStatusBadge({ train }: { train: TrackedTrainListItem }) {
  // pending/unresolved show the resolution status itself -- no journey
  // status exists yet for either. Once resolved, the journey status plus a
  // delay badge takes over. No "active only" filter and no attempt to
  // distinguish a genuinely-finished journey from one that's merely gone
  // quiet -- per Decision 1/Finding 1 of the design spec, the backend can't
  // honestly support that distinction today.
  if (train.resolutionStatus !== 'resolved') {
    return (
      <Badge color={train.resolutionStatus === 'unresolved' ? 'red' : 'gray'} variant="light">
        {STATUS_LABELS[train.resolutionStatus] ?? train.resolutionStatus}
      </Badge>
    );
  }
  return (
    <Group gap={6} wrap="nowrap">
      {train.status && (
        <Badge color={train.status === 'cancelled' ? 'red' : 'gray'} variant="light">
          {STATUS_LABELS[train.status] ?? train.status}
        </Badge>
      )}
      {train.delayMinutes !== null && (
        <Badge color={train.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
          {train.delayMinutes > 0 ? `${train.delayMinutes}m late` : 'On time'}
        </Badge>
      )}
    </Group>
  );
}
