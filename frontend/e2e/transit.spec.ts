import { test, expect, type Page } from '@playwright/test';

// The transit pages against the live api and the seed's metrolink and tfl
// networks (frontend/e2e/fixtures/seed.sql; API_TRANSIT_NETWORKS in the
// frontend-e2e job). A smoke test: each page renders its data, and the
// board says it is scheduled, not live.

/** The A to Z of stops (`/transit/[network]/stops`). */
function stopList(page: Page) {
  return page.getByRole('list', { name: 'Stops' });
}

/** The network page's "Find a stop" type-ahead. */
function stopFinder(page: Page) {
  return page.getByRole('combobox', { name: 'Find a stop' });
}

/** Any request for a network's full stop list. */
const STOPS_REQUEST = /\/api\/transit\/[a-z0-9-]+\/stops$/;

/** Ceilings for the network page's weight (DS-229): it used to render every
 * stop (about 215 KB of HTML for Metrolink's 99, 345 KB for TfL's 356 on
 * prod). The seed has a handful of stops, so these catch a list of them
 * coming back with room to spare, not a few bytes' drift. */
const NETWORK_PAGE_MAX_HTML_BYTES = 120_000;
const NETWORK_PAGE_MAX_ELEMENTS = 700;

test.describe('transit pages', () => {
  test('the network page lists its lines, and finds a stop without listing them all', async ({ page }) => {
    const stopRequests: string[] = [];
    page.on('request', (request) => {
      if (STOPS_REQUEST.test(new URL(request.url()).pathname)) stopRequests.push(request.url());
    });
    await page.goto('/transit/metrolink');
    await expect(page.getByRole('heading', { level: 1, name: 'Manchester Metrolink' })).toBeVisible();
    await expect(page.getByRole('link', { name: /^Blue Line/ })).toBeVisible();
    // No A to Z on the page, and no stop list fetched until the finder is used.
    await expect(stopList(page)).toHaveCount(0);
    expect(stopRequests).toEqual([]);

    await stopFinder(page).fill('picc');
    await page.getByRole('option', { name: /^Piccadilly/ }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAPIC$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Piccadilly' })).toBeVisible();
    expect(stopRequests).toHaveLength(1);
  });

  test('the finder works by keyboard, and its A to Z link lists every stop', async ({ page }) => {
    await page.goto('/transit/metrolink');
    await stopFinder(page).focus();
    await page.keyboard.type('ecc');
    await expect(page.getByRole('status').filter({ hasText: '1 stop matches.' })).toBeAttached();
    await page.keyboard.press('ArrowDown');
    await expect(stopFinder(page)).toHaveAttribute('aria-activedescendant', /.+/);
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAECC$/);

    await page.goto('/transit/metrolink');
    await page.getByRole('link', { name: 'All stops, A to Z' }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Manchester Metrolink stops' })).toBeVisible();
    await expect(stopList(page).getByRole('link')).toHaveText(['Deansgate-Castlefield', 'Eccles', 'Piccadilly']);
  });

  test('Enter on several matches opens the A to Z filtered, and the old ?q= goes there too', async ({ page }) => {
    await page.goto('/transit/tfl');
    await stopFinder(page).fill('a');
    await expect(page.getByRole('option').first()).toBeVisible();
    await stopFinder(page).press('Enter');
    await expect(page).toHaveURL(/\/transit\/tfl\/stops\?q=a$/);
    await expect(page.getByRole('textbox', { name: 'Find a stop' })).toHaveValue('a');

    await page.goto('/transit/metrolink?q=picc');
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\?q=picc$/);
    await expect(stopList(page).getByRole('link')).toHaveText(['Piccadilly']);
  });

  test("the TfL network's finder opens TfL's own stop page", async ({ page }) => {
    await page.goto('/transit/tfl');
    await stopFinder(page).fill('kings cross');
    const option = page.getByRole('option', { name: /^King's Cross St\. Pancras/ });
    // Named with its lines.
    await expect(option).toContainText('Victoria');
    await option.click();
    await expect(page).toHaveURL(/\/tfl\/stops\/940GZZLUKSX$/);
    await expect(page.getByRole('heading', { level: 1, name: "King's Cross St. Pancras" })).toBeVisible();
  });

  for (const network of ['metrolink', 'tfl']) {
    test(`the ${network} network page stays light: no stop list in its HTML or DOM`, async ({ page, request }) => {
      const response = await request.get(`/transit/${network}`);
      const html = await response.text();
      expect(html.length).toBeLessThan(NETWORK_PAGE_MAX_HTML_BYTES);
      expect(html).not.toContain('aria-label="Stops"');
      await page.goto(`/transit/${network}`);
      await expect(stopFinder(page)).toBeVisible();
      const elements = await page.evaluate(() => document.getElementsByTagName('*').length);
      expect(elements).toBeLessThan(NETWORK_PAGE_MAX_ELEMENTS);
    });
  }

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

  test('clicking a stop in the A to Z opens its stop page', async ({ page }) => {
    const stopHeading = page.getByRole('heading', { level: 1, name: 'Piccadilly' });

    // From the A to Z.
    await page.goto('/transit/metrolink/stops');
    await stopList(page).getByRole('link', { name: 'Piccadilly' }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAPIC$/);
    await expect(stopHeading).toBeVisible();

    // And by keyboard: Enter on the focused link.
    await page.goto('/transit/metrolink/stops');
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
    // Live arrivals are on each stop's page, and the seed's Tube and DLR
    // lines are never called trams.
    await expect(page.getByText(/Each stop's page has live arrivals/)).toBeVisible();
    await expect(page.locator('main')).not.toContainText(/tram/i);
    await expect(page.locator('meta[name="description"]')).toHaveAttribute(
      'content',
      'London (TfL): Tube and DLR lines, stops and scheduled times.',
    );
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
