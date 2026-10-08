import { Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { JourneyCreationFlow } from '@/components/JourneyCreationFlow';
import { TextLink } from '@/components/TextLink';
import { getTrainSearchDates } from '@/lib/api';
import { TRACK_JOURNEY_DESTINATION } from '@/lib/navLinks';
import { parsePlanSearchParams } from '@/lib/tripPlanUrl';

/** `/plan` -- the trip planner (`PlanTripFlow`) on its own page, linked from
 * the nav as `PLAN_JOURNEY_DESTINATION` (`lib/navLinks.ts`).
 *
 * A route of its own rather than a `?mode=plan` switch on `/journeys/new`:
 *
 * 1. That page's heading and copy are about TRACKING, which needs an
 *    account. Planning doesn't (`GET /Trips/plan` is unauthenticated), so it
 *    needs different copy, a different `<h1>` and different link-preview
 *    metadata -- all static here, with no branching on a query param.
 * 2. Nav active state is a pathname match (`isActiveNavHref`). A query-param
 *    mode would light up "Track a Journey" and "Plan a Journey" together, or
 *    need `useSearchParams` (and its `<Suspense>` requirement) in the nav.
 * 3. A path is the stable thing to share. `/journeys/new?mode=plan` still
 *    works: `next.config.mjs` redirects it here.
 *
 * The page renders `JourneyCreationFlow` in `planOnly` mode, so once a
 * planned route is saved the visitor gets the same "add a leg / Done" view
 * `/journeys/new` gives.
 *
 * Signed-out visitors can plan and compare routes; only "Track this
 * journey" needs a session. `PlanTripFlow` already handles that: the
 * `POST /Journeys` 401 opens `LoginPromptModal`, whose login link returns
 * here (`useLoginHref` keeps the path and query). The copy below says so up
 * front instead of implying an account is needed to plan.
 *
 * `?origin=CRS` pre-fills From, the same parameter `/track` takes, for a
 * station page's "Plan a journey from here". Anything that isn't a
 * three-letter code is ignored. Not read in metadata, for the reason
 * `/track` gives: a per-visitor value must not leak into link previews.
 *
 * The rest of the query is the last search (`lib/tripPlanUrl.ts`): each
 * search writes it into the address bar, so a shared or bookmarked `/plan`
 * URL reopens the same form, Advanced options included. */
const METADATA_TITLE = 'Plan a Journey — Distant Signal';
const METADATA_DESCRIPTION =
  'Find train routes between any two UK stations, with changes and optional stops on the way, and compare the options. No account needed to plan; log in to track the journey you pick.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

const CRS_PATTERN = /^[A-Za-z]{3}$/;

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
  // The date picker's last day: what the timetable search accepts
  // (`getTrainSearchDates`, cached for a few minutes), as on `/trains`. A
  // failure leaves `null`, and the picker falls back to a week ahead.
  const searchDates = await getTrainSearchDates().catch(() => null);

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Plan a Journey</Title>
      <Text c="dimmed">
        Find routes between two stations, including changes and any stops you want on the way, then compare the options.
        Already know which train you&apos;re catching?{' '}
        <TextLink href={TRACK_JOURNEY_DESTINATION.href} underline="always" inline>
          Track it directly
        </TextLink>
        .
      </Text>
      <Text size="sm" c="dimmed">
        You don&apos;t need an account to plan. To track the route you pick, you&apos;ll be asked to log in when you
        save it.
      </Text>
      <JourneyCreationFlow planOnly planOrigin={planOrigin} planQuery={planQuery} planSearchDates={searchDates} />
    </Stack>
  );
}
