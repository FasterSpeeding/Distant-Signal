import { Stack, Title, SimpleGrid, Text, Group, VisuallyHidden } from '@mantine/core';
import { CardLink } from '@/components/CardLink';
import { SectionTitle } from '@/components/SectionTitle';
import type { Metadata } from 'next';
import {
  ApiNotFoundError,
  getAllOperators,
  getJourney,
  getLineStatusForMode,
  getMyJourneys,
  getMyTrackedTrains,
  getPreferences,
  getSessionOrLoggedOut,
  getSharedGroupCustomLines,
  getStationName,
  getStopPointDisruption,
} from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { DISPLAYED_MODES_PARAM, MERGED_TFL_LINE_IDS } from '@/lib/modes';
import { LineStatusCard } from '@/components/LineStatusCard';
import { PinnedTramLineCard } from '@/components/PinnedTramLineCard';
import { LoginControl } from '@/components/LoginControl';
import { OperatorStatusCard } from '@/components/OperatorStatusCard';
import { TextLink } from '@/components/TextLink';
import { PLAN_JOURNEY_DESTINATION } from '@/lib/navLinks';
import { StateBadge } from '@/components/StateBadge';
import { StatusBadge } from '@/components/StatusBadge';
import { severityRank, worstStatus } from '@/lib/severity';
import { formatSampleSummary, representativeStatus } from '@/lib/sampleStats';
import { loadPinnedTramLines } from '@/lib/transit';
import { mergeSharedCustomLines, type MergedSharedCustomLine } from '@/lib/sharedCustomLines';
import { homeJourneyDetailIds, splitHomeJourneys } from '@/lib/homeJourneys';
import { splitJourneyTrains } from '@/lib/journeyTrains';
import { londonCalendarDay } from '@/lib/londonWallClock';
import type { JourneyDetail, LineStatus, LineStatusReport, Preferences } from '@/lib/types';
import { pageMetadata, previewCards, SITE_NAME } from '@/lib/pageMetadata';
import { CurrentJourneySection, UpcomingJourneysSection } from './HomeJourneys';
import { notGoodServiceSummary, RightNowModule } from './RightNowModule';
import { SharedCustomLineSummaryRow } from './SharedCustomLineSummaryRow';

// See app/lines/[id]/page.tsx-adjacent history page and this repo's other
// dynamic routes for the same `revalidate = 0` rationale: without it,
// Next.js treats this route as eligible for static generation and tries to
// prerender it during `next build`, which fails since the `api` service
// only exists on the compose network at runtime.
export const revalidate = 0;

/** Per-page Open Graph/Twitter/`<title>` metadata, so pasting this app's
 * own front door into Slack/Discord/iMessage/etc. unfurls as something
 * other than the site-wide fallback in `app/layout.tsx`. Same four-field
 * shape every detail page in this app already emits (`app/train/[uid]/
 * [date]/page.tsx`, `app/stations/[crs]/page.tsx`, `app/lines/[id]/
 * page.tsx`, `app/incidents/[id]/page.tsx`, `app/groups/join/[token]/
 * page.tsx`): `title`/`description` plus an `openGraph` and a `twitter`
 * carrying the same two strings.
 *
 * A static `export const metadata` rather than an async
 * `generateMetadata()`: this route takes no params, and the copy below is
 * deliberately the same for every visitor. `generateMetadata` COULD read
 * the session and vary the text with it, but the only consumers of this
 * are link-unfurler bots, which never carry a session cookie and would
 * therefore always get the anonymous branch's wording anyway (the same
 * reasoning `app/groups/join/[token]/page.tsx` spells out for its own
 * deliberately-unauthenticated fetch). Paying a `getSession()` round trip
 * per request to render a string no bot can ever see would be pure cost.
 *
 * Because that bot only ever renders the ANONYMOUS branch, the description
 * describes what that branch actually shows -- the live "Right now" module
 * -- and hedges the pinned/tracked sections as something logging in
 * unlocks, echoing the anonymous branch's own subtitle below rather than
 * promising sections a logged-out visitor will not find on the page they
 * were just linked to.
 *
 * Unlike every other page's, this title carries no `X — ` prefix: the site
 * name IS this page's name, and "Distant Signal — Distant Signal" is not
 * an improvement. It is still spelled out here rather than left to
 * `layout.tsx`'s inherited value, because `openGraph.title`/
 * `twitter.title` below need the same string and nothing inherits into
 * those.
 *
 * The two strings are bound to consts and referenced three times each for
 * the same reason the detail pages build local `title`/`description`
 * variables before returning them: three hand-copied literals is three
 * places for the `<title>` and the unfurled card to drift apart. The same
 * pair of consts, with the same names, is how `/incidents`, `/trains` and
 * `/stations` spell theirs. */
const METADATA_TITLE = SITE_NAME;
const METADATA_DESCRIPTION =
  'Live UK rail status: which lines have problems right now. Log in to pin lines and stations and track trains.';

export const metadata: Metadata = {
  ...pageMetadata(METADATA_TITLE, METADATA_DESCRIPTION),
  // The site name is this page's name: no "Distant Signal · Distant Signal".
  title: { absolute: METADATA_TITLE },
  ...previewCards(METADATA_TITLE, METADATA_DESCRIPTION),
};

// The exact shape getPreferences() already returns for a 401, named so the
// fallback below is typed as `Preferences` rather than inferred with
// `never[]` members. Per-user data fails closed during an outage (design
// spec Decision 5) instead of being stale-served.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };

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
 * `firstSampleStats`.
 *
 * Decision 3's "pinned-station dashboard row" case: extends the same
 * fullCoverageStats-first precedence `representativeStatus` itself already
 * applies per-line, one level further, across every report this dashboard
 * row spans. */
function representativeStatusAcrossReports(reports: LineStatusReport[]): LineStatus | undefined {
  const perReportRepresentatives = reports.map((r) => representativeStatus(r.lineStatuses));
  const withFullCoverage = perReportRepresentatives.find((s) => s?.fullCoverageStats);
  const withStats = withFullCoverage ?? perReportRepresentatives.find((s) => s?.sampleStats);
  return withStats ?? reports[0]?.lineStatuses[0];
}

export default async function DashboardPage() {
  // Same defensive fallback as app/layout.tsx and TicketPanel.tsx: an
  // auth-status glitch degrades to "treat as anonymous", not a broken
  // homepage. See docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md
  // §Home page redesign. `getSessionOrLoggedOut()` (`lib/api.ts`) is what
  // actually degrades this -- unlike a bare `.catch()`, it logs the
  // failure first, since a `getSession()` rejection here is never a
  // confirmed "not logged in", just an inability to confirm either way.
  const session = await getSessionOrLoggedOut();

  // Concurrent, independent fetches -- getPreferences()/getMyTrackedTrains()
  // have no data dependency on each other or on line status.
  // getMyTrackedTrains()'s own null-on-401 return is the complete "not
  // logged in" signal (mirroring the Journeys page's own established reasoning),
  // so it's safe to fire unconditionally alongside the other two rather
  // than gating it on `session.authenticated` first -- same treatment
  // `preferences` already gets below (fetched even for the anonymous
  // branch, which simply doesn't use it). Per
  // docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md
  // Decision 3.
  const [preferences, allReports, myTrackedTrains, sharedCustomLines, allOperators, myJourneys] = await Promise.all([
    // Fails closed to "nothing pinned" -- the exact shape getPreferences
    // already returns for a 401 -- rather than being stale-served: this is
    // per-user data, and the design spec's Decision 5 excludes per-user
    // state from the stale cache on correctness grounds. Losing the pinned
    // sections for the duration of an outage is materially better than
    // losing the whole page, which is what an unguarded throw here did.
    getPreferences().catch(() => NO_PREFERENCES),
    // Every displayed mode, not just national-rail: a pinned TfL line would
    // otherwise be silently missing from "Your Lines".
    withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () => getLineStatusForMode(DISPLAYED_MODES_PARAM)),
    // null is getMyTrackedTrains()'s own established "not logged in" value,
    // and the call site below already collapses it to []. Same fail-closed
    // rationale as preferences above.
    //
    // Trains OTHER members shared into the caller's groups are not
    // fetched: the journey sections are the caller's own travel, and a
    // train someone else is on is no journey of theirs. /journeys and
    // /groups/{id} list those.
    getMyTrackedTrains().catch(() => null),
    // Custom LINES other members have shared into a group the caller
    // belongs to. Gated and null-on-401 like the call above, and
    // `.catch(() => null)`: this section is auxiliary, so a backend hiccup
    // there must cost the caller the shared rows, never the dashboard.
    getSharedGroupCustomLines().catch(() => null),
    // Same public, unauthenticated, cheap-to-fetch-in-full list
    // `/operators` itself fetches (~25-40 rows) -- filtered down to the
    // caller's own pins below, rather than a per-code batch fetch. See
    // this plan's Judgment Call 6.
    //
    // Gated on `session.authenticated`: the anonymous branch below never
    // renders "Your Operators" (or anything else pinned-operator-shaped)
    // at all, so fetching the catalogue for an anonymous visitor would be
    // pure cost. `Promise.resolve([])` is a safe placeholder for that
    // branch specifically because nothing downstream of it in the
    // anonymous return path ever reads this value.
    //
    // `.catch(() => null)`, NOT `.catch(() => [])`: unlike the anonymous
    // placeholder above, a `[]` here would be indistinguishable from "the
    // catalogue is genuinely empty" once fed through the same filter a
    // real result goes through, and would tell a caller who HAS pinned
    // operators that they have nothing pinned. `null` is already a value
    // this page handles elsewhere in this same `Promise.all` (see
    // `getMyTrackedTrains`/`getSharedGroupCustomLines` above) -- the "Your
    // Operators" section below renders a distinct "couldn't load" branch
    // for it instead of the ordinary empty-state sentence, whenever the
    // caller actually has pinned operators to have lost.
    session.authenticated
      ? withStaleFallback('allOperators', () => getAllOperators()).catch(() => null)
      : Promise.resolve([]),
    // The "Current journey" and "Upcoming journeys" sections. Gated like
    // the operators catalogue (the anonymous branch shows neither), and
    // per-user, so a failure is `null`: no journeys, and every train is
    // listed as one tracked on its own (`splitJourneyTrains` folds
    // nothing then), so no train vanishes.
    session.authenticated ? getMyJourneys().catch(() => null) : Promise.resolve(null),
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
          {/* Review §3.1.7: on mobile this used to repeat "Distant
              Signal" -- once as the nav brand, again as this page's own
              `<h1>` a little further down -- reading as duplicated
              branding above the fold. Re-measured after Task 1.2's nav
              collapse (which is what this task was sequenced after): the
              nav no longer wraps to a multi-row header on a phone, so
              the gap between the two is smaller than it once was, but
              the duplication itself is unchanged -- the nav still says
              "Distant Signal" and this heading did too, both still
              visible without scrolling. The `<h1>` stays, for the
              document outline and for a link-unfurler bot with no nav to
              read a name from; only its TEXT changes, to something the
              nav doesn't already say -- the tagline immediately below
              keeps the rest of the pitch. */}
          <Title order={1}>Live UK rail status</Title>
          <Text c="dimmed">
            Live UK rail line status, train tracking and Delay Repay support. Pin the lines, stations and operators you
            care about once you&apos;re logged in.
          </Text>
        </Stack>

        <RightNowModule summary={rightNow} />

        <Group gap="lg">
          {/* First: planning is the one journey feature that works without
              an account, so it is the one worth offering here. */}
          <TextLink href={PLAN_JOURNEY_DESTINATION.href}>Plan a journey</TextLink>
          <TextLink href="/lines">Browse all lines</TextLink>
          <TextLink href="/stations">Look up a station</TextLink>
          {/* Review M8: the anonymous homepage is the only place that can
              advertise the operators list exists to a visitor who hasn't
              logged in yet -- added alongside the other two quick links it
              already carries, in the same position "Browse operators"
              takes in the logged-in "Your Operators" section below. */}
          <TextLink href="/operators">Browse operators</TextLink>
          {/* Proactive, not reactive -- session is already in hand on this
              page (see this task's own Step 1), so there's no reason to
              wait for a failed pin click the way PinToggle does elsewhere.
              §Policy's Tier-2 "proactive where session is already fetched"
              refinement. */}
          <LoginControl underline="always">Log in to pin your lines, stations and operators</LoginControl>
        </Group>
      </Stack>
    );
  }

  // Logged-in branch: the journey sections (current, upcoming, with the
  // trains tracked on their own as one-leg journeys), then the pinned
  // lines, stations and operators, reusing what was fetched concurrently
  // above.
  // The pinned set came out in whatever order `/Line/Mode/…/Status`
  // happened to return, which visibly differed between two captures minutes
  // apart. Worst first, then alphabetical: a dashboard should lead with
  // what needs attention, and must not reshuffle under the user.
  // Every journey that could be under way or still to come today is read in
  // full (`homeJourneyDetailIds`): the list row has no departure or arrival
  // time. Started here, awaited below, so these reads overlap the pinned
  // station and tram reads. A failed read leaves that journey to its list
  // row (`splitHomeJourneys`).
  const now = new Date();
  const journeyItems = myJourneys ?? [];
  const journeyDetailsPromise = Promise.all(
    homeJourneyDetailIds(journeyItems, londonCalendarDay(now.toISOString())).map(async (id) => {
      const detail = await getJourney(id).catch(() => null);
      return detail === null ? null : ([id, detail] as const);
    }),
  );

  const pinnedLineReports = allReports
    .filter((report) => preferences.pinnedLines.includes(report.id) && !MERGED_TFL_LINE_IDS.includes(report.id))
    .sort((a, b) => {
      const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });
  // Pinned tram lines (`metrolink:blue`) are not in the mode statuses
  // above; each served network's lines and statuses come from
  // `/public/transit/{network}`, after the National Rail and TfL ones, in
  // pin order. None while no tram network is served.
  const pinnedTramLines = await loadPinnedTramLines(preferences.pinnedLines);

  // `allOperators` is `null` when its fetch failed (see the fetch's own
  // comment above) -- distinct from a genuinely empty catalogue, which
  // `getAllOperators()` never actually returns in practice but which this
  // code treats identically anyway (there being nothing to filter either
  // way). Named so the "Your Operators" section below can render a
  // "couldn't load" branch instead of the ordinary empty-state sentence
  // specifically when the caller has pinned operators that may have been
  // lost, rather than genuinely having none.
  const operatorsFetchFailed = allOperators === null;

  // Same worst-first-then-alphabetical sort as `pinnedLineReports` above,
  // filtered against the caller's `pinnedOperators` rather than
  // `pinnedLines`. `allOperators` is the whole public catalogue (see its
  // own fetch comment above); this is the caller's own pinned subset of it.
  // `?? []`: a failed fetch (`null`) has nothing to filter, and is treated
  // as empty here the same "fail closed" way `pinnedLineReports`/
  // `pinnedStationEntries` already are elsewhere on this page -- the
  // distinct "couldn't load" messaging for this case lives in the render
  // branch below, keyed off `operatorsFetchFailed` instead.
  const pinnedOperatorSummaries = (allOperators ?? [])
    .filter((operator) => preferences.pinnedOperators.includes(operator.code))
    .sort((a, b) => {
      const rankDiff = severityRank(b.worstSeverity) - severityRank(a.worstSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });

  const pinnedStationEntries = await Promise.all(
    preferences.pinnedStations.map(async (crs) => {
      // `/StopPoint/{crs}/Disruption` 404s when no catalogue line covers
      // this CRS at all (crates/api/src/routes/line_status.rs's
      // `get_stop_point_disruption`) -- distinct from a real `200 []`
      // meaning "covered, every line's fine." Told apart here the same way
      // `/stations/[crs]/page.tsx`'s `fetchStationDisruptions` does, so a
      // pinned-but-uncovered station doesn't silently render the same
      // "Good Service" badge a genuinely fine one gets (the exact bug this
      // task fixes, on a second surface). Any other failure keeps the
      // previous fail-soft behavior: one pinned station's disruption call
      // failing must not take the whole dashboard down. Stale-served first
      // (it is public, read-only status data), and only degrades to an
      // empty list if nothing is cached.
      let reports: LineStatusReport[] = [];
      let coverage: 'covered' | 'none' = 'covered';
      try {
        reports = await withStaleFallback(`stopPointDisruption:${crs}`, () => getStopPointDisruption(crs));
      } catch (err) {
        if (err instanceof ApiNotFoundError) {
          coverage = 'none';
        }
      }
      return {
        crs,
        // The station detail page already shows "London Kings Cross (KGX)";
        // there is no reason for the dashboard to show a bare code. Hour-cached
        // reference data (see `getStationName`), and a failure here falls back
        // to the code rather than taking the dashboard down.
        name: await getStationName(crs).catch(() => null),
        reports,
        coverage,
      };
    }),
  );

  const journeyDetails = new Map<number, JourneyDetail>(
    (await journeyDetailsPromise).filter((entry): entry is readonly [number, JourneyDetail] => entry !== null),
  );
  // A train tracked on its own is a one-leg journey: it is the current
  // journey while under way, and among the upcoming ones while still to
  // come, ordered in with them by departure. A train that is a leg of one
  // of the caller's journeys is shown through that journey, the same rule
  // /journeys applies (`splitJourneyTrains`). null (not logged in, or the
  // fetch failed) is no trains.
  const { standalone: standaloneTrains } = splitJourneyTrains(myTrackedTrains ?? [], myJourneys);
  const { current, alsoInProgress, upcoming } = splitHomeJourneys(journeyItems, journeyDetails, standaloneTrains, now);

  // Custom lines a fellow group member shared with the caller. One row per
  // line carrying every group it reached them through (the wire shape is
  // one row per (group, line) pair -- see `mergeSharedCustomLines`).
  //
  // Status comes from `allReports`, already fetched above: a granted line
  // is now included in `/Line/Mode/.../Status` for this caller by the same
  // read-path widening that makes the grant mean anything, so this section
  // costs no extra request. A line with no computed status yet simply
  // renders without a badge.
  //
  // Deliberately its own section rather than folded into "Your Lines":
  // those are the caller's own pins, with their own pin controls, and a
  // shared line is view-only -- no pin toggle, no edit, no un-share (only
  // the owner, from the group's page, can stop sharing it).
  //
  // A granted member CAN pin a shared line (`pinned_lines` takes any id),
  // and "Your Lines" above renders it straight out of `allReports` when
  // they have -- so those are excluded here, or the same line would render
  // twice on one screen. "Your Lines" wins that tie: the caller put it
  // there on purpose.
  //
  // Keyed off `pinnedLineReports`, NOT `preferences.pinnedLines`: the two
  // differ for a pinned line the aggregator has not written a
  // `line_status` row for yet (a brand-new custom line, before its first
  // cycle). Such an id is in `preferences.pinnedLines` but absent from
  // `allReports`, so "Your Lines" does not render it -- excluding it here
  // too would drop it from the home page entirely rather than deduplicate
  // it. `pinnedLineReports` is exactly the set that section will render.
  // Named rather than inlined at its one prior call site: also reused below
  // to de-duplicate "Your Operators" against lines already shown in "Your
  // Lines" a few hundred pixels above it (review M10).
  const pinnedLineIds = new Set(pinnedLineReports.map((report) => report.id));
  const sharedLines: MergedSharedCustomLine[] = mergeSharedCustomLines(sharedCustomLines ?? [], pinnedLineIds);
  const reportByLineId = new Map(allReports.map((report) => [report.id, report]));

  // Review §3.1.4: both empty states below used to pair a "Browse all
  // lines"/"Look up a station" link beside their section heading with an
  // IDENTICAL link ~40px below it, inside the "you haven't pinned
  // anything" sentence -- a dashboard that reads as a stack of duplicated
  // prompts rather than one useful action per section. The heading-level
  // link now only renders once its section actually has something to
  // browse PAST (pinned rows already on screen); an empty section keeps
  // just the one link, inline in its own sentence.
  const pinnedLineCount = pinnedLineReports.length + pinnedTramLines.length;
  const allPinnedSectionsEmpty =
    pinnedLineCount === 0 && pinnedStationEntries.length === 0 && pinnedOperatorSummaries.length === 0;

  return (
    <Stack p="lg" gap="xl">
      {/* The logged-in home is a dashboard of equal sections, led by the
          journey sections when there are any, so no one section can be the
          page's title. Its h1 names the page for the outline and screen
          readers, and every section below is an h2 (docs/style-guide.md,
          Heading rules). */}
      <VisuallyHidden>
        <Title order={1}>Home</Title>
      </VisuallyHidden>

      {/* The journey under way now leads the page, then the journeys still
          to come. "Current journey" renders only when there is one.
          "Upcoming journeys" always does: its "New journey" button is one
          of the two ways in to /journeys/new (the nav has none, DS-115),
          so a new user with no journeys finds it here too. */}
      {current !== null && <CurrentJourneySection current={current} alsoInProgress={alsoInProgress} now={now} />}
      <UpcomingJourneysSection upcoming={upcoming} loadFailed={myJourneys === null} />

      {/* When ALL THREE pinned sections are empty, the live "Right now" module
          is the only thing on this page with real content -- it leads,
          ahead of otherwise-empty prompts, rather than being buried below
          them (its usual spot, further down, still applies whenever only
          Lines is empty but Stations or Operators has something pinned). */}
      {allPinnedSectionsEmpty && <RightNowModule summary={rightNow} />}

      <Stack gap="md">
        <Group justify="space-between">
          <SectionTitle>Your lines</SectionTitle>
          {pinnedLineCount > 0 && <TextLink href="/lines">Browse all lines</TextLink>}
        </Group>
        {pinnedLineCount === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any lines yet.{' '}
            <TextLink href="/lines" underline="always" inline>
              Browse all lines
            </TextLink>{' '}
            to pin some.
          </Text>
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {pinnedLineReports.map((report) => (
              <LineStatusCard key={report.id} report={report} />
            ))}
            {pinnedTramLines.map((pinned) => (
              <PinnedTramLineCard key={pinned.line.id} pinned={pinned} />
            ))}
          </SimpleGrid>
        )}
      </Stack>

      {sharedLines.length > 0 && (
        <Stack gap="md">
          <SectionTitle>Lines shared with you</SectionTitle>
          <Stack gap="xs">
            {sharedLines.map((row) => (
              <SharedCustomLineSummaryRow
                key={row.line.lineId}
                row={row}
                report={reportByLineId.get(row.line.lineId)}
              />
            ))}
          </Stack>
        </Stack>
      )}

      <Stack gap="md">
        <Group justify="space-between">
          <SectionTitle>Your stations</SectionTitle>
          {pinnedStationEntries.length > 0 && <TextLink href="/stations">Look up a station</TextLink>}
        </Group>
        {pinnedStationEntries.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any stations yet.{' '}
            <TextLink href="/stations" underline="always" inline>
              Look up a station
            </TextLink>{' '}
            to pin one.
          </Text>
        ) : (
          <Stack gap="xs">
            {pinnedStationEntries.map(({ crs, name, reports, coverage }) => {
              const representative = representativeStatusAcrossReports(reports);
              return (
                <CardLink key={crs} href={`/stations/${crs}`}>
                  <Stack gap={4}>
                    <Group justify="space-between">
                      <Text fw={600}>{name ? `${name} (${crs})` : crs}</Text>
                      {coverage === 'none' ? (
                        <StateBadge tone="neutral">Not tracked</StateBadge>
                      ) : (
                        <StatusBadge severity={worstSeverityAcrossReports(reports)} />
                      )}
                    </Group>
                    {coverage === 'none' ? (
                      <Text size="xs" c="dimmed">
                        Not covered by our line-status tracking yet.
                      </Text>
                    ) : (
                      representative && (
                        <Text size="xs" c="dimmed">
                          {formatSampleSummary(representative)}
                        </Text>
                      )
                    )}
                  </Stack>
                </CardLink>
              );
            })}
          </Stack>
        )}
      </Stack>

      <Stack gap="md">
        <Group justify="space-between">
          <SectionTitle>Your operators</SectionTitle>
          {pinnedOperatorSummaries.length > 0 && <TextLink href="/operators">Browse all operators</TextLink>}
        </Group>
        {pinnedOperatorSummaries.length === 0 ? (
          operatorsFetchFailed && preferences.pinnedOperators.length > 0 ? (
            // Distinct from the ordinary empty-state sentence below: the
            // catalogue fetch failed (`allOperators === null`, see its own
            // fetch comment above) and the caller DOES have pinned
            // operators, so "you haven't pinned any" would be actively
            // false -- this reads as a transient outage instead of a
            // (wrong) claim about the caller's own preferences.
            <Text c="dimmed">Couldn&apos;t load operator status right now.</Text>
          ) : (
            <Text c="dimmed">
              You haven&apos;t pinned any operators yet.{' '}
              <TextLink href="/operators" underline="always" inline>
                Browse all operators
              </TextLink>{' '}
              to pin some.
            </Text>
          )
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {pinnedOperatorSummaries.map((operator) => (
              <OperatorStatusCard
                key={operator.code}
                operator={operator}
                pinned
                // Review M9: `LineStatusCard` (the "Your Lines" card just
                // above) has no pin control of its own -- rather than the
                // dashboard teaching two different rules for two
                // same-shaped cards, this card goes without one here too;
                // unpinning stays available from `/operators` itself.
                showPin={false}
                // Review M10: when the line actually driving this
                // operator's rollup is already a card in "Your Lines"
                // above, collapse the repeated reason into a short
                // cross-reference instead of printing the same disruption
                // a second (or third, alongside the station row) time.
                dedupedLineId={
                  operator.worstLineId && pinnedLineIds.has(operator.worstLineId) ? operator.worstLineId : undefined
                }
              />
            ))}
          </SimpleGrid>
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
          unconditionally above and `notGoodServiceSummary` is pure.

          `!allPinnedSectionsEmpty`: when Stations and Operators also have
          nothing pinned, this module already rendered once, at the very top
          of the page (review §3.1.4) -- this is its ordinary spot for the
          narrower case where Lines is empty but Stations or Operators is
          not. */}
      {!allPinnedSectionsEmpty && pinnedLineCount === 0 && <RightNowModule summary={rightNow} />}
    </Stack>
  );
}
