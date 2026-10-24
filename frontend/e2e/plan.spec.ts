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
// whole file makes nine plan requests.

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

// Rail, tram, rail (transit trip planning §11): the seed's "A rail, tram,
// rail route for /plan": ZTA 07:00 to MAN 07:30 (E2ETR1), the Blue Line
// from Piccadilly (9400ZZMAPIC) 07:44 to Victoria (9400ZZMAVIC) 07:52,
// and MCV 08:10 to ZTB 08:40 (E2ETR2), with walks between MAN and
// Piccadilly and between Victoria and MCV (`transit_rail_links`). The api
// plans it for real; saving is answered here, so no journey is left behind
// for the other specs and screenshots.
test('plans a route with a tram in the middle, and saves the tram as an untracked leg', async ({ page }) => {
  let saved: { legs: Record<string, unknown>[] } | null = null;
  await page.route('**/api/Journeys/itinerary', async (route) => {
    saved = route.request().postDataJSON() as { legs: Record<string, unknown>[] };
    await route.fulfill({ json: { journeyId: 990001, legId: 1, trackingId: null, resolutionStatus: null } });
  });
  await page.route('**/api/Journeys/990001', (route) => route.fulfill({ status: 404, body: 'Not found' }));

  await openPlan(page, 'origin=ZTA&destination=ZTB&date=2026-09-17&departAfter=06:45');
  const response = await findRoutes(page);
  expect(new URL(response.url()).searchParams.get('transit')).toBe('true');

  const card = page.locator('[data-plan-entry]').first();
  await expect(card.getByText('07:44 Tram (Blue Line) Piccadilly to Victoria 07:52')).toBeVisible();
  await expect(card.getByText('Tram', { exact: true })).toBeVisible();
  await expect(card.getByText(/^Walk to Piccadilly tram stop, \d+ min$/)).toBeVisible();
  await expect(card.getByText(/^Walk from Victoria tram stop, \d+ min$/)).toBeVisible();
  await expect(card.getByText(/^About \d+ min walk, estimated$/).first()).toBeVisible();
  await expect(card.getByText('Scheduled times, not live')).toBeVisible();
  await expect(card).not.toContainText(/on time|expected|late/i);

  await card.getByRole('radio').check();
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
  expect(legs[1]).toMatchObject({ kind: 'untracked', destinationStopId: '9400ZZMAPIC' });
  // The tram's own timetabled times (07:44 to 07:52 BST), its line and stations.
  expect(legs[2]).toEqual({
    kind: 'untracked',
    transitMode: 'tram',
    originName: 'Piccadilly',
    destinationName: 'Victoria',
    lineName: 'Blue Line',
    originStopId: '9400ZZMAPIC',
    destinationStopId: '9400ZZMAVIC',
    plannedDeparture: '2026-09-17T06:44:00.000Z',
    plannedArrival: '2026-09-17T06:52:00.000Z',
  });
  expect(legs[3]).toMatchObject({ kind: 'untracked', originStopId: '9400ZZMAVIC' });
  expect(legs[4]).toMatchObject({ trainUid: 'E2ETR2', originCrs: 'MCV', destinationCrs: 'ZTB' });
});
