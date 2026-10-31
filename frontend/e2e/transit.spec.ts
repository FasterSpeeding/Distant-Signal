import { test, expect, type Page } from '@playwright/test';

// The transit pages against the live api and the seed's metrolink and tfl
// networks (frontend/e2e/fixtures/seed.sql; API_TRANSIT_NETWORKS in the
// frontend-e2e job). A smoke test: each page renders its data, and the
// board says it is scheduled, not live.

/** The network page's A to Z of stops. */
function stopList(page: Page) {
  return page.getByRole('list', { name: 'Stops' });
}

test.describe('transit pages', () => {
  test('the network page lists its lines and stops', async ({ page }) => {
    await page.goto('/transit/metrolink');
    await expect(page.getByRole('heading', { level: 1, name: 'Manchester Metrolink' })).toBeVisible();
    await expect(page.getByRole('link', { name: /^Blue Line/ })).toBeVisible();
    await expect(stopList(page).getByRole('link', { name: 'Piccadilly' })).toBeVisible();
  });

  test('a stop shows its scheduled departures, a replacement bus labelled', async ({ page }) => {
    await page.goto('/transit/metrolink/stops/9400ZZMAPIC?date=2026-09-17');
    await expect(page.getByRole('heading', { level: 1, name: 'Piccadilly' })).toBeVisible();
    await expect(page.getByText('Scheduled times, not live')).toBeVisible();
    const board = page.getByRole('list', { name: 'Scheduled departures' });
    await expect(board.getByRole('listitem').first()).toContainText('07:02');
    await expect(board).toContainText('Replacement bus');
  });

  test('the network shows its line statuses and disruptions', async ({ page }) => {
    await page.goto('/transit/metrolink');
    // Each line is a clickable card: its link is named by the line, the
    // status is elsewhere in the card.
    const card = (name: string) =>
      page.locator('[data-clickable-card]').filter({ has: page.getByRole('link', { name, exact: true }) });
    await expect(card('Blue Line')).toContainText('Minor delays');
    await expect(card('Green Line')).toContainText('No reported disruption');
    // A summary: each disrupted line's headline, the other notices and
    // what is coming up, each linking its own page.
    const byLine = page.getByRole('list', { name: 'Disruptions by line' });
    await expect(byLine.getByRole('link', { name: 'Delays between Piccadilly and Eccles' })).toHaveAttribute(
      'href',
      '/transit/metrolink/incidents/E2E-S1',
    );
    await expect(page.getByRole('list', { name: 'Other notices' })).toContainText('Lift out of service at Piccadilly');
    await expect(page.getByRole('list', { name: 'Disruptions coming up' })).toContainText(
      'No trams to Deansgate-Castlefield',
    );
    // The detail is on those pages, not here.
    await expect(page.getByText('A points fault at Cornbrook is causing delays.')).toHaveCount(0);
  });

  test('a disruption has its own page, with a preview card', async ({ page, request }) => {
    await page.goto('/transit/metrolink');
    await page
      .getByRole('list', { name: 'Disruptions by line' })
      .getByRole('link', { name: 'Delays between Piccadilly and Eccles' })
      .click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/incidents\/E2E-S1$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Delays between Piccadilly and Eccles' })).toBeVisible();
    await expect(page.getByText('A points fault at Cornbrook is causing delays.')).toBeVisible();
    await expect(page.getByText('Allow extra time.')).toBeVisible();
    await expect(
      page.getByRole('list', { name: 'Lines affected' }).getByRole('link', { name: 'Blue Line' }),
    ).toHaveAttribute('href', '/transit/metrolink/lines/blue');
    const image = await page.locator('meta[property="og:image"]').getAttribute('content');
    expect(image).toMatch(/\/transit\/metrolink\/incidents\/E2E-S1\/card\.png\?v=/);
    const card = await request.get('/transit/metrolink/incidents/E2E-S1/card.png');
    expect(card.status()).toBe(200);
    expect(card.headers()['content-type']).toBe('image/png');

    // The lift notice's page links its stop.
    await page.goto('/transit/metrolink/incidents/E2E-S2');
    await page.getByRole('link', { name: 'Piccadilly', exact: true }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAPIC$/);

    const missing = await page.goto('/transit/metrolink/incidents/NOPE');
    expect(missing?.status()).toBe(404);
  });

  test('a stop shows its notices, and a line its status', async ({ page }) => {
    await page.goto('/transit/metrolink/stops/9400ZZMAPIC?date=2026-09-17');
    await expect(page.getByRole('link', { name: 'MAN', exact: true })).toHaveAttribute('href', '/stations/MAN');
    await expect(
      page.getByRole('list', { name: 'Notices' }).getByRole('link', { name: 'Lift out of service at Piccadilly' }),
    ).toHaveAttribute('href', '/transit/metrolink/incidents/E2E-S2');
    await page.goto('/transit/metrolink/lines/blue');
    await expect(page.getByText('Minor delays')).toBeVisible();
    await expect(page.getByRole('list', { name: 'Disruptions now' })).toContainText('Allow extra time.');
  });

  test('the line timetable shows a grid', async ({ page }) => {
    await page.goto('/transit/metrolink/lines/blue/timetable?date=2026-09-17&direction=outbound&from=07:00');
    await expect(page.getByRole('heading', { level: 1, name: 'Blue Line timetable' })).toBeVisible();
    await expect(page.getByRole('rowheader', { name: 'Eccles' })).toBeVisible();
  });

  test('clicking a stop on the network page opens its stop page', async ({ page }) => {
    const stopHeading = page.getByRole('heading', { level: 1, name: 'Piccadilly' });

    // From the stop list.
    await page.goto('/transit/metrolink');
    await stopList(page).getByRole('link', { name: 'Piccadilly' }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAPIC$/);
    await expect(stopHeading).toBeVisible();

    // And by keyboard: Enter on the focused link.
    await page.goto('/transit/metrolink');
    await stopList(page).getByRole('link', { name: 'Piccadilly' }).focus();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAPIC$/);
    await expect(stopHeading).toBeVisible();
  });

  test('a line page and its timetable link their stops', async ({ page }) => {
    await page.goto('/transit/metrolink/lines/blue');
    await page.getByRole('link', { name: 'Eccles', exact: true }).first().click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAECC$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Eccles' })).toBeVisible();

    await page.goto('/transit/metrolink/lines/blue/timetable?date=2026-09-17&direction=outbound&from=07:00');
    await page.getByRole('rowheader', { name: 'Eccles' }).getByRole('link', { name: 'Eccles' }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAECC$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Eccles' })).toBeVisible();
  });

  test('the TfL network groups its lines by mode and links the Overground and Elizabeth line', async ({ page }) => {
    await page.goto('/transit');
    await expect(page.getByRole('heading', { level: 1, name: 'Trams and metros' })).toBeVisible();
    await page.getByRole('link', { name: 'London (TfL)' }).click();
    await expect(page).toHaveURL(/\/transit\/tfl$/);
    await expect(page.getByRole('heading', { level: 2, name: 'Underground' })).toBeVisible();
    await expect(page.getByRole('heading', { level: 2, name: 'DLR' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Elizabeth line' })).toHaveAttribute('href', '/lines/elizabeth-line');
    // TfL's own status, so no disruptions list.
    await expect(page.getByRole('heading', { name: 'Disruptions' })).toHaveCount(0);
  });

  test('a TfL line says how often it runs and links its status page', async ({ page }) => {
    // Dated: the seed's trains run on 2026-09-17 and the day it ran, so
    // "today" alone would fail a run that crosses midnight.
    await page.goto('/transit/tfl/lines/victoria?date=2026-09-17');
    await expect(page.getByRole('heading', { level: 1, name: 'Victoria' })).toBeVisible();
    await expect(page.getByRole('heading', { name: /^How often it runs on / })).toBeVisible();
    await expect(page.getByText('From Brixton: first 07:00, last 07:10')).toBeVisible();
    await expect(page.getByText('Every 3 to 4 minutes')).toBeVisible();
    await expect(page.getByRole('link', { name: 'Status details' })).toHaveAttribute('href', '/lines/tfl-victoria');
  });

  test('the DLR is a line of the TfL network, with its own frequency', async ({ page }) => {
    await page.goto('/transit/tfl');
    await page.getByRole('link', { name: 'DLR', exact: true }).click();
    await expect(page).toHaveURL(/\/transit\/tfl\/lines\/dlr$/);
    await page.goto('/transit/tfl/lines/dlr?date=2026-09-17');
    await expect(page.getByRole('heading', { level: 1, name: 'DLR' })).toBeVisible();
    await expect(page.getByText('From Bank: first 07:05, last 07:15')).toBeVisible();
    await expect(page.getByText('Every 10 minutes')).toBeVisible();
    await expect(page.getByRole('link', { name: 'Status details' })).toHaveAttribute('href', '/lines/tfl-dlr');
  });

  test("a TfL network stop is TfL's own stop page", async ({ page }) => {
    await page.goto('/transit/tfl/stops/9400ZZLUKSX');
    await expect(page).toHaveURL(/\/tfl\/stops\/940GZZLUKSX$/);
    await expect(page.getByRole('heading', { level: 1, name: "King's Cross St. Pancras" })).toBeVisible();
    // And the line page links it there directly.
    await page.goto('/transit/tfl/lines/victoria');
    await expect(page.getByRole('link', { name: "King's Cross St. Pancras" })).toHaveAttribute(
      'href',
      '/tfl/stops/940GZZLUKSX',
    );
  });

  test('an unknown network is not found', async ({ page }) => {
    const response = await page.goto('/transit/nowhere');
    expect(response?.status()).toBe(404);
  });
});
