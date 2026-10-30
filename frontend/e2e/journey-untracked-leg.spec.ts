import { test, expect, type Page, type Route } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// "Tube, bus or walk" legs (spec
// docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
// §2): adding an untracked Tube leg after a train leg on /plan,
// and starting a journey with one. Every /api call the page makes from the
// browser is answered here, as in journey-add-leg.spec.ts, so the spec
// needs no seeded journey and no session.

const STATIONS = [
  { code: 'EUS', name: 'London Euston' },
  { code: 'MKC', name: 'Milton Keynes Central' },
];

/** `GET /tfl/stops` rows (invented ids): the TfL stop lookup. */
const TFL_STOPS = [
  { id: '940GZZTESTE', name: 'Euston', modes: ['tube'], lines: [{ id: 'northern', name: 'Northern' }] },
  { id: '940GZZTESTV', name: 'Victoria', modes: ['tube'], lines: [{ id: 'victoria', name: 'Victoria' }] },
  { id: '940GZZTESTB', name: 'Bank', modes: ['dlr'], lines: [{ id: 'dlr', name: 'DLR' }] },
];

const JOURNEY_ID = 77;

/** Leg 1: Milton Keynes to Euston, timetabled to arrive at 09:52 BST. */
const TRAIN_LEG = {
  id: 1,
  kind: 'train',
  originCrs: 'MKC',
  originName: 'Milton Keynes Central',
  destinationCrs: 'EUS',
  destinationName: 'London Euston',
  serviceDate: '2026-10-21',
  departAfter: '08:00',
  departBefore: null,
  arriveAfter: null,
  arriveBefore: null,
  windowSearched: true,
  matchMode: 'manual',
  legSkip: null,
  transitMode: null,
  lineName: null,
  plannedDeparture: null,
  plannedArrival: null,
  untrackedStatus: null,
  trackedTrainState: {
    id: 1,
    serviceDate: '2026-10-21',
    pinOriginCrs: 'MKC',
    pinDestinationCrs: 'EUS',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'pending',
    trainUid: 'P12345',
    trainId: null,
    status: null,
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: null,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: 'EUS',
    scheduleDestinationName: 'London Euston',
    scheduleCallingPoints: null,
    journeyStops: [
      stop('MKC', 'Milton Keynes Central', 'Origin', null, '2026-10-21T08:20:00Z'),
      stop('EUS', 'London Euston', 'Terminate', '2026-10-21T08:52:00Z', null),
    ],
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
    trackingPhase: 'planned',
    firstReportExpectedAt: '2026-10-21T07:35:00Z',
  },
};

function stop(crs: string, name: string, kind: string, arrival: string | null, departure: string | null) {
  return {
    crs,
    name,
    tiploc: null,
    kind,
    scheduledArrival: arrival,
    scheduledDeparture: departure,
    publicArrival: arrival,
    publicDeparture: departure,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    stopStatus: 'Scheduled',
    skipSource: null,
    platform: null,
    plannedPlatform: null,
    platformChanged: false,
  };
}

function untrackedLegFrom(body: Record<string, unknown>, id: number) {
  return {
    id,
    kind: 'untracked',
    originCrs: null,
    originName: body.originName ?? null,
    destinationCrs: null,
    destinationName: body.destinationName ?? null,
    originStopId: body.originStopId ?? null,
    destinationStopId: body.destinationStopId ?? null,
    serviceDate: '2026-10-21',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    legSkip: null,
    trackedTrainState: null,
    transitMode: body.transitMode,
    lineName: body.lineName ?? null,
    plannedDeparture: body.plannedDeparture,
    plannedArrival: body.plannedArrival,
    untrackedStatus: 'planned',
  };
}

function json(route: Route, body: unknown) {
  return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
}

/** Answers the page's /api calls. The journey starts with `firstLegs` and
 * gains each untracked leg POSTed to it; resolves with the POST bodies. */
async function mockApi(page: Page, firstLegs: unknown[]): Promise<Record<string, unknown>[]> {
  const bodies: Record<string, unknown>[] = [];
  const legs = [...firstLegs];
  await page.route('**/api/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    if (path === '/api/stations') {
      const q = (url.searchParams.get('q') ?? '').toLowerCase();
      return json(
        route,
        STATIONS.filter((s) => s.code.toLowerCase() === q || s.name.toLowerCase().includes(q)),
      );
    }
    if (/^\/api\/stations\/[A-Z]{3}\//.test(path)) return json(route, []);
    if (path === '/api/tfl/stops') {
      const q = (url.searchParams.get('q') ?? '').toLowerCase();
      const modes = (url.searchParams.get('modes') ?? '').split(',');
      return json(
        route,
        TFL_STOPS.filter((s) => s.name.toLowerCase().includes(q) && s.modes.some((m) => modes.includes(m))),
      );
    }
    if (path === '/api/Journeys' && request.method() === 'POST') {
      const body = request.postDataJSON() as { leg: Record<string, unknown> };
      bodies.push(body.leg);
      if (body.leg.mode === 'untracked') legs.push(untrackedLegFrom(body.leg, 1));
      return json(route, { journeyId: JOURNEY_ID, legId: 1, trackingId: null, resolutionStatus: null });
    }
    if (path === `/api/Journeys/${JOURNEY_ID}` && request.method() === 'GET') {
      return json(route, {
        id: JOURNEY_ID,
        customName: null,
        createdAt: '2026-10-20T00:00:00Z',
        isOwner: true,
        shareLink: null,
        legs,
      });
    }
    if (path === `/api/Journeys/${JOURNEY_ID}/legs` && request.method() === 'POST') {
      const body = request.postDataJSON() as Record<string, unknown>;
      bodies.push(body);
      legs.push(untrackedLegFrom(body, legs.length + 1));
      return json(route, { legId: legs.length, trackingId: null });
    }
    return route.fulfill({ status: 404, body: '' });
  });
  return bodies;
}

test.describe('untracked legs', () => {
  test('adds a Tube leg after a train leg, starting from its arrival', async ({ page }) => {
    const bodies = await mockApi(page, [TRAIN_LEG]);
    await page.goto('/plan?mode=known');
    await page.getByRole('combobox', { name: 'Origin station' }).fill('MKC');
    await page.getByText('Search a time window', { exact: true }).click();
    await page.getByRole('combobox', { name: 'Destination station' }).fill('EUS');
    await page.getByLabel('Earliest departure (optional)').fill('08:00');
    await page.getByRole('button', { name: 'Search for a train' }).click();

    await page.getByRole('button', { name: 'Add a leg' }).click();
    const dialog = page.getByRole('dialog', { name: 'Add a leg to this journey' });
    await dialog.getByText('Tube, bus or walk', { exact: true }).click();

    // Seeded from leg 1: where and when it arrives.
    await expect(dialog.getByRole('combobox', { name: 'From (optional)' })).toHaveValue('London Euston');
    await expect(dialog.getByLabel('Date', { exact: true })).toHaveValue('21 Oct 2026');
    await expect(dialog.getByRole('textbox', { name: 'Leaves', exact: true })).toHaveValue('09:52');
    await expect(dialog.getByText('Your last leg arrives at 09:52.')).toBeVisible();

    await dialog.getByLabel('Line (optional)').fill('Victoria line');
    // Typed, not picked: free text, so no stop id is sent.
    await dialog.getByRole('combobox', { name: 'To (optional)' }).fill('Victoria Coach Station');
    await dialog.getByRole('textbox', { name: 'Leaves', exact: true }).fill('10:05');
    await dialog.getByRole('textbox', { name: 'Arrives', exact: true }).fill('10:20');

    const results = await new AxeBuilder({ page }).include('[role="dialog"]').analyze();
    expect(results.violations).toEqual([]);

    await dialog.getByRole('button', { name: 'Add leg' }).click();
    await expect.poll(() => bodies.length).toBe(2);
    expect(bodies[1]).toEqual({
      mode: 'untracked',
      transitMode: 'tube',
      lineName: 'Victoria line',
      originName: 'London Euston',
      destinationName: 'Victoria Coach Station',
      plannedDeparture: '2026-10-21T09:05:00.000Z',
      plannedArrival: '2026-10-21T09:20:00.000Z',
    });
    await expect(page.getByText('Leg 2: Tube: Victoria line, London Euston to Victoria Coach Station')).toBeVisible();
    await expect(page.getByText('Not live tracked')).toBeVisible();
  });

  test('starts a journey with Tube stops picked from the TfL lookup', async ({ page }) => {
    const bodies = await mockApi(page, []);
    await page.goto('/plan');
    await page.getByText('Tube, bus or walk', { exact: true }).click();
    const from = page.getByRole('combobox', { name: 'From (optional)' });
    await from.fill('eus');
    await page.getByRole('option', { name: 'Euston · Tube: Northern' }).click();
    await expect(from).toHaveValue('Euston');
    const to = page.getByRole('combobox', { name: 'To (optional)' });
    await to.fill('vic');
    await page.getByRole('option', { name: 'Victoria · Tube: Victoria' }).click();
    await expect(to).toHaveValue('Victoria');
    await page.getByRole('textbox', { name: 'Leaves', exact: true }).fill('08:30');
    await page.getByRole('textbox', { name: 'Arrives', exact: true }).fill('08:45');

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);

    await page.getByRole('button', { name: 'Start journey' }).click();
    await expect.poll(() => bodies.length).toBe(1);
    expect(bodies[0]).toMatchObject({
      mode: 'untracked',
      transitMode: 'tube',
      originName: 'Euston',
      originStopId: '940GZZTESTE',
      destinationName: 'Victoria',
      destinationStopId: '940GZZTESTV',
    });
    await expect(page.getByText('Leg 1: Tube, Euston to Victoria')).toBeVisible();
  });

  test('starts a journey with a walk', async ({ page }) => {
    const bodies = await mockApi(page, []);
    await page.goto('/plan');
    await page.getByText('Tube, bus or walk', { exact: true }).click();
    await page.getByRole('combobox', { name: 'How' }).click();
    await page.getByRole('option', { name: 'Walk' }).click();
    await page.getByLabel('To (optional)').fill('London Euston');
    await page.getByRole('textbox', { name: 'Leaves', exact: true }).fill('08:30');
    await page.getByRole('textbox', { name: 'Arrives', exact: true }).fill('08:45');
    await page.getByRole('button', { name: 'Start journey' }).click();

    await expect.poll(() => bodies.length).toBe(1);
    expect(bodies[0]).toMatchObject({ mode: 'untracked', transitMode: 'walk', destinationName: 'London Euston' });
    await expect(page.getByText('Leg 1: Walk, To London Euston')).toBeVisible();
    // A next leg can follow it.
    await expect(page.getByRole('button', { name: 'Add a leg' })).toBeVisible();
  });
});
