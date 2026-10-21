import { Box, Group, Stack, Text } from '@mantine/core';
import { CardLink } from '@/components/CardLink';
import { DelayBadge } from '@/components/DelayBadge';
import { JourneyStatusGroupBadge } from '@/components/JourneyStatusBadge';
import { SectionTitle } from '@/components/SectionTitle';
import { StatusRow } from '@/components/StatusRow';
import { TextLink } from '@/components/TextLink';
import { formatTime } from '@/lib/dateFormat';
import { currentLeg, homeJourneyStatusGroup, MAX_UPCOMING_JOURNEYS, type HomeJourney } from '@/lib/homeJourneys';
import { legDepartureInstant, legExpectedArrivalInstant } from '@/lib/journeyLegChaining';
import { legDestinationArrivalLabel, legEndpointName } from '@/lib/journeyLegLabel';
import {
  journeyDisplayName,
  journeyListItemNameInput,
  journeyNameInputFromLegs,
  journeyPreviewLine,
} from '@/lib/journeyName';
import { legStatusGroup } from '@/lib/journeyStatus';
import { routeLabel, stationLabel } from '@/lib/stationLabel';
import { isUntrackedLeg, untrackedLegPlaces, untrackedLegTimes, untrackedLegTitle } from '@/lib/untrackedLeg';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

/* The logged-in home page's journey sections: "Current journey" (the one
 * under way now) and "Upcoming journeys". `lib/homeJourneys.ts` decides
 * which journey goes where; this file only draws them. Server-rendered,
 * like the rest of the home page. */

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

/** The current leg's own live status: its delay in minutes where a running
 * train has one ("12 min late"), otherwise its status badge. `null` for a
 * single-leg journey with nothing the journey's own badge doesn't say. */
function LegStatus({ leg, legCount }: { leg: JourneyLegDetail; legCount: number }) {
  const group = legStatusGroup(leg);
  const delay = leg.trackedTrainState?.delayMinutes ?? null;
  if (group === 'delayed' && delay !== null) return <DelayBadge delayMinutes={delay} />;
  if (legCount < 2) return null;
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
  const name = journeyNameInputFromLegs(detail.customName, detail.legs);
  const preview = journeyPreviewLine(name);
  const leg = currentLeg(detail, now);
  const legIndex = leg === null ? -1 : detail.legs.indexOf(leg);
  const isLastLeg = legIndex === detail.legs.length - 1;
  const end = leg === null ? null : legEndText(leg);
  const arrival = leg === null ? null : legArrivalText(leg);
  const endFact = [end, arrival].filter((part): part is string => part !== null).join(', ');

  return (
    <Box className="currentJourneyCard">
      <CardLink href={`/journeys/${detail.id}`} padding="lg">
        <Stack gap="sm">
          <StatusRow
            align="flex-start"
            title={
              <Text fw={700} fz="lg" lineClamp={2} miw={0}>
                {journeyDisplayName(name)}
              </Text>
            }
            subtitle={
              preview && (
                <Text size="sm" c="dimmed">
                  {preview}
                </Text>
              )
            }
            trailing={<JourneyStatusGroupBadge group={homeJourneyStatusGroup(journey)} />}
          />
          {leg !== null && (
            <dl className="keyFacts keyFacts--compact">
              <div>
                <dt>{detail.legs.length > 1 ? `Now, leg ${legIndex + 1} of ${detail.legs.length}` : 'Now'}</dt>
                <dd>
                  <Group gap="xs" wrap="wrap">
                    <span>{legNowText(leg)}</span>
                    <LegStatus leg={leg} legCount={detail.legs.length} />
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
        </Stack>
      </CardLink>
    </Box>
  );
}

/** One journey in a compact list: its name, the route and date a custom
 * name hides (`journeyPreviewLine`), its departure time when it was read
 * in full, and its whole-journey status (`homeJourneyStatusGroup`). Links
 * to the journey page. */
export function HomeJourneyRow({ journey }: { journey: HomeJourney }) {
  const name = journeyListItemNameInput(journey.item);
  const details = [journeyPreviewLine(name), journey.departure ? `Departs ${formatTime(journey.departure)}` : null]
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

/** "Current journey": the soonest-arriving journey under way, then any
 * others under way at the same time as compact rows. Rendered only when
 * there is one. */
export function CurrentJourneySection({
  current,
  alsoInProgress,
  now,
}: {
  current: HomeJourney & { detail: JourneyDetail };
  alsoInProgress: readonly HomeJourney[];
  now: Date;
}) {
  return (
    <Stack component="section" aria-labelledby="current-journey-heading" gap="md">
      <SectionTitle id="current-journey-heading">Current journey</SectionTitle>
      <CurrentJourneyCard journey={current} now={now} />
      {alsoInProgress.length > 0 && (
        <Stack gap="xs">
          <SectionTitle order={3}>Also in progress</SectionTitle>
          {alsoInProgress.map((journey) => (
            <HomeJourneyRow key={journey.item.id} journey={journey} />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

/** "Upcoming journeys", soonest first, capped at `MAX_UPCOMING_JOURNEYS`
 * with "View all" to the Track page, which lists every journey. Rendered
 * only when there is one. */
export function UpcomingJourneysSection({ upcoming }: { upcoming: readonly HomeJourney[] }) {
  const shown = upcoming.slice(0, MAX_UPCOMING_JOURNEYS);
  const hidden = upcoming.length - shown.length;
  return (
    <Stack component="section" aria-labelledby="upcoming-journeys-heading" gap="md">
      <Group justify="space-between">
        <SectionTitle id="upcoming-journeys-heading">Upcoming journeys</SectionTitle>
        <TextLink href="/track/mine">View all</TextLink>
      </Group>
      <Stack gap="xs">
        {shown.map((journey) => (
          <HomeJourneyRow key={journey.item.id} journey={journey} />
        ))}
      </Stack>
      {hidden > 0 && (
        <Text size="sm" c="dimmed">
          {hidden} more upcoming {hidden === 1 ? 'journey is' : 'journeys are'} on the Track page.
        </Text>
      )}
    </Stack>
  );
}
