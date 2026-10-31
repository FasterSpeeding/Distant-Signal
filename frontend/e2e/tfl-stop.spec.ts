import { test, expect } from '@playwright/test';

// A TfL stop's page (DS-136) against the live api, the seed's two TfL stops
// (frontend/e2e/fixtures/seed.sql) and scripts/tfl-arrivals-stub.py, which
// the frontend-e2e job runs as the api's TFL_BASE_URL: King's Cross St.
// Pancras gets a fixed board, Russell Square a 503.

test.describe('TfL stop page', () => {
  test('shows live arrivals by platform, its lines and its National Rail station', async ({ page }) => {
    await page.goto('/tfl/stops/940GZZLUKSX');
    await expect(page.getByRole('heading', { level: 1, name: "King's Cross St. Pancras" })).toBeVisible();
    await expect(page.getByText('Underground', { exact: true })).toBeVisible();

    const south = page.getByRole('list', { name: 'Victoria, Southbound - Platform 5' });
    await expect(south.getByRole('listitem')).toHaveCount(2);
    await expect(south.getByRole('listitem').first()).toContainText('Brixton');
    await expect(south.getByRole('listitem').first()).toContainText('Due');
    const north = page.getByRole('list', { name: 'Victoria, Northbound - Platform 6' });
    await expect(north.getByRole('listitem').first()).toContainText('Walthamstow Central');
    await expect(north.getByRole('listitem').first()).toContainText('2 min');
    // No destination: TfL's "towards".
    await expect(north.getByRole('listitem').nth(1)).toContainText('Seven Sisters');
    await expect(page.getByText(/^Updated/)).toBeVisible();

    await expect(page.getByRole('link', { name: 'London Kings Cross (KGX)' })).toHaveAttribute('href', '/stations/KGX');
    await expect(page.getByRole('link', { name: 'Victoria', exact: true })).toHaveAttribute(
      'href',
      '/lines/tfl-victoria',
    );
    // TfL's credit: the site footer's, not a second copy on the page.
    await expect(page.getByRole('main').getByText(/Powered by TfL Open Data/)).toHaveCount(0);
    await expect(page.getByRole('contentinfo').getByText(/Powered by TfL Open Data/)).toBeVisible();
  });

  test("shows the stop's timetable from the TfL network (the frontend-e2e job serves tfl)", async ({ page }) => {
    await page.goto('/tfl/stops/940GZZLUKSX?date=2026-09-17');
    await expect(page.getByRole('heading', { level: 2, name: 'Timetable' })).toBeVisible();
    await expect(page.getByText('Victoria towards Walthamstow Central')).toBeVisible();
    await expect(page.getByText('First 07:15, last 07:25')).toBeVisible();
    const board = page.getByRole('list', { name: 'Scheduled departures' });
    await expect(board.getByRole('listitem').first()).toContainText('07:15');
    await expect(board.getByRole('listitem').first()).toContainText('Walthamstow Central');
  });

  test('keeps the page when TfL is unavailable', async ({ page }) => {
    await page.goto('/tfl/stops/940GZZLURSQ');
    await expect(page.getByRole('heading', { level: 1, name: 'Russell Square' })).toBeVisible();
    await expect(page.getByText("Couldn't load live arrivals")).toBeVisible();
    await expect(page.getByRole('link', { name: 'Piccadilly', exact: true })).toBeVisible();
  });

  test('an unknown stop is not found', async ({ page }) => {
    // The page streams behind its loading skeleton, so the status is
    // already sent; the not-found page replaces it.
    await page.goto('/tfl/stops/940GZZLUNOPE');
    await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
  });

  test('a station search result for a TfL stop opens its page', async ({ page }) => {
    await page.goto('/stations');
    const field = page.getByRole('combobox', { name: 'Station name or code' });
    await field.fill('russell');
    await page.getByRole('option', { name: /^Russell Square · Tube/ }).click();
    await page.getByRole('button', { name: 'Look up' }).click();
    await expect(page).toHaveURL(/\/tfl\/stops\/940GZZLURSQ$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Russell Square' })).toBeVisible();
  });

  test('"Add as a journey leg" opens the Tube form at the stop', async ({ page }) => {
    await page.goto('/tfl/stops/940GZZLUKSX');
    await page.getByRole('link', { name: 'Add as a journey leg' }).click();
    await expect(page).toHaveURL(/\/plan\?fromTflStop=940GZZLUKSX$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Plan a journey' })).toBeVisible();
    await expect(page.getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
    await expect(page.getByRole('combobox', { name: 'From (optional)' })).toHaveValue("King's Cross St. Pancras");
  });
});
