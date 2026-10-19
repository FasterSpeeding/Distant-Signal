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
// whole file makes six plan requests.

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

test('plans by arrival time, then pages later and earlier', async ({ page }) => {
  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-17&arriveBy=09:00');
  await expect(page.getByRole('radio', { name: 'Arrive by' })).toBeChecked();
  await expect(page.getByLabel('Time', { exact: true })).toHaveValue('09:00');

  const response = await findRoutes(page);
  const sent = new URL(response.url()).searchParams;
  expect(sent.get('arriveBy')).toBe('09:00:00');
  expect(sent.has('departAfter')).toBe(false);

  // The latest train arriving by 09:00: the 08:00, arriving 08:50.
  const summary = page.getByTestId('plan-timing-summary');
  await expect(summary).toHaveText('Arrive by 09:00, 17 Sep 2026');
  await expect(page.getByText(/^08:00 .*08:50$/)).toBeVisible();
  await expect(page).toHaveURL(/arriveBy=09%3A00/);

  // Later: depart after 08:01, so the 08:30.
  await page.getByRole('button', { name: 'Later routes' }).click();
  await expect(summary).toHaveText('Depart after 08:01, 17 Sep 2026');
  await expect(page.getByText(/^08:30 .*09:20$/)).toBeVisible();
  await expect(page.getByRole('radio', { name: 'Depart after' })).toBeChecked();
  await expect(page).toHaveURL(/departAfter=08%3A01/);
  await expect(page).not.toHaveURL(/arriveBy/);

  // Earlier from there: arrive by 09:19, so the 08:00 again.
  await page.getByRole('button', { name: 'Earlier routes' }).click();
  await expect(summary).toHaveText('Arrive by 09:19, 17 Sep 2026');
  await expect(page.getByText(/^08:00 .*08:50$/)).toBeVisible();
  await expect(page.getByRole('radio', { name: 'Arrive by' })).toBeChecked();
});

test('the toggle switches a search from departure to arrival time', async ({ page }) => {
  await openPlan(page, 'origin=ZEA&destination=ZEC&date=2026-09-17&departAfter=07:00');
  await expect(page.getByRole('radio', { name: 'Depart after' })).toBeChecked();

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
