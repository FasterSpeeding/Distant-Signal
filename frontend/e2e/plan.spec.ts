import { test, expect, type Page } from '@playwright/test';

// /plan in arrive-by mode, end to end against the real api and the seed's
// tiny timetable (frontend/e2e/fixtures/seed.sql, "A tiny timetable for
// /plan"): on 2026-09-17, ZEA to ZEC at 07:30, 08:00 and 08:30, each taking
// 50 minutes. Spec: docs/superpowers/specs/2026-10-19-trips-plan-arrive-by-design.md.
//
// The browser's clock is fixed on the seed's date, so the date picker
// (whose first day is today) keeps the date the URL restores.
//
// `/Trips/plan` is rate limited per client (20 a minute, burst 10), so the
// whole file makes ten plan requests.

const FIXED_NOW = new Date('2026-09-17T06:00:00Z'); // 07:00 in London

async function openPlan(page: Page, query: string) {
  await page.clock.setFixedTime(FIXED_NOW);
  await page.goto(`/plan?${query}`);
  await expect(page.getByRole('heading', { level: 1, name: 'Plan a journey' })).toBeVisible();
}

async function findRoutes(page: Page) {
  const planned = page.waitForResponse((response) => response.url().includes('/api/Trips/plan'));
  await page.getByRole('button', { name: 'Find routes' }).click();
  return planned;
}

test('plans by arrival time, then gathers later and earlier routes around it', async ({ page }) => {
  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-17&arriveBy=09:00');
  await expect(page.getByRole('radio', { name: 'Arrive by' })).toBeChecked();
  await expect(page.getByLabel('Time', { exact: true })).toHaveValue('09:00');

  const response = await findRoutes(page);
  const sent = new URL(response.url()).searchParams;
  expect(sent.get('arriveBy')).toBe('09:00:00');
  expect(sent.has('departAfter')).toBe(false);

  // The latest train arriving by 09:00: the 08:00, arriving 08:50.
  const summary = page.getByTestId('plan-timing-summary');
  const cards = page.locator('[data-plan-entry]');
  await expect(summary).toHaveText('Arrive by 09:00, 17 Sep 2026');
  await expect(cards).toHaveCount(1);
  await expect(cards.nth(0).getByText(/^08:00 .*08:50$/)).toBeVisible();
  await expect(page).toHaveURL(/arriveBy=09%3A00/);

  // Later: departs after 08:00, so the 08:30, added below.
  let paged = page.waitForResponse((r) => r.url().includes('/api/Trips/plan'));
  await page.getByRole('button', { name: 'Later routes' }).click();
  expect(new URL((await paged).url()).searchParams.get('departAfter')).toBe('08:01:00');
  await expect(cards).toHaveCount(2);
  await expect(cards.nth(1).getByText(/^08:30 .*09:20$/)).toBeVisible();

  // Earlier: arrives before 08:50, so the 07:30, added above.
  paged = page.waitForResponse((r) => r.url().includes('/api/Trips/plan'));
  await page.getByRole('button', { name: 'Earlier routes' }).click();
  expect(new URL((await paged).url()).searchParams.get('arriveBy')).toBe('08:49:00');
  await expect(cards).toHaveCount(3);
  await expect(cards.nth(0).getByText(/^07:30 .*08:20$/)).toBeVisible();

  // The visitor's search is still what the page, form and URL say.
  await expect(summary).toHaveText('Arrive by 09:00, 17 Sep 2026');
  await expect(page.getByRole('radio', { name: 'Arrive by' })).toBeChecked();
  await expect(page).toHaveURL(/arriveBy=09%3A00/);

  // Nothing earlier or later on the seed's day.
  await page.getByRole('button', { name: 'Earlier routes' }).click();
  await expect(page.getByText('No earlier journeys on this day.')).toBeVisible();
  await page.getByRole('button', { name: 'Later routes' }).click();
  await expect(page.getByText('No later journeys on this day.')).toBeVisible();
  await expect(cards).toHaveCount(3);
});

test('the toggle switches a search from departure to arrival time', async ({ page }) => {
  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-17&departAfter=07:00');
  await expect(page.getByRole('radio', { name: 'Depart at' })).toBeChecked();

  await page.getByText('Arrive by', { exact: true }).click();
  await page.getByLabel('Time', { exact: true }).fill('08:30');
  await findRoutes(page);

  // Nothing leaves later and still arrives by 08:30 than the 07:30.
  await expect(page.getByTestId('plan-timing-summary')).toHaveText('Arrive by 08:30, 17 Sep 2026');
  await expect(page.getByText(/^07:30 .*08:20$/)).toBeVisible();
  await expect(page).toHaveURL(/arriveBy=08%3A30/);
});

test('says when nothing arrives in time, with what to try', async ({ page }) => {
  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-17&arriveBy=08:00');
  await findRoutes(page);
  await expect(page.getByText('Try a later arrival time, or search by departure time.')).toBeVisible();
});

test('an arrival just after midnight searches both dates', async ({ page }) => {
  // By 00:50 on the 18th: the 17th's timetable has nothing after
  // midnight, the 18th's 00:10 gets there. The request is the 17th's
  // trains with dayOffset=1, plus the 18th's (acrossMidnight).
  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-18&arriveBy=00:50');
  const response = await findRoutes(page);
  const sent = new URL(response.url()).searchParams;
  expect(sent.get('date')).toBe('2026-09-17');
  expect(sent.get('dayOffset')).toBe('1');
  expect(sent.get('acrossMidnight')).toBe('true');
  await expect(page.getByTestId('plan-timing-summary')).toHaveText('Arrive by 00:50, 18 Sep 2026');
  await expect(page.getByText(/^00:10 .*00:40$/)).toBeVisible();
});

// Compare options (DS-196): the seed's "Two routes for /plan's Compare
// options", ZOA to ZOD, fast via ZOS and 20 minutes slower via ZOC.
test('compare options lists another route besides the fastest, labelled', async ({ page }) => {
  await openPlan(page, 'origin=ZOA&destination=ZOD&date=2026-09-17&departAfter=07:30');
  await page.getByText('Compare options', { exact: true }).click();
  const response = await findRoutes(page);
  expect(new URL(response.url()).searchParams.get('results')).toBe('options');

  const cards = page.locator('[data-plan-entry]');
  await expect(cards).toHaveCount(2);
  await expect(cards.nth(0).getByText(/^08:00 .*09:00$/)).toBeVisible();
  await expect(cards.nth(0).getByText('Fastest', { exact: true })).toBeVisible();
  await expect(cards.nth(1).getByText(/^08:05 .*09:20$/)).toBeVisible();
  await expect(cards.nth(1).getByText(/^Via /)).toBeVisible();
});

// The operator filter (2026-10-31): the seed's Compare options routes are
// run by VT and GR (`schedule_services`). The filter is restored from the
// URL into Advanced options and sent; the plan is answered here, as the api
// would answer it (its DB tests cover the search), so this file stays
// within `/Trips/plan`'s rate limit.
test('an operator filter from the URL avoids an operator, and says so', async ({ page }) => {
  let sent: URLSearchParams | null = null;
  await page.route('**/api/Trips/plan?*', async (route) => {
    sent = new URL(route.request().url()).searchParams;
    await route.fulfill({ json: avoidVtPlan });
  });
  await openPlan(page, 'origin=ZOA&destination=ZOD&date=2026-09-17&departAfter=07:30&avoidOperator=VT');
  await expect(page.getByRole('button', { name: /Advanced options ?\(1 set\)/ })).toBeVisible();
  await expect(page.getByText('Avoid VT', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Find routes' }).click();

  await expect(page.getByText('Showing routes that avoid VT.')).toBeVisible();
  expect(sent!.get('avoidOperator')).toBe('VT');
  const cards = page.locator('[data-plan-entry]');
  await expect(cards).toHaveCount(1);
  await expect(cards.nth(0).getByText(/^08:05 .*09:20$/)).toBeVisible();
  await expect(cards.nth(0).locator('[data-leg-operator]')).toHaveText('GR');

  // "Change" opens Advanced options at the operator field.
  await page.getByRole('button', { name: 'Change' }).click();
  await expect(page.getByRole('radio', { name: 'Avoid these' })).toBeChecked();
  await expect(page.getByRole('radio', { name: 'Avoid these' })).toBeFocused();
});

// Prefer (2026-10-31, P3): Compare options with GR preferred. The api's
// answer (fulfilled here, as above) adds GR's route as a labelled option.
test('preferring an operator adds its route to Compare options, labelled', async ({ page }) => {
  let sent: URLSearchParams | null = null;
  await page.route('**/api/Trips/plan?*', async (route) => {
    sent = new URL(route.request().url()).searchParams;
    await route.fulfill({ json: preferGrPlan });
  });
  await openPlan(
    page,
    'origin=ZOA&destination=ZOD&date=2026-09-17&departAfter=07:30&results=options&preferOperator=GR',
  );
  await expect(page.getByText('Prefer GR', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Find routes' }).click();
  const cards = page.locator('[data-plan-entry]');
  await expect(cards).toHaveCount(2);
  expect(sent!.get('preferOperator')).toBe('GR');
  expect(sent!.get('results')).toBe('options');
  await expect(cards.nth(1).getByText('GR only', { exact: true })).toBeVisible();
  await expect(page.locator('[data-operator-filter-banner]')).toHaveCount(0);
});

/** `/Trips/plan`'s answer for ZOA to ZOD with `preferOperator=GR`: VT's
 * fastest route, then GR's, labelled `operatorOnly`. */
const preferGrPlan = {
  ...avoidVtPlanBase(),
  results: 'options',
  avoidOperator: [],
  preferOperator: ['GR'],
  preferOperatorResult: 'added',
  segments: [
    {
      originCrs: 'ZOA',
      destinationCrs: 'ZOD',
      cappedByMaxChanges: false,
      noResultReason: null,
      itineraries: [
        { ...trainItinerary('E2EOP1', 'VT', '08:00:00', '09:00:00', 60), labels: [{ kind: 'fastest' }] },
        {
          ...trainItinerary('E2EOP2', 'GR', '08:05:00', '09:20:00', 75),
          labels: [{ kind: 'operatorOnly', operators: ['GR'] }],
        },
      ],
    },
  ],
  journeys: [],
};

/** One direct train itinerary, as the api serves it (trimmed). */
function trainItinerary(uid: string, operator: string, departure: string, arrival: string, minutes: number) {
  return {
    legs: [
      {
        kind: 'train',
        trainUid: uid,
        serviceDate: '2026-09-17',
        serviceMode: 'train',
        originCrs: 'ZOA',
        destinationCrs: 'ZOD',
        scheduledDeparture: departure,
        scheduledArrival: arrival,
        publicDeparture: departure,
        publicArrival: arrival,
        departureDayOffset: 0,
        arrivalDayOffset: 0,
        operator,
      },
    ],
    changeCount: 0,
    totalDurationMinutes: minutes,
  };
}

/** The top-level fields every plan answer here shares. */
function avoidVtPlanBase() {
  return {
    maxChanges: 4,
    operator: [],
    operatorGroups: {},
    operatorsNotRunning: [],
    serviceDates: ['2026-09-17'],
  };
}

/** `/Trips/plan`'s answer for ZOA to ZOD from 07:30 avoiding VT: only GR's
 * 08:05 via ZOC (the api's own shape, trimmed to what the page reads). */
const avoidVtPlan = {
  results: 'fastest',
  maxChanges: 4,
  operator: [],
  avoidOperator: ['VT'],
  operatorGroups: {},
  operatorsNotRunning: [],
  serviceDates: ['2026-09-17'],
  segments: [
    {
      originCrs: 'ZOA',
      destinationCrs: 'ZOD',
      cappedByMaxChanges: false,
      noResultReason: null,
      itineraries: [
        {
          legs: [
            {
              kind: 'train',
              trainUid: 'E2EOP2',
              serviceDate: '2026-09-17',
              serviceMode: 'train',
              originCrs: 'ZOA',
              destinationCrs: 'ZOD',
              scheduledDeparture: '08:05:00',
              scheduledArrival: '09:20:00',
              publicDeparture: '08:05:00',
              publicArrival: '09:20:00',
              departureDayOffset: 0,
              arrivalDayOffset: 0,
              operator: 'GR',
            },
          ],
          changeCount: 0,
          totalDurationMinutes: 75,
        },
      ],
    },
  ],
  journeys: [
    {
      changeCount: 0,
      departure: { time: '08:05:00', dayOffset: 0 },
      arrival: { time: '09:20:00', dayOffset: 0 },
      totalDurationMinutes: 75,
    },
  ],
};

// Rail, tram, rail (transit trip planning §11): the seed's "A rail, tram,
// rail route for /plan": ZTA 06:20 to MAN 06:50 (E2ETR1), the Blue Line
// from Piccadilly (9400ZZMAPIC) 07:02 to Eccles (9400ZZMAECC) 07:27,
// and ZTE 07:45 to ZTB 08:15 (E2ETR2), with walks between MAN and
// Piccadilly and between Eccles and ZTE (`transit_rail_links`). The api
// plans it for real; saving is answered here, so no journey is left behind
// for the other specs and screenshots.
test('plans a route with a tram in the middle, and saves the tram as an untracked leg', async ({ page }) => {
  let saved: { legs: Record<string, unknown>[] } | null = null;
  await page.route('**/api/Journeys/itinerary', async (route) => {
    saved = route.request().postDataJSON() as { legs: Record<string, unknown>[] };
    await route.fulfill({ json: { journeyId: 990001, legId: 1, trackingId: null, resolutionStatus: null } });
  });
  await page.route('**/api/Journeys/990001', (route) => route.fulfill({ status: 404, body: 'Not found' }));

  await openPlan(page, 'origin=ZTA&destination=ZTB&date=2026-09-17&departAfter=06:15');
  const response = await findRoutes(page);
  expect(new URL(response.url()).searchParams.get('transit')).toBe('true');

  const card = page.locator('[data-plan-entry]').first();
  await expect(card.getByText('07:02 Tram (Blue Line) Piccadilly to Eccles 07:27')).toBeVisible();
  await expect(card.getByText('Tram', { exact: true })).toBeVisible();
  await expect(card.getByText(/^Walk to Piccadilly tram stop, \d+ min$/)).toBeVisible();
  await expect(card.getByText(/^Walk from Eccles tram stop, \d+ min$/)).toBeVisible();
  await expect(card.getByText(/^About \d+ min walk, estimated$/).first()).toBeVisible();
  await expect(card.getByText('Scheduled times, not live')).toBeVisible();
  await expect(card).not.toContainText(/on time|expected|late/i);

  // The card's stretched label covers its radio (a clickable card).
  await card.getByRole('radio').check({ force: true });
  await page.getByRole('button', { name: 'Track this journey' }).click();
  await expect(page.getByText('First leg tracked')).toBeVisible();

  expect(saved).not.toBeNull();
  const legs = saved!.legs;
  expect(legs.map((leg) => leg.transitMode ?? 'train')).toEqual(['train', 'walk', 'tram', 'walk', 'train']);
  expect(legs[0]).toMatchObject({
    trainUid: 'E2ETR1',
    serviceDate: '2026-09-17',
    originCrs: 'ZTA',
    destinationCrs: 'MAN',
  });
  // A walk to a stop is named after it, without its id: the api takes a
  // stop id only on a ride.
  expect(legs[1]).toMatchObject({ kind: 'untracked', destinationName: 'Piccadilly' });
  expect(legs[1]).not.toHaveProperty('destinationStopId');
  // The tram's own timetabled times (07:02 to 07:27 BST), its line, stations
  // and network ride.
  expect(legs[2]).toEqual({
    kind: 'untracked',
    transitMode: 'tram',
    originName: 'Piccadilly',
    destinationName: 'Eccles',
    lineName: 'Blue Line',
    originStopId: '9400ZZMAPIC',
    destinationStopId: '9400ZZMAECC',
    plannedDeparture: '2026-09-17T06:02:00.000Z',
    plannedArrival: '2026-09-17T06:27:00.000Z',
    // Its network ride, to find it in the timetable again (DS-237).
    transitNetwork: 'metrolink',
    transitLineId: 'blue',
    headsign: 'Eccles',
  });
  expect(legs[3]).toMatchObject({ kind: 'untracked', originName: 'Eccles' });
  expect(legs[3]).not.toHaveProperty('originStopId');
  expect(legs[4]).toMatchObject({ trainUid: 'E2ETR2', originCrs: 'ZTE', destinationCrs: 'ZTB' });
});

// A timetable TRANSFER link between two different stations (Euston to
// Victoria is a 19-minute one) is a journey of its own: saving the plan
// keeps it as an untracked `transfer` leg on its times, not a change within
// one station to drop. The plan (made-up codes) and the save are answered
// here, so this spends none of the file's `/Trips/plan` budget.
test('saves a TRANSFER link between two stations as an untracked transfer leg', async ({ page }) => {
  const train = (trainUid: string, originCrs: string, destinationCrs: string, dep: string, arr: string) => ({
    kind: 'train',
    trainUid,
    serviceDate: '2026-09-17',
    originCrs,
    destinationCrs,
    scheduledDeparture: dep,
    scheduledArrival: arr,
    publicDeparture: dep,
    publicArrival: arr,
    arrivalDayOffset: 0,
  });
  await page.route('**/api/Trips/plan?*', (route) =>
    route.fulfill({
      json: {
        results: 'fastest',
        segments: [
          {
            originCrs: 'ZEA',
            destinationCrs: 'ZEC',
            cappedByMaxChanges: false,
            itineraries: [
              {
                legs: [
                  train('E2EXF1', 'ZEA', 'ZXB', '08:00:00', '08:40:00'),
                  { kind: 'transfer', mode: 'TRANSFER', originCrs: 'ZXB', destinationCrs: 'ZXC', minutes: 19 },
                  train('E2EXF2', 'ZXC', 'ZEC', '09:10:00', '09:50:00'),
                ],
                changeCount: 1,
                totalDurationMinutes: 110,
              },
            ],
          },
        ],
      },
    }),
  );
  let saved: { legs: Record<string, unknown>[] } | null = null;
  await page.route('**/api/Journeys/itinerary', async (route) => {
    saved = route.request().postDataJSON() as { legs: Record<string, unknown>[] };
    await route.fulfill({ json: { journeyId: 990002, legId: 1, trackingId: null, resolutionStatus: null } });
  });
  await page.route('**/api/Journeys/990002', (route) => route.fulfill({ status: 404, body: 'Not found' }));

  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-17&departAfter=07:30');
  await findRoutes(page);
  const card = page.locator('[data-plan-entry]').first();
  await expect(card.getByText(/^Change ZXB to ZXC, 19 min$/)).toBeVisible();
  await card.getByRole('radio').check({ force: true });
  await page.getByRole('button', { name: 'Track this journey' }).click();
  await expect(page.getByText('First leg tracked')).toBeVisible();

  expect(saved).not.toBeNull();
  expect(saved!.legs).toEqual([
    { trainUid: 'E2EXF1', serviceDate: '2026-09-17', originCrs: 'ZEA', destinationCrs: 'ZXB' },
    // From the first train's arrival (08:40 BST) for 19 minutes.
    {
      kind: 'untracked',
      transitMode: 'transfer',
      originName: 'ZXB',
      destinationName: 'ZXC',
      plannedDeparture: '2026-09-17T07:40:00.000Z',
      plannedArrival: '2026-09-17T07:59:00.000Z',
    },
    { trainUid: 'E2EXF2', serviceDate: '2026-09-17', originCrs: 'ZXC', destinationCrs: 'ZEC' },
  ]);
});

// Wimbledon to Milton Keynes by the District line, as production planned
// it on 2026-11-02 (made-up codes and times here): a walk into a TfL
// station, a timetabled Tube ride, a walk out, then a train. The save is
// answered as the api's validator would (`journeys::attach_untracked_stops`):
// a stop id on a leg whose mode can't have one is a 400, "Only a tube, dlr,
// tram, metro leg can have a stop id." Each walk once carried its stop's id
// and the save failed.
test('saves a plan with walks to and from a Tube station, the stop ids only on the Tube ride', async ({ page }) => {
  const walk = (end: 'to' | 'from', crs: string, stop: { id: string; name: string }) => ({
    kind: 'transfer',
    mode: 'WALK',
    originCrs: end === 'to' ? crs : null,
    destinationCrs: end === 'to' ? null : crs,
    minutes: 4,
    transitWalk: { network: 'tfl', stop, towardsStop: end === 'to', walkMinutes: 2, estimated: true },
  });
  const testbury = { id: '940GZZLUTST', name: 'Testbury' };
  const westTest = { id: '940GZZLUWTS', name: 'West Test' };
  await page.route('**/api/Trips/plan?*', (route) =>
    route.fulfill({
      json: {
        results: 'fastest',
        segments: [
          {
            originCrs: 'ZEA',
            destinationCrs: 'ZEC',
            cappedByMaxChanges: false,
            itineraries: [
              {
                legs: [
                  walk('to', 'ZEA', testbury),
                  {
                    kind: 'transit',
                    network: 'tfl',
                    serviceMode: 'tube',
                    liveTracking: false,
                    realtime: false,
                    serviceDate: '2026-09-17',
                    scheduledDeparture: '08:09:00',
                    scheduledArrival: '08:25:00',
                    departureDayOffset: 0,
                    arrivalDayOffset: 0,
                    line: { id: 'tfl:district', name: 'District', colour: '#13843f' },
                    headsign: null,
                    originCrs: null,
                    destinationCrs: null,
                    originStop: { ...testbury, platform: null },
                    destinationStop: { ...westTest, platform: null },
                    headwayMinutes: 4,
                    notices: [],
                  },
                  walk('from', 'ZWT', westTest),
                  {
                    kind: 'train',
                    trainUid: 'E2EWM1',
                    serviceDate: '2026-09-17',
                    originCrs: 'ZWT',
                    destinationCrs: 'ZEC',
                    scheduledDeparture: '08:47:00',
                    scheduledArrival: '09:50:00',
                    publicDeparture: '08:47:00',
                    publicArrival: '09:50:00',
                    arrivalDayOffset: 0,
                  },
                ],
                changeCount: 1,
                totalDurationMinutes: 110,
              },
            ],
          },
        ],
      },
    }),
  );
  const RIDES_WITH_TFL_STOPS = ['tube', 'dlr', 'tram', 'metro'];
  let saved: { legs: Record<string, unknown>[] } | null = null;
  await page.route('**/api/Journeys/itinerary', async (route) => {
    saved = route.request().postDataJSON() as { legs: Record<string, unknown>[] };
    const refused = saved.legs.some(
      (leg) =>
        (leg.originStopId !== undefined || leg.destinationStopId !== undefined) &&
        !RIDES_WITH_TFL_STOPS.includes(String(leg.transitMode)),
    );
    await (refused
      ? route.fulfill({
          status: 400,
          json: { error: 'invalid_itinerary', message: 'Only a tube, dlr, tram, metro leg can have a stop id.' },
        })
      : route.fulfill({ json: { journeyId: 990003, legId: 1, trackingId: null, resolutionStatus: null } }));
  });
  await page.route('**/api/Journeys/990003', (route) => route.fulfill({ status: 404, body: 'Not found' }));

  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-17&departAfter=07:30');
  await findRoutes(page);
  const card = page.locator('[data-plan-entry]').first();
  await card.getByRole('radio').check({ force: true });
  await page.getByRole('button', { name: 'Track this journey' }).click();
  await expect(page.getByText('First leg tracked')).toBeVisible();
  await expect(page.getByText("Couldn't track this journey")).toHaveCount(0);

  expect(saved).not.toBeNull();
  expect(saved!.legs.map((leg) => leg.transitMode ?? 'train')).toEqual(['walk', 'tube', 'walk', 'train']);
  expect(saved!.legs[0]).toMatchObject({ destinationName: 'Testbury' });
  expect(saved!.legs[0]).not.toHaveProperty('destinationStopId');
  expect(saved!.legs[1]).toMatchObject({ originStopId: '940GZZLUTST', destinationStopId: '940GZZLUWTS' });
  expect(saved!.legs[2]).toMatchObject({ originName: 'West Test' });
  expect(saved!.legs[2]).not.toHaveProperty('originStopId');
});
