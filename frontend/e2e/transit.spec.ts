import { test, expect, type Page } from '@playwright/test';

// The tram pages against the live api and the seed's metrolink network
// (frontend/e2e/fixtures/seed.sql; API_TRANSIT_NETWORKS=metrolink in the
// frontend-e2e job). A smoke test: each page renders its data, and the
// board says it is scheduled, not live.

/** The network page's A to Z of stops (the lift notice in its
 * disruptions links Piccadilly too). */
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
    const now = page.getByRole('list', { name: 'Disruptions now' });
    await expect(now).toContainText('Delays between Piccadilly and Eccles');
    await expect(page.getByRole('list', { name: 'Disruptions coming up' })).toContainText(
      'No trams to Deansgate-Castlefield',
    );
  });

  test('a stop shows its notices, and a line its status', async ({ page }) => {
    await page.goto('/transit/metrolink/stops/9400ZZMAPIC?date=2026-09-17');
    await expect(page.getByRole('link', { name: 'MAN', exact: true })).toHaveAttribute('href', '/stations/MAN');
    await expect(page.getByRole('list', { name: 'Notices' })).toContainText('Lift out of service at Piccadilly');
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

    // From the lift notice in the disruptions, which names the stop.
    await page.goto('/transit/metrolink');
    const now = page.getByRole('list', { name: 'Disruptions now' });
    await now.getByRole('link', { name: 'Piccadilly' }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAPIC$/);
    await expect(stopHeading).toBeVisible();

    // And by keyboard: Enter on the focused link.
    await page.goto('/transit/metrolink');
    await now.getByRole('link', { name: 'Piccadilly' }).focus();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAPIC$/);
    await expect(stopHeading).toBeVisible();
  });

  test('a line page and its timetable link their stops', async ({ page }) => {
    await page.goto('/transit/metrolink/lines/blue');
    await page.getByRole('link', { name: 'Eccles' }).first().click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAECC$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Eccles' })).toBeVisible();

    await page.goto('/transit/metrolink/lines/blue/timetable?date=2026-09-17&direction=outbound&from=07:00');
    await page.getByRole('rowheader', { name: 'Eccles' }).getByRole('link', { name: 'Eccles' }).click();
    await expect(page).toHaveURL(/\/transit\/metrolink\/stops\/9400ZZMAECC$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Eccles' })).toBeVisible();
  });

  test('an unknown network is not found', async ({ page }) => {
    const response = await page.goto('/transit/nowhere');
    expect(response?.status()).toBe(404);
  });
});
