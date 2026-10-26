import { test, expect, type Page, type Route } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// "Add a leg" on /journeys/new (components/AddJourneyLegButton.tsx): its
// From and To fields are the site's standard station search, so a station
// can be found by its full name or by its code, and the leg is always sent
// with CRS codes.
//
// Every /api call the page makes from the browser is answered here, so the
// spec needs no seeded journey and no session: the station search gets a
// small fixture (matched on code or name, as the real api does), and the
// journey calls stand in for `POST /Journeys`, `GET /Journeys/{id}` and
// `POST /Journeys/{id}/legs`.

const STATIONS = [
  { code: 'EUS', name: 'London Euston' },
  { code: 'KGX', name: 'London Kings Cross' },
  { code: 'MKC', name: 'Milton Keynes Central' },
];

const JOURNEY_ID = 99;

/** Leg 1, matched to a train, so the page offers "Add a leg" with its
 * destination (EUS) as the next leg's origin. */
const MATCHED_LEG = {
  id: 1,
  originCrs: 'MKC',
  originName: 'Milton Keynes Central',
  destinationCrs: 'EUS',
  destinationName: 'London Euston',
  serviceDate: '2026-09-22',
  departAfter: '08:00',
  departBefore: null,
  arriveAfter: null,
  arriveBefore: null,
  windowSearched: true,
  matchMode: 'manual',
  legSkip: null,
  trackedTrainState: {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'MKC',
    pinDestinationCrs: 'EUS',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'P12345',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 0,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: 'EUS',
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
  },
};

function json(route: Route, body: unknown) {
  return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
}

function stop(crs: string, kind: string, times: { publicArrival?: string; publicDeparture?: string }) {
  return {
    crs,
    name: null,
    tiploc: null,
    kind,
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    stopStatus: 'Unknown',
    skipSource: null,
    platform: null,
    plannedPlatform: null,
    platformChanged: false,
    ...times,
  };
}

/** Leg 1 with its train's stops, timetabled into EUS at 23:12 UTC on
 * 22 September: 00:12 on 23 September in London (BST). */
const MATCHED_LEG_WITH_ARRIVAL = {
  ...MATCHED_LEG,
  trackedTrainState: {
    ...MATCHED_LEG.trackedTrainState,
    journeyStops: [
      stop('MKC', 'Origin', { publicDeparture: '2026-09-22T22:30:00Z' }),
      stop('EUS', 'Terminate', { publicArrival: '2026-09-22T23:12:00Z' }),
    ],
  },
};

/** Answers the page's /api calls; resolves with each add-leg POST body. */
async function mockApi(page: Page, firstLeg: object = MATCHED_LEG): Promise<unknown[]> {
  const legBodies: unknown[] = [];
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
    if (path === '/api/tfl/stops/rail-crs') {
      return json(route, url.searchParams.get('ids') === '940GZZLUEUS' ? { '940GZZLUEUS': ['EUS'] } : {});
    }
    if (path === '/api/Journeys' && request.method() === 'POST') {
      return json(route, { journeyId: JOURNEY_ID, legId: 1, trackingId: 42, resolutionStatus: 'pending' });
    }
    if (path === `/api/Journeys/${JOURNEY_ID}` && request.method() === 'GET') {
      return json(route, {
        id: JOURNEY_ID,
        customName: null,
        createdAt: '2026-09-22T00:00:00Z',
        isOwner: true,
        legs: [firstLeg],
      });
    }
    if (path === `/api/Journeys/${JOURNEY_ID}/legs` && request.method() === 'POST') {
      legBodies.push(request.postDataJSON());
      return json(route, { legId: 2, trackingId: null });
    }
    return route.fulfill({ status: 404, body: '' });
  });
  return legBodies;
}

/** Tracks leg 1 (a time-window search) and opens the "Add a leg" modal,
 * as the API left it. */
async function openPrefilledAddLeg(page: Page) {
  await page.goto('/journeys/new?mode=known');
  await page.getByRole('combobox', { name: 'Origin station' }).fill('MKC');
  await page.getByText('Search a time window', { exact: true }).click();
  await page.getByRole('combobox', { name: 'Destination station' }).fill('EUS');
  await page.getByLabel('Earliest departure (optional)').fill('08:00');
  await page.getByRole('button', { name: 'Search for a train' }).click();

  await page.getByRole('button', { name: 'Add a leg' }).click();
  const dialog = page.getByRole('dialog', { name: 'Add a leg to this journey' });
  // Pre-filled from leg 1's destination, shown by name.
  await expect(dialog.getByRole('combobox', { name: 'From station' })).toHaveValue('London Euston (EUS)');
  return dialog;
}

/** The same, with an earliest departure entered. */
async function openAddLeg(page: Page) {
  const dialog = await openPrefilledAddLeg(page);
  await dialog.getByLabel('Earliest departure (optional)').fill('09:00');
  return dialog;
}

test.describe('add a leg', () => {
  test("starts from the previous leg's arrival, the next day after midnight", async ({ page }) => {
    const legBodies = await mockApi(page, MATCHED_LEG_WITH_ARRIVAL);
    const dialog = await openPrefilledAddLeg(page);

    await expect(dialog.getByLabel('Service date')).toHaveValue('23 Sep 2026');
    const departFrom = dialog.getByLabel('Earliest departure (optional)');
    await expect(departFrom).toHaveValue('00:12');
    await expect(dialog.getByText(/Your last leg arrives at 00:12\./)).toBeVisible();

    const to = dialog.getByRole('combobox', { name: 'To station' });
    await to.fill('kgx');
    await to.press('Tab');
    await dialog.getByRole('button', { name: 'Add leg' }).click();
    await expect.poll(() => legBodies.length).toBe(1);
    expect(legBodies[0]).toMatchObject({
      originCrs: 'EUS',
      destinationCrs: 'KGX',
      serviceDate: '2026-09-23',
      departWindow: { after: '00:12', before: null },
    });
  });

  test("after a Tube leg, starts at the Tube stop's station and the leg's arrival", async ({ page }) => {
    // Vauxhall to Euston on the Victoria line, picked from TfL's stops, as
    // the API serves an untracked leg: no station code, a stop id.
    const legBodies = await mockApi(page, {
      ...MATCHED_LEG,
      kind: 'untracked',
      originCrs: null,
      originName: 'Vauxhall',
      destinationCrs: null,
      destinationName: 'Euston',
      transitMode: 'tube',
      lineName: 'Victoria line',
      originStopId: '940GZZLUVXL',
      destinationStopId: '940GZZLUEUS',
      plannedDeparture: '2026-09-22T08:00:00Z',
      plannedArrival: '2026-09-22T08:16:00Z',
      untrackedStatus: 'planned',
      departAfter: null,
      windowSearched: false,
      trackedTrainState: null,
    });
    await page.goto('/journeys/new?mode=known');
    await page.getByRole('combobox', { name: 'Origin station' }).fill('MKC');
    await page.getByText('Search a time window', { exact: true }).click();
    await page.getByRole('combobox', { name: 'Destination station' }).fill('EUS');
    await page.getByLabel('Earliest departure (optional)').fill('08:00');
    await page.getByRole('button', { name: 'Search for a train' }).click();

    await page.getByRole('button', { name: 'Add a leg' }).click();
    const dialog = page.getByRole('dialog', { name: 'Add a leg to this journey' });
    await expect(dialog.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
    await expect(dialog.getByRole('combobox', { name: 'From station' })).toHaveValue('London Euston (EUS)');
    await expect(dialog.getByLabel('Service date')).toHaveValue('22 Sep 2026');
    await expect(dialog.getByLabel('Earliest departure (optional)')).toHaveValue('09:16');

    const to = dialog.getByRole('combobox', { name: 'To station' });
    await to.fill('mkc');
    await to.press('Tab');
    await dialog.getByRole('button', { name: 'Add leg' }).click();
    await expect.poll(() => legBodies.length).toBe(1);
    expect(legBodies[0]).toMatchObject({ mode: 'window', originCrs: 'EUS', destinationCrs: 'MKC' });
  });

  test('after a walk to a place that is no station, opens on "Tube, bus or walk" from it', async ({ page }) => {
    await mockApi(page, {
      ...MATCHED_LEG,
      kind: 'untracked',
      originCrs: null,
      originName: 'Euston',
      destinationCrs: null,
      destinationName: 'Home',
      transitMode: 'walk',
      plannedDeparture: '2026-09-22T08:00:00Z',
      plannedArrival: '2026-09-22T08:16:00Z',
      untrackedStatus: 'planned',
      departAfter: null,
      windowSearched: false,
      trackedTrainState: null,
    });
    await page.goto('/journeys/new?mode=known');
    await page.getByRole('combobox', { name: 'Origin station' }).fill('MKC');
    await page.getByText('Search a time window', { exact: true }).click();
    await page.getByRole('combobox', { name: 'Destination station' }).fill('EUS');
    await page.getByLabel('Earliest departure (optional)').fill('08:00');
    await page.getByRole('button', { name: 'Search for a train' }).click();

    await page.getByRole('button', { name: 'Add a leg' }).click();
    const dialog = page.getByRole('dialog', { name: 'Add a leg to this journey' });
    await expect(dialog.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
    await expect(dialog.getByRole('combobox', { name: 'From (optional)' })).toHaveValue('Home');
    await expect(dialog.getByRole('textbox', { name: 'Leaves' })).toHaveValue('09:16');
    await expect(dialog.getByRole('combobox', { name: 'From station' })).toHaveCount(0);
  });

  test('leaves the time empty when the previous arrival is not known', async ({ page }) => {
    await mockApi(page);
    const dialog = await openPrefilledAddLeg(page);

    await expect(dialog.getByLabel('Earliest departure (optional)')).toHaveValue('');
    await expect(dialog.getByText(/Your last leg arrives/)).toHaveCount(0);
  });

  test('finds the station by its full name', async ({ page }) => {
    const legBodies = await mockApi(page);
    const dialog = await openAddLeg(page);

    const to = dialog.getByRole('combobox', { name: 'To station' });
    await to.fill('London Kings Cross');
    await expect(page.getByRole('option', { name: 'KGX · London Kings Cross' })).toBeVisible();
    // Leaving the field picks the station the typed name matches exactly.
    await to.press('Tab');
    await expect(to).toHaveValue('London Kings Cross (KGX)');
    await expect(dialog.getByText('Must be a 3-letter station code')).toHaveCount(0);
    await expect(dialog.getByText('Only trains reaching KGX at or after this time.')).toBeVisible();

    await dialog.getByRole('button', { name: 'Add leg' }).click();
    await expect.poll(() => legBodies.length).toBe(1);
    expect(legBodies[0]).toMatchObject({ mode: 'window', originCrs: 'EUS', destinationCrs: 'KGX' });
  });

  test('picks a suggestion from part of a name with the keyboard', async ({ page }) => {
    const legBodies = await mockApi(page);
    const dialog = await openAddLeg(page);

    const to = dialog.getByRole('combobox', { name: 'To station' });
    await to.fill('kings');
    await expect(page.getByRole('option', { name: 'KGX · London Kings Cross' })).toBeVisible();
    await to.press('ArrowDown');
    await to.press('Enter');
    await expect(to).toHaveValue('London Kings Cross (KGX)');

    await dialog.getByRole('button', { name: 'Add leg' }).click();
    await expect.poll(() => legBodies.length).toBe(1);
    expect(legBodies[0]).toMatchObject({ originCrs: 'EUS', destinationCrs: 'KGX' });
  });

  test('finds the station by its code', async ({ page }) => {
    const legBodies = await mockApi(page);
    const dialog = await openAddLeg(page);

    const to = dialog.getByRole('combobox', { name: 'To station' });
    await to.fill('kgx');
    await to.press('Tab');
    await expect(to).toHaveValue('London Kings Cross (KGX)');
    await expect(dialog.getByRole('button', { name: 'Clear To station' })).toBeVisible();
    await expect(dialog.getByText('Must be a 3-letter station code')).toHaveCount(0);
    // With a station picked: its clear button and live region included.
    const results = await new AxeBuilder({ page }).include('[role="dialog"]').analyze();
    expect(results.violations).toEqual([]);

    await dialog.getByRole('button', { name: 'Add leg' }).click();
    await expect.poll(() => legBodies.length).toBe(1);
    expect(legBodies[0]).toMatchObject({ originCrs: 'EUS', destinationCrs: 'KGX' });
  });

  test('says when a typed value is not a station', async ({ page }) => {
    const legBodies = await mockApi(page);
    const dialog = await openAddLeg(page);

    const to = dialog.getByRole('combobox', { name: 'To station' });
    await to.fill('Nowhere at all');
    await to.press('Tab');
    await expect(dialog.getByText('Must be a 3-letter station code')).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Add leg' })).toBeDisabled();
    expect(legBodies).toHaveLength(0);

    // The error is tied to the field.
    await expect(to).toHaveAttribute('aria-invalid', 'true');

    // The field error is legible: Mantine's error red is the site's
    // error-text token (app/globals.css), not its 3.28:1 default.
    const results = await new AxeBuilder({ page }).include('[role="dialog"]').analyze();
    expect(results.violations).toEqual([]);
  });
});
