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

/** Answers the page's /api calls; resolves with each add-leg POST body. */
async function mockApi(page: Page): Promise<unknown[]> {
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
    if (path === '/api/Journeys' && request.method() === 'POST') {
      return json(route, { journeyId: JOURNEY_ID, legId: 1, trackingId: 42, resolutionStatus: 'pending' });
    }
    if (path === `/api/Journeys/${JOURNEY_ID}` && request.method() === 'GET') {
      return json(route, {
        id: JOURNEY_ID,
        customName: null,
        createdAt: '2026-09-22T00:00:00Z',
        isOwner: true,
        legs: [MATCHED_LEG],
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

/** Tracks leg 1 (a time-window search) and opens the "Add a leg" modal. */
async function openAddLeg(page: Page) {
  await page.goto('/journeys/new');
  await page.getByRole('combobox', { name: 'Origin station' }).fill('MKC');
  await page.getByText('Search a time window', { exact: true }).click();
  await page.getByRole('combobox', { name: 'Destination station' }).fill('EUS');
  await page.getByLabel('Earliest departure (optional)').fill('08:00');
  await page.getByRole('button', { name: 'Search for a train' }).click();

  await page.getByRole('button', { name: 'Add a leg' }).click();
  const dialog = page.getByRole('dialog', { name: 'Add a leg to this journey' });
  // Pre-filled from leg 1's destination.
  await expect(dialog.getByRole('combobox', { name: 'From station' })).toHaveValue('EUS');
  await dialog.getByLabel('Earliest departure (optional)').fill('09:00');
  return dialog;
}

test.describe('add a leg', () => {
  test('finds the station by its full name', async ({ page }) => {
    const legBodies = await mockApi(page);
    const dialog = await openAddLeg(page);

    const to = dialog.getByRole('combobox', { name: 'To station' });
    await to.fill('London Kings Cross');
    await expect(page.getByRole('option', { name: 'KGX · London Kings Cross' })).toBeVisible();
    // Left as the typed name, without picking the suggestion.
    await to.press('Tab');
    await expect(to).toHaveValue('London Kings Cross');
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
    await expect(to).toHaveValue('KGX');

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
    await expect(dialog.getByText('Must be a 3-letter station code')).toHaveCount(0);
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
