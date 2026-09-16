import { Badge, Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import {
  ApiNotFoundError,
  getLineStatusForMode,
  getMyTrackedTrains,
  getPreferences,
  getSession,
  getSharedGroupTrains,
  getStationName,
  getStopPointDisruption,
} from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
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
import { mergeSharedTrains, type MergedSharedTrain } from '@/lib/sharedTrains';
import type { LineStatus, LineStatusReport, Preferences, TrackedTrainListItem } from '@/lib/types';

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
const METADATA_TITLE = 'Distant Signal';
const METADATA_DESCRIPTION =
  "Live UK rail line status at a glance: which lines aren't running a Good Service right now — then pin the lines and stations you care about, and track your trains, once you're logged in.";

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

// The exact shape getPreferences() already returns for a 401, named so the
// fallback below is typed as `Preferences` rather than inferred with
// `never[]` members. Per-user data fails closed during an outage (design
// spec Decision 5) instead of being stale-served.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [] };

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
  return { count: affected.length, worst: affected.slice(0, RIGHT_NOW_LIMIT) };
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
  const [preferences, allReports, myTrackedTrains, sharedGroupTrains] = await Promise.all([
    // Fails closed to "nothing pinned" -- the exact shape getPreferences
    // already returns for a 401 -- rather than being stale-served: this is
    // per-user data, and the design spec's Decision 5 excludes per-user
    // state from the stale cache on correctness grounds. Losing the pinned
    // sections for the duration of an outage is materially better than
    // losing the whole page, which is what an unguarded throw here did.
    getPreferences().catch(() => NO_PREFERENCES),
    // Every displayed mode, not just national-rail: a pinned TfL line would
    // otherwise be silently missing from "Your Lines".
    withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () =>
      getLineStatusForMode(DISPLAYED_MODES_PARAM),
    ),
    // null is getMyTrackedTrains()'s own established "not logged in" value,
    // and the call site below already collapses it to []. Same fail-closed
    // rationale as preferences above.
    getMyTrackedTrains().catch(() => null),
    // Trains OTHER members shared into a group the caller belongs to.
    // Gated and null-on-401 in exactly the same way as the call above, and
    // `.catch(() => null)` for the same reason /track/mine gives for its
    // own copy of this call: the group-shared half of this section is
    // auxiliary, so a backend hiccup there must cost the caller the shared
    // rows, never their own rows or the whole dashboard. `null` is already
    // a value this page handles (it is the 401 return), so the failure
    // collapses into the existing "nothing shared with you" branch.
    getSharedGroupTrains().catch(() => null),
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

  // null (not logged in) collapses to [] -- the same "hide entirely"
  // treatment a logged-in user with zero tracked trains gets (Decision 4 of
  // the design spec).
  const ownTrains = myTrackedTrains ?? [];
  // De-duplicated per train and filtered against the caller's own rows --
  // see `mergeSharedTrains`' own doc comment for why both halves of that
  // are the frontend's job rather than the query's. Note the full own-train
  // list (not the sliced five) is what that filter is built from: a train
  // that is the caller's own is never a "shared" row here, whether or not
  // it made this section's cap.
  const sharedTrains = mergeSharedTrains(sharedGroupTrains ?? [], new Set(ownTrains.map((t) => t.id)));

  // ONE list, own rows first then shared -- the same single-list, own-first
  // shape /track/mine settled on (see its own comment): in a merged list a
  // row with no attribution reads as one the caller tracked themselves,
  // which is exactly why each shared row carries its "from <group>"/
  // "Shared by <who>" tags. Interleaving is deliberately not attempted:
  // `trackedAt` orders the caller's own half (and a shared train never
  // exposes one -- spec §4's "Never shown" list), and re-sorting on
  // `serviceDate`/`pinScheduledDeparture` would override exactly the
  // ordering Decision 1 chose.
  //
  // slice(0, 5) of an already trackedAt-DESC-ordered response is "5 most
  // recently tracked" with no client-side re-sort needed (Decision 1/3) --
  // the backend query is already ordered that way. The cap is on the
  // section as a whole, not per half: Decision 1's whole point is that this
  // supplementary section must not out-compete the line-status overview
  // above it, and two five-row halves would be ten rows. A caller with five
  // or more of their own trains therefore sees the shared ones via "View
  // all" (/track/mine, which caps neither half) rather than here.
  const trackedTrainRows: TrackedTrainRow[] = [
    ...ownTrains.map((train): TrackedTrainRow => ({ kind: 'own', train })),
    ...sharedTrains.map((row): TrackedTrainRow => ({ kind: 'shared', row })),
  ].slice(0, 5);

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
            {pinnedStationEntries.map(({ crs, name, reports, coverage }) => {
              const representative = representativeStatusAcrossReports(reports);
              return (
                <Link key={crs} href={`/stations/${crs}`} style={{ textDecoration: 'none', color: 'inherit' }}>
                  <Card withBorder>
                    <Stack gap={4}>
                      <Group justify="space-between">
                        <Text fw={600}>{name ? `${name} (${crs})` : crs}</Text>
                        {coverage === 'none' ? (
                          <Badge color="gray" variant="light">
                            Not tracked
                          </Badge>
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

      {trackedTrainRows.length > 0 && (
        <Stack gap="md">
          <Group justify="space-between">
            <Title order={2}>Your Tracked Trains</Title>
            <TextLink href="/track/mine">View all</TextLink>
          </Group>
          <Stack gap="xs">
            {/* Keys are prefixed per half because the two id spaces are
                the same one -- `SharedGroupTrain.trainSubscriptionId` and
                `TrackedTrainListItem.id` are both `train_subscriptions.id`
                -- so an unprefixed key could collide if
                `mergeSharedTrains`' own own-train filter ever regressed. */}
            {trackedTrainRows.map((row) =>
              row.kind === 'own' ? (
                <TrackedTrainSummaryRow key={`own-${row.train.id}`} train={row.train} />
              ) : (
                <SharedTrainSummaryRow key={`shared-${row.row.train.trainSubscriptionId}`} row={row.row} />
              ),
            )}
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
            <Group gap="xs" wrap="wrap">
              <Text size="sm" c="dimmed">
                Showing the first {worst.length} — {hidden} more{' '}
                {hidden === 1 ? 'line is' : 'lines are'} not at Good Service.
              </Text>
              {/* `underline="always"`: this sits in the flow of a sentence
                  rather than in a nav or beside a heading, so colour must
                  not be the only thing marking it (see TextLink's own doc
                  comment, WCAG 1.4.1). */}
              <TextLink href="/lines" underline="always">
                Browse all lines
              </TextLink>
            </Group>
          )}
        </>
      )}
    </Stack>
  );
}

/** One row of the "Your Tracked Trains" section: either a train the caller
 * tracked themselves, or one another member shared into a group they
 * belong to. A discriminated union rather than one widened row type, so
 * the shared half can never accidentally be handed to the own-row
 * component (which links to the owner-scoped `/train/by-id/{id}` route) or
 * vice versa. */
type TrackedTrainRow =
  | { kind: 'own'; train: TrackedTrainListItem }
  | { kind: 'shared'; row: MergedSharedTrain };

/** A train another member shared into a group the caller belongs to,
 * rendered in the same list as the caller's own summary rows above.
 * Home-page-local sibling of `TrackedTrainSummaryRow` below, and a trimmed
 * mirror of /track/mine's own `SharedTrainListRow` -- same tags, same
 * wording, same linking rule.
 *
 * Deliberately NOT `TrackedTrainSummaryRow` with extra props: the caller
 * doesn't own this train, so none of an own row's affordances apply. There
 * is no rename (`POST /Train/{id}/name` is owner-scoped), no ticket data
 * (spec §4 forbids a shared train ever carrying any) and no delete -- and
 * the home page's own rows carry none of those controls either, so the
 * distinction that actually matters here is the LINK: an own row falls back
 * to `/train/by-id/{id}`, which is owner-scoped and 404s for anyone else.
 *
 * So this row links exactly when a `trainUid` is known, and renders plain
 * text otherwise -- a deliberately weaker test than the own row's
 * `resolutionStatus === 'resolved' && trainUid`, because `trainUid` is
 * populated well before the status reaches `resolved` and
 * `/train/{uid}/{date}` is public and unscoped. Same reasoning
 * /track/mine's shared row states at length. */
function SharedTrainSummaryRow({ row }: { row: MergedSharedTrain }) {
  const { train, groupNames } = row;
  // `routeLabel`, not `trackedTrainDisplayName`: this page's own rows
  // (`TrackedTrainSummaryRow` below) label by route too, so using the
  // sharer's `customName` here would make the two halves of one list
  // disagree about what a row's heading even is. /track/mine, whose own
  // rows DO show a custom name, shows it for shared rows for the same
  // consistency reason.
  const route = routeLabel(
    train.pinOriginCrs,
    train.pinOriginName,
    train.pinDestinationCrs,
    train.pinDestinationName,
  );
  // Same date-only degradation as the own row below, for the same reason:
  // a pin with no schedule data yet has no departure time, and `Invalid
  // Date` is never an acceptable label.
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  const href = train.trainUid ? `/train/${train.trainUid}/${train.serviceDate}` : null;

  const card = (
    <Card withBorder>
      <Stack gap={4}>
        <Group justify="space-between" wrap="nowrap">
          <Text fw={500}>{route}</Text>
          <TrackedTrainStatusBadge train={train} />
        </Group>
        <Text size="sm" c="dimmed">
          {when}
        </Text>
        <Group gap="xs" wrap="wrap">
          {/* One badge per group this train reached the caller through --
              a train shared into two of their groups is two tags, not an
              arbitrarily-picked one. `addedByName` is `null` when the
              sharer has neither a name nor a username on their account --
              never their email, which is not something to show the rest of
              a group (`crates/api/src/data/users.rs`'s `display_label`).
              "a member" then, never a raw user id -- same wording and same
              fallback /track/mine and /groups/{id} already use, and
              `?.trim() ||` rather than `??` for the same reason they use
              it: a blank name is not a label either. */}
          {groupNames.map((groupName) => (
            <Badge key={groupName} variant="light" color="grape">
              from {groupName}
            </Badge>
          ))}
          <Text size="sm" c="dimmed">
            Shared by {train.addedByName?.trim() || 'a member'}
          </Text>
        </Group>
      </Stack>
    </Card>
  );

  // Whole-card link when there's somewhere to go, matching the own row
  // directly below; nothing inside the card is itself interactive, so
  // there's no nested-<a> problem of the kind /track/mine's richer row has
  // to work around.
  return href ? (
    <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
      {card}
    </Link>
  ) : (
    card
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
            {/* `pinScheduledDeparture` is `null` for an NR-primary
                subscription whose train has no schedule data yet -- same
                date-only degradation as /track/mine's own row. */}
            {train.pinScheduledDeparture
              ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
              : formatDate(train.serviceDate)}
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
  schedule_matched: 'Matched to schedule',
  unresolved: 'Unmatched',
  awaiting_activation: 'Not yet started',
  en_route: 'En route',
  completed: 'Completed',
  cancelled: 'Cancelled',
};

/** Structural, not `TrackedTrainListItem`: `SharedTrainSummaryRow` renders
 * the identical badges off a `SharedGroupTrain`, whose `resolutionStatus`/
 * `status` are plain `string`s on the wire rather than the own-list's
 * narrowed unions. Both shapes satisfy this, and neither needs an adapter
 * -- the branching below already treats every value as an opaque token
 * (`STATUS_LABELS` falls back to the raw string for anything unlisted).
 * Same widening /track/mine's own `RowStatusBadge` already made. */
function TrackedTrainStatusBadge({
  train,
}: {
  train: { resolutionStatus: string; status: string | null; delayMinutes: number | null };
}) {
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
