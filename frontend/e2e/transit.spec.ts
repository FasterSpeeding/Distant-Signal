import { test, expect } from '@playwright/test';

// The tram pages against the live api and the seed's metrolink network
// (frontend/e2e/fixtures/seed.sql; API_TRANSIT_NETWORKS=metrolink in the
// frontend-e2e job). A smoke test: each page renders its data, and the
// board says it is scheduled, not live.

test.describe('transit pages', () => {
  test('the network page lists its lines and stops', async ({ page }) => {
    await page.goto('/transit/metrolink');
    await expect(page.getByRole('heading', { level: 1, name: 'Manchester Metrolink' })).toBeVisible();
    await expect(page.getByRole('link', { name: /^Blue Line/ })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Piccadilly' })).toBeVisible();
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
    await expect(page.getByRole('link', { name: /Blue Line/ })).toContainText('Minor delays');
    await expect(page.getByRole('link', { name: /Green Line/ })).toContainText('No reported disruption');
    const now = page.getByRole('list', { name: 'Disruptions now' });
    await expect(now).toContainText('Delays between Piccadilly and Eccles');
    await expect(page.getByRole('list', { name: 'Disruptions coming up' })).toContainText(
      'No trams to Deansgate-Castlefield',
    );
  });

  test('a stop shows its notices, and a line its status', async ({ page }) => {
    await page.goto('/transit/metrolink/stops/9400ZZMAPIC?date=2026-09-17');
    await expect(page.getByRole('link', { name: 'MAN' })).toHaveAttribute('href', '/stations/MAN');
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

  test('an unknown network is not found', async ({ page }) => {
    const response = await page.goto('/transit/nowhere');
    expect(response?.status()).toBe(404);
  });
});
