import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// "Use my location" on /stations (app/stations/NearbyStationList.tsx),
// against the live api and the seed's two stations with coordinates
// (frontend/e2e/fixtures/seed.sql: London Kings Cross and London Euston).
// The browser's location is mocked to a point between them, so the real
// /api/stations/nearby lookup runs.

const NEAR_KINGS_CROSS = { latitude: 51.53, longitude: -0.13 };

test.describe('nearby stations', () => {
  test.use({ geolocation: NEAR_KINGS_CROSS, permissions: ['geolocation'] });

  test('each result is one link to its station, with the distance set apart', async ({ page }) => {
    await page.goto('/stations');
    await page.getByRole('button', { name: 'Use my location' }).click();

    const list = page.getByRole('list', { name: 'Nearest stations' });
    await expect(list).toBeVisible();
    const items = list.getByRole('listitem');
    await expect(items).toHaveCount(2);

    // Nearest first. The accessible name carries the name, code and distance.
    const euston = list.getByRole('link', { name: /^London Euston \(EUS\), \d+\.\d km away$/ });
    const kingsCross = list.getByRole('link', { name: /^London Kings Cross \(KGX\), \d+\.\d km away$/ });
    await expect(items.nth(0).getByRole('link')).toHaveAccessibleName(/^London Euston/);
    await expect(euston).toHaveAttribute('href', '/stations/EUS');
    await expect(kingsCross).toHaveAttribute('href', '/stations/KGX');

    for (const link of [euston, kingsCross]) {
      // One interactive element per row, and a full-size tap target.
      await expect(link.locator('a, button, input')).toHaveCount(0);
      const box = await link.boundingBox();
      expect(box?.height).toBeGreaterThanOrEqual(44);

      // The distance is its own element, to the right of the name.
      const distance = link.locator('[data-distance]');
      await expect(distance).toHaveText(/^\d+\.\d km$/);
      const name = link.getByText(/\([A-Z]{3}\)$/);
      await expect(name).not.toContainText('km');
      const nameBox = await name.boundingBox();
      const distanceBox = await distance.boundingBox();
      expect(distanceBox!.x).toBeGreaterThan(nameBox!.x + nameBox!.width);
    }

    await euston.click();
    await expect(page).toHaveURL(/\/stations\/EUS$/, { timeout: 30_000 });
  });

  test('every row is reachable by keyboard and opens with Enter', async ({ page }) => {
    await page.goto('/stations');
    await page.getByRole('button', { name: 'Use my location' }).click();
    const first = page.getByRole('list', { name: 'Nearest stations' }).getByRole('link').first();
    await expect(first).toBeVisible();

    await page.getByRole('button', { name: 'Use my location' }).focus();
    await page.keyboard.press('Tab');
    await expect(first).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/stations\/EUS$/, { timeout: 30_000 });
  });

  // The results only exist after the click, so the page-load sweep in
  // accessibility.spec.ts never sees them.
  for (const colorScheme of ['light', 'dark'] as const) {
    test(`the results pass axe (${colorScheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme });
      await page.goto('/stations');
      await page.getByRole('button', { name: 'Use my location' }).click();
      await expect(page.getByRole('list', { name: 'Nearest stations' })).toBeVisible();
      const results = await new AxeBuilder({ page }).include('[data-nearby-stations]').analyze();
      expect(results.violations.map((v) => ({ id: v.id, nodes: v.nodes.map((n) => n.target) }))).toEqual([]);
    });
  }
});

test.describe('nearby stations, location declined', () => {
  // No geolocation permission: Chromium refuses the request at once.
  test('says so calmly and points back to the search', async ({ page }) => {
    await page.goto('/stations');
    await page.getByRole('button', { name: 'Use my location' }).click();
    await expect(page.getByText('Location access was declined. You can still search by name above.')).toBeVisible();
    await expect(page.getByRole('list', { name: 'Nearest stations' })).toHaveCount(0);
  });
});
