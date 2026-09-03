import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// The route-level sweep in e2e/accessibility.spec.ts cannot see this state:
// ConnectivityMonitor's "Reconnecting..." banner only exists while the
// backend is unreachable or the device is offline, so no amount of visiting
// routes will render it. This spec forces that state and runs the same axe
// ruleset against it.
//
// It exists because it caught a real defect: Mantine hardcodes a titled
// Notification's description to gray 6, bypassing this app's
// `--mantine-color-dimmed` override, so the banner originally shipped at
// 3.32:1 on white. See the matching comment in app/globals.css.
const RULES = ['color-contrast', 'landmark-one-main', 'region', 'heading-order', 'page-has-heading-one'];

test.describe('connectivity banner', () => {
  test('renders with no axe violations while disconnected', async ({ page }) => {
    // axe's recursive frame walk is slow on this page; the default 30s is
    // not enough headroom on a loaded machine.
    test.setTimeout(120_000);

    await page.goto('/lines');
    // Hydration gate: ThemeToggle is a client component, so its button
    // being visible means React has attached its window listeners.
    await expect(page.getByRole('button', { name: /Theme:/ })).toBeVisible();
    await page.waitForTimeout(1500);

    // The same event the browser itself fires when connectivity drops, and
    // the one @mantine/hooks' useNetwork listens for. Deliberately NOT
    // `context.setOffline(true)`: that also blocks axe-core's own injection
    // and stalls the analyze() call.
    await page.evaluate(() => window.dispatchEvent(new Event('offline')));

    const status = page.getByRole('status');
    await expect(status).toContainText('Reconnecting');
    await expect(status).toHaveAttribute('aria-live', 'polite');

    const results = await new AxeBuilder({ page }).withRules(RULES).analyze();
    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
  });

  test('keeps the page content on screen while the banner is up', async ({ page }) => {
    await page.goto('/lines');
    await expect(page.getByRole('button', { name: /Theme:/ })).toBeVisible();
    await page.waitForTimeout(1500);
    await page.evaluate(() => window.dispatchEvent(new Event('offline')));

    await expect(page.getByRole('status')).toContainText('Reconnecting');
    // The whole point of the feature: the banner is non-blocking and the
    // last-known content stays put rather than being replaced by an error.
    await expect(page.getByRole('heading', { name: 'All Lines', level: 1 })).toBeVisible();
  });
});
