import { Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { JourneyCreationFlow } from '@/components/JourneyCreationFlow';
import type { UntrackedLegOrigin } from '@/components/UntrackedLegStartForm';
import { getPlannerOperators, getTflStop, getTrainSearchDates } from '@/lib/api';
import { parseJourneyEntryMode } from '@/lib/journeyEntryMode';
import { PLAN_JOURNEY_HREF, PLAN_JOURNEY_LABEL } from '@/lib/navLinks';
import { OWN_PREVIEW_IMAGE, pageMetadata } from '@/lib/pageMetadata';
import { JOURNEY_FROM_TFL_STOP_PARAM, TFL_STOP_ID } from '@/lib/tflStops';
import { parsePlanSearchParams } from '@/lib/tripPlanUrl';
import type { TransitMode } from '@/lib/types';

/** `/plan`, "Plan a journey": where every journey starts. Reached from the
 * button at the top of the Journeys page and of the home page's "Upcoming
 * journeys", and from station and stop pages; not a nav entry (DS-194).
 *
 * DS-194 merged two pages into this one. `/plan` was the trip planner on
 * its own; `/journeys/new` ("New journey") offered the same planner beside
 * "I know my route" (`TrackTrainForm`) and "Tube, bus or walk"
 * (`UntrackedLegStartForm`). Both rendered `JourneyCreationFlow`, so the
 * merge is that component with every option, the planner first. Old
 * `/journeys/new` links redirect here with their query
 * (`next.config.mjs`); every parameter either page read is read here:
 *
 * - `?mode=` picks the option (`lib/journeyEntryMode.ts`): `plan` (the
 *   default), `known`, `untracked`, or `/track`'s `pick` and `window`.
 * - `?fromTflStop=` (a TfL stop page's "Add as a journey leg") opens "Tube,
 *   bus or walk" from that stop, whatever `?mode=` says.
 * - `?origin=CRS` pre-fills From (a station page's "Plan a journey from
 *   here"); the rest of the query is the planner's last search
 *   (`lib/tripPlanUrl.ts`), which each search writes into the address bar,
 *   so a shared or bookmarked URL reopens the same form.
 *
 * None of them is read in metadata: a per-visitor value must not leak into
 * link previews, and the metadata describes the page as a whole.
 *
 * How a journey gets saved (leg 1 straight away, then "Add a leg" one at a
 * time, or a planned route's legs in one atomic `POST /Journeys/itinerary`)
 * is `JourneyCreationFlow`'s and `PlanTripFlow`'s doc comments' subject.
 *
 * Planning needs no account (`GET /Trips/plan` is unauthenticated); only
 * saving does. Each form shows its button to everyone and gates on the real
 * 401 (`useNeedsLogin`), whose login prompt returns here with the query. */
const METADATA_DESCRIPTION =
  'Plan a train journey or start from a train you know, then track it live from start to finish.';

export const metadata: Metadata = pageMetadata(PLAN_JOURNEY_LABEL, METADATA_DESCRIPTION, {
  ...OWN_PREVIEW_IMAGE,
  path: PLAN_JOURNEY_HREF,
});

const CRS_PATTERN = /^[A-Za-z]{3}$/;

/** The TfL stop `?fromTflStop=` names (a stop page's "Add as a journey
 * leg"), as the untracked form's starting point: its first mode and its
 * name. `null` for no stop, a malformed ID or one the api doesn't know,
 * which leaves the page as it always opens. */
async function untrackedOriginFrom(raw: string | string[] | undefined): Promise<UntrackedLegOrigin | null> {
  const id = Array.isArray(raw) ? raw[0] : raw;
  if (!id || !TFL_STOP_ID.test(id)) return null;
  const stop = await getTflStop(id).catch(() => null);
  if (!stop) return null;
  const mode: TransitMode = (['tube', 'dlr', 'tram'] as const).find((m) => stop.modes.includes(m)) ?? 'tube';
  return { stopId: stop.id, name: stop.name, mode };
}

export default async function PlanPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const { origin } = params;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const planOrigin = originParam && CRS_PATTERN.test(originParam) ? originParam.toUpperCase() : undefined;
  const planQuery = parsePlanSearchParams(params);
  const { entryMode, knownRouteMode } = parseJourneyEntryMode(params.mode);
  // The planner's date picker's last day: what the timetable search accepts
  // (`getTrainSearchDates`, cached for a few minutes), as on `/trains`. A
  // failure leaves `null`, and the picker falls back to a week ahead.
  // The operator filter's choices (`GET /Trips/operators`, cached for a
  // few minutes); `null` on failure, and the field says it can't offer
  // them.
  const [searchDates, untrackedOrigin, operators] = await Promise.all([
    getTrainSearchDates().catch(() => null),
    untrackedOriginFrom(params[JOURNEY_FROM_TFL_STOP_PARAM]),
    getPlannerOperators().catch(() => null),
  ]);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>{PLAN_JOURNEY_LABEL}</Title>
      <Text c="dimmed">
        Find routes between two stations, or start from a train you already know. Once the first leg is tracked you can
        add another for a change of trains, or stop there: a single train is already a complete journey.
      </Text>
      <Text size="sm" c="dimmed">
        You don&apos;t need an account to plan. To track a journey, you&apos;ll be asked to log in when you save it.
      </Text>
      <JourneyCreationFlow
        initialEntryMode={entryMode}
        knownRouteMode={knownRouteMode}
        planOrigin={planOrigin}
        planQuery={planQuery}
        planSearchDates={searchDates}
        planOperators={operators}
        untrackedOrigin={untrackedOrigin}
      />
    </Stack>
  );
}
