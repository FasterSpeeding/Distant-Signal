import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Opens the phone-viewport nav drawer and returns it.
 *
 * `page.goto` resolves on `load`, which can land before React has hydrated
 * the server-rendered burger button; a click in that window does nothing, so
 * the drawer never opens and anything waiting on it times out. That was the
 * cause of the intermittent nav-drawer failures in `nav.spec.ts` and
 * `accessibility.spec.ts`. Retrying the click until the dialog is actually
 * visible makes the open deterministic: a pre-hydration no-op click is simply
 * repeated, and once the drawer is visible no further click is made.
 */
export async function openNavDrawer(page: Page): Promise<Locator> {
  const burger = page.locator('nav').getByRole('button', { name: 'Navigation menu' });
  const drawer = page.getByRole('dialog', { name: 'Menu' });
  await expect(async () => {
    if (!(await drawer.isVisible())) {
      await burger.click();
    }
    await expect(drawer).toBeVisible({ timeout: 2_000 });
  }).toPass({ timeout: 20_000 });
  return drawer;
}
