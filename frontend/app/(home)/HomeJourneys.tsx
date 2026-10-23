import { Box, Card, Group, Stack, Text } from '@mantine/core';
import { ButtonLink } from '@/components/ButtonLink';
import { CardLink } from '@/components/CardLink';
import { DelayBadge } from '@/components/DelayBadge';
import { JourneyBreakBanner } from '@/components/JourneyBreakBanner';
import { JourneyStatusGroupBadge } from '@/components/JourneyStatusBadge';
import { instantLateDepartureTimes, LateDepartureNote } from '@/components/LateDepartureNote';
import { LoginControl } from '@/components/LoginControl';
import { SectionTitle } from '@/components/SectionTitle';
import { StatusRow } from '@/components/StatusRow';
import { TextLink } from '@/components/TextLink';
import { TrackedRowStatusBadge } from '@/components/TrackedRowStatusBadge';
import { formatTime } from '@/lib/dateFormat';
import { describeFailure } from '@/lib/failure';
import { followedFromLabel } from '@/lib/followedFrom';
import { JOURNEYS_DESTINATION, NEW_JOURNEY_LABEL, PLAN_JOURNEY_DESTINATION, TRACK_JOURNEY_HREF } from '@/lib/navLinks';
import { tokens } from '@/lib/theme';
import {
  currentLeg,
  homeJourneyStatusGroup,
  isLoneTrain,
  MAX_UPCOMING_JOURNEYS,
  nextLateDepartureReliance,
  type HomeCurrent,
  type HomeEntry,
  type HomeJourney,
  type HomeLoneTrain,
} from '@/lib/homeJourneys';
import { legDepartureInstant, legExpectedArrivalInstant } from '@/lib/journeyLegChaining';
import { legDestinationArrivalLabel, legEndpointName } from '@/lib/journeyLegLabel';
import {
  journeyDisplayName,
  journeyListItemNameInput,
  journeyNameInputFromLegs,
  journeyPreviewLine,
} from '@/lib/journeyName';
import { legStatusGroup, type JourneySummaryGroup } from '@/lib/journeyStatus';
import { routeLabel, stationLabel } from '@/lib/stationLabel';
import { trackedTrainDisplayName, trackedTrainHref, trackedTrainRouteLabel } from '@/lib/trackingName';
import { isUntrackedLeg, untrackedLegPlaces, untrackedLegTimes, untrackedLegTitle } from '@/lib/untrackedLeg';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

/* The logged-in home page's journey sections: "Current journey" (the one
 * under way now) and "Upcoming journeys". A train tracked on its own is a
 * one-leg journey in both, drawn from its list row with the Track list's
 * own badge (`TrackedRowStatusBadge`). `lib/homeJourneys.ts` decides which
 * goes where; this file only draws them. Server-rendered, like the rest
 * of the home page. */

/** What the traveller is doing now: the current leg's route and time ("York
 * (YRK) to Newcastle (NCL) · 14:02"), or an untracked leg's mode, places
 * and planned times ("Tube: Victoria line, Euston to Victoria · 08:05 to
 * 08:20"). */
function legNowText(leg: JourneyLegDetail): string {
  if (isUntrackedLeg(leg)) {
    const places = untrackedLegPlaces(leg);
    const times = untrackedLegTimes(leg);
    const what = places ? `${untrackedLegTitle(leg)}, ${places}` : untrackedLegTitle(leg);
    return times ? `${what} · ${times}` : what;
  }
  // The server-resolved names first, as the journey's name uses them, then
  // the calling-point table's (`legEndpointName`), so the leg and the name
  // above it never disagree.
  const state = leg.trackedTrainState;
  const stops = state?.journeyStops;
  const route = routeLabel(
    leg.originCrs,
    leg.originName ?? legEndpointName(leg.originCrs, stops, state, 'origin'),
    leg.destinationCrs,
    leg.destinationName ?? legEndpointName(leg.destinationCrs, stops, state, 'destination'),
  );
  const departure = legDepartureInstant(leg);
  return departure ? `${route} · ${formatTime(departure)}` : route;
}

/** Where the current leg ends, by name and code ("York (YRK)"), or its
 * place name for an untracked leg. `null` when nothing names it. */
function legEndText(leg: JourneyLegDetail): string | null {
  if (leg.destinationCrs) return stationLabel(leg.destinationCrs, leg.destinationName);
  return leg.destinationName?.trim() || leg.trackedTrainState?.scheduleDestinationName || null;
}

/** "arrive 15:22" (reported) or "arrive est. 15:22" for the current leg's
 * own destination. An untracked leg's planned arrival is an estimate too. */
function legArrivalText(leg: JourneyLegDetail): string | null {
  if (isUntrackedLeg(leg)) {
    const arrival = legExpectedArrivalInstant(leg);
    return arrival ? `arrive est. ${formatTime(arrival)}` : null;
  }
  return legDestinationArrivalLabel(leg.destinationCrs, leg.trackedTrainState?.journeyStops);
}

/** The current leg's own live status, secondary to the journey's: its
 * delay in minutes where a running train has one ("12 min late"),
 * otherwise its status badge. `null` when that would only repeat the
 * journey's own badge (`headline`). */
function LegStatus({ leg, headline }: { leg: JourneyLegDetail; headline: JourneySummaryGroup }) {
  const group = legStatusGroup(leg);
  const delay = leg.trackedTrainState?.delayMinutes ?? null;
  if (group === 'delayed' && delay !== null) return <DelayBadge delayMinutes={delay} />;
  if (group === headline) return null;
  return <JourneyStatusGroupBadge group={group} />;
}

/** The journey under way now, drawn to stand out from everything else on
 * the page: a 2px brand border and roomier padding (`.currentJourneyCard`),
 * the journey's name at a larger size and its headline status, which is
 * the whole journey's (`homeJourneyStatusGroup`), never only the train
 * under way. Secondary to that, two facts: the leg the traveller is on
 * with its own live delay or status, and the next change or the arrival.
 * The whole card links to the journey page. */
export function CurrentJourneyCard({ journey, now }: { journey: HomeJourney & { detail: JourneyDetail }; now: Date }) {
  const { detail } = journey;
  const name = journeyNameInputFromLegs(
    detail.customName,
    detail.legs.map((leg) => ({ ...leg, trainUid: leg.trackedTrainState?.trainUid ?? null })),
  );
  const preview = journeyPreviewLine(name);
  const headline = homeJourneyStatusGroup(journey);
  const leg = currentLeg(detail, now);
  const legIndex = leg === null ? -1 : detail.legs.indexOf(leg);
  const isLastLeg = legIndex === detail.legs.length - 1;
  const end = leg === null ? null : legEndText(leg);
  const arrival = leg === null ? null : legArrivalText(leg);
  const endFact = [end, arrival].filter((part): part is string => part !== null).join(', ');
  const reliance = leg === null ? null : nextLateDepartureReliance(detail, leg, now);

  return (
    <Box className="currentJourneyCard">
      <CardLink href={`/journeys/${detail.id}`} padding="lg">
        <Stack gap="sm">
          {/* The preview line sits under the row, not beside the badge, so
              on a phone it gets the card's full width. */}
          <Stack gap={4}>
            <StatusRow
              align="flex-start"
              title={
                <Text fw={700} fz="lg" lineClamp={2} miw={0}>
                  {journeyDisplayName(name)}
                </Text>
              }
              trailing={<JourneyStatusGroupBadge group={headline} />}
            />
            {preview && (
              <Text size="sm" c="dimmed">
                {preview}
              </Text>
            )}
            {/* A journey the traveller follows (DS-146): whose it is. */}
            {journey.item.followedFrom && (
              <Text size="sm" c="dimmed">
                {followedFromLabel(journey.item.followedFrom)}
              </Text>
            )}
          </Stack>
          {leg !== null && (
            <dl className="keyFacts keyFacts--compact">
              <div>
                <dt>{detail.legs.length > 1 ? `Now, leg ${legIndex + 1} of ${detail.legs.length}` : 'Now'}</dt>
                <dd>
                  <Group gap="xs" wrap="wrap">
                    <span>{legNowText(leg)}</span>
                    <LegStatus leg={leg} headline={headline} />
                  </Group>
                </dd>
              </div>
              {endFact !== '' && (
                <div>
                  <dt>{isLastLeg ? 'Destination' : 'Next change'}</dt>
                  <dd>{endFact}</dd>
                </div>
              )}
            </dl>
          )}
          {reliance && <LateDepartureNote {...instantLateDepartureTimes(reliance)} subject="the next train" />}
        </Stack>
      </CardLink>
    </Box>
  );
}

/** One journey in a compact list: its name, the route and date a custom
 * name hides (`journeyPreviewLine`), its departure time when it was read
 * in full, whose it is when the traveller follows it rather than owns it
 * (DS-146), and its whole-journey status (`homeJourneyStatusGroup`). Links
 * to the journey page. */
export function HomeJourneyRow({ journey }: { journey: HomeJourney }) {
  const name = journeyListItemNameInput(journey.item);
  const followed = journey.item.followedFrom;
  const details = [
    journeyPreviewLine(name),
    journey.departure ? `Departs ${formatTime(journey.departure)}` : null,
    followed ? followedFromLabel(followed) : null,
  ]
    .filter((part): part is string => part !== null)
    .join(' · ');
  return (
    <CardLink href={`/journeys/${journey.item.id}`}>
      <StatusRow
        align="flex-start"
        title={journeyDisplayName(name)}
        subtitle={
          details !== '' && (
            <Text size="sm" c="dimmed">
              {details}
            </Text>
          )
        }
        trailing={<JourneyStatusGroupBadge group={homeJourneyStatusGroup(journey)} />}
      />
    </CardLink>
  );
}

/** A lone train's default name (route, date and time), the line under a
 * custom name, as `journeyPreviewLine` gives a journey's. */
function loneTrainDefaultName(entry: HomeLoneTrain): string {
  return trackedTrainDisplayName({ ...entry.train, customName: null });
}

/** A lone train under way, drawn like `CurrentJourneyCard` for a one-leg
 * journey: its name and status badge, then the train it is (route and
 * timetabled departure) and its destination with the arrival ("York
 * (YRK), arrive est. 12:12"). Read in full (`entry.leg`), both facts come
 * from the same helpers as a journey's leg; otherwise from the list row,
 * which has no arrival time. The whole card links to the train's page. */
export function CurrentTrainCard({ entry }: { entry: HomeLoneTrain }) {
  const { train, leg } = entry;
  const route = trackedTrainRouteLabel(train);
  const nowText =
    leg !== null
      ? legNowText(leg)
      : train.pinScheduledDeparture
        ? `${route} · ${formatTime(train.pinScheduledDeparture)}`
        : route;
  const destination = [
    train.pinDestinationCrs ? stationLabel(train.pinDestinationCrs, train.pinDestinationName) : null,
    leg === null ? null : legArrivalText(leg),
  ]
    .filter((part): part is string => part !== null)
    .join(', ');
  return (
    <Box className="currentJourneyCard">
      <CardLink href={trackedTrainHref(train)} padding="lg">
        <Stack gap="sm">
          <Stack gap={4}>
            <StatusRow
              align="flex-start"
              title={
                <Text fw={700} fz="lg" lineClamp={2} miw={0}>
                  {trackedTrainDisplayName(train)}
                </Text>
              }
              trailing={<TrackedRowStatusBadge status={entry.status} />}
            />
            {train.customName && (
              <Text size="sm" c="dimmed">
                {loneTrainDefaultName(entry)}
              </Text>
            )}
          </Stack>
          <dl className="keyFacts keyFacts--compact">
            <div>
              <dt>Now</dt>
              <dd>{nowText}</dd>
            </div>
            {destination !== '' && (
              <div>
                <dt>Destination</dt>
                <dd>{destination}</dd>
              </div>
            )}
          </dl>
        </Stack>
      </CardLink>
    </Box>
  );
}

/** A lone train in a compact list, as `HomeJourneyRow` draws a journey:
 * its name (by default its route, date and time), that default under a
 * custom name, and its badge. Links to the train's page. */
export function HomeTrainRow({ entry }: { entry: HomeLoneTrain }) {
  const { train } = entry;
  return (
    <CardLink href={trackedTrainHref(train)}>
      <StatusRow
        align="flex-start"
        title={trackedTrainDisplayName(train)}
        subtitle={
          train.customName && (
            <Text size="sm" c="dimmed">
              {loneTrainDefaultName(entry)}
            </Text>
          )
        }
        trailing={<TrackedRowStatusBadge status={entry.status} />}
      />
    </CardLink>
  );
}

/** One row of either section: a journey or a lone train. */
function HomeEntryRow({ entry }: { entry: HomeEntry }) {
  return isLoneTrain(entry) ? <HomeTrainRow entry={entry} /> : <HomeJourneyRow journey={entry} />;
}

/** A React key unique across journeys and trains, whose ids are separate
 * spaces. */
function entryKey(entry: HomeEntry): string {
  return isLoneTrain(entry) ? `train-${entry.train.id}` : `journey-${entry.item.id}`;
}

/** "Current journey": the soonest-arriving journey (or lone train) under
 * way, then any others under way at the same time as compact rows.
 * Rendered only when there is one. */
export function CurrentJourneySection({
  current,
  alsoInProgress,
  now,
}: {
  current: HomeCurrent;
  alsoInProgress: readonly HomeEntry[];
  now: Date;
}) {
  return (
    <Stack component="section" aria-labelledby="current-journey-heading" gap="md">
      <SectionTitle id="current-journey-heading">Current journey</SectionTitle>
      {isLoneTrain(current) ? <CurrentTrainCard entry={current} /> : <CurrentJourneyCard journey={current} now={now} />}
      {/* An open break (spec 2026-10-22 §8.1), under the card: the card is
          one link, so the alert's "See options" can't sit inside it. */}
      {!isLoneTrain(current) && current.detail.openBreak && (
        <JourneyBreakBanner
          journeyId={current.detail.id}
          openBreak={current.detail.openBreak}
          legs={current.detail.legs}
          isOwner={current.detail.isOwner}
          suggestionsMuted={current.detail.suggestionsMuted ?? false}
        />
      )}
      {alsoInProgress.length > 0 && (
        <Stack gap="xs">
          <SectionTitle order={3}>Also in progress</SectionTitle>
          {alsoInProgress.map((entry) => (
            <HomeEntryRow key={entryKey(entry)} entry={entry} />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

/** "Upcoming journeys": journeys and trains tracked on their own together,
 * soonest first, capped at `MAX_UPCOMING_JOURNEYS` with "View all" to the
 * Journeys page, which lists every one. Its header carries "New journey",
 * one of the two ways in to `/journeys/new` now the nav has none (DS-115),
 * so the section is rendered for every logged-in visitor: with nothing
 * upcoming it says so, and still offers "New journey" where a new user
 * would look for it. When `loadFailed` (the journeys couldn't be read), it
 * says that instead of claiming there are none, and still lists any lone
 * trains under it. */
export function UpcomingJourneysSection({
  upcoming,
  loadFailed = false,
}: {
  upcoming: readonly HomeEntry[];
  loadFailed?: boolean;
}) {
  const shown = upcoming.slice(0, MAX_UPCOMING_JOURNEYS);
  const hidden = upcoming.length - shown.length;
  return (
    <Stack component="section" aria-labelledby="upcoming-journeys-heading" gap="md">
      <Group justify="space-between" gap="sm">
        <SectionTitle id="upcoming-journeys-heading">Upcoming journeys</SectionTitle>
        <Group gap="md">
          {shown.length > 0 && <TextLink href={JOURNEYS_DESTINATION.href}>View all</TextLink>}
          <ButtonLink href={TRACK_JOURNEY_HREF} variant="light">
            {NEW_JOURNEY_LABEL}
          </ButtonLink>
        </Group>
      </Group>
      {loadFailed ? (
        <Text c="dimmed">{describeFailure('load', 'your journeys')}</Text>
      ) : (
        shown.length === 0 && <Text c="dimmed">No upcoming journeys.</Text>
      )}
      {shown.length > 0 && (
        <Stack gap="xs">
          {shown.map((entry) => (
            <HomeEntryRow key={entryKey(entry)} entry={entry} />
          ))}
        </Stack>
      )}
      {hidden > 0 && (
        <Text size="sm" c="dimmed">
          {hidden} more upcoming {hidden === 1 ? 'journey is' : 'journeys are'} on the Journeys page.
        </Text>
      )}
    </Stack>
  );
}

/** The logged-out home page's "Upcoming journeys": the signed-in section's
 * frame (same heading and id, a link where "View all" sits) around one flat
 * card in the place the journey rows would be, saying what tracking a
 * journey gives you and offering the way in. No example row: the page shows
 * real data or nothing (docs/style-guide.md, "Honest data" and the Empty
 * state), so the card describes a journey rather than drawing a made-up
 * one. Not dismissible: no other prompt on the site is.
 *
 * One button, "Log in or sign up": Authentik's one flow both logs in and
 * enrols a new account (`crates/api/src/routes/auth.rs`), so there is no
 * separate sign-up entry to link. The planner needs no account, so it is a
 * plain link beside the heading. */
export function UpcomingJourneysTeaser() {
  return (
    <Stack component="section" aria-labelledby="upcoming-journeys-heading" gap="md">
      <Group justify="space-between" gap="sm">
        <SectionTitle id="upcoming-journeys-heading">Upcoming journeys</SectionTitle>
        <TextLink href={PLAN_JOURNEY_DESTINATION.href}>{PLAN_JOURNEY_DESTINATION.label}</TextLink>
      </Group>
      <Card {...tokens.card.static}>
        <Stack gap="sm" align="flex-start">
          <Stack gap={4}>
            <Text fw={600}>Track a journey to see it here</Text>
            <Text c="dimmed" size="sm">
              Live delays and platform changes for every leg, a warning when a connection is at risk, other trains if it
              breaks, and help claiming Delay Repay.
            </Text>
          </Stack>
          <LoginControl variant="button">Log in or sign up</LoginControl>
        </Stack>
      </Card>
    </Stack>
  );
}
