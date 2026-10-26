import { test, expect, type BrowserContext, type Page } from '@playwright/test';
import { openNavDrawer } from './navDrawer';

// /account/preferences in both auth states (app/account/preferences/page.tsx).
//
// Logged out, the page is open and complete for what this browser keeps
// (colour theme, pride flag), with one prompt to log in for the account
// sections. Those device-local choices survive a reload, apply on every
// page, and are left alone by logging in (the account keeps no copy, so
// there is nothing to merge). Logged in, the account sections replace the
// prompt.
//
// Drives the real app, as every spec here does. `E2E_SESSION_COOKIE` is the
// raw `distant_signal_session` value the seed's session backs (see
// e2e/nav.spec.ts's own comment).
const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const PREFERENCES = '/account/preferences';

async function logIn(context: BrowserContext): Promise<void> {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  await context.addCookies([
    {
      name: 'distant_signal_session',
      value: SESSION_COOKIE!,
      domain: base.hostname,
      path: '/',
      httpOnly: true,
      secure: false,
      sameSite: 'Lax',
      expires: -1,
    },
  ]);
}

/** Picks a pride flag on the Preferences page. Retried until the choice
 * lands on `<body>`: a `selectOption` before hydration changes the native
 * select but runs no handler. */
async function choosePrideFlag(page: Page, label: string, value: string): Promise<void> {
  const select = page.getByRole('combobox', { name: 'Pride flag' });
  await expect(async () => {
    await select.selectOption({ label });
    await expect(page.locator('body')).toHaveAttribute('data-pride', value, { timeout: 1_000 });
  }).toPass({ timeout: 20_000 });
}

/** The flag's stripe along the top of the page (`globals.css`'s
 * `body[data-pride=...]::before`), in pixels: 0 when there is none. */
async function prideStripeHeight(page: Page): Promise<number> {
  return page.evaluate(() => {
    const before = getComputedStyle(document.body, '::before');
    return before.content === 'none' ? 0 : parseFloat(before.height) || 0;
  });
}

test.describe('Preferences, logged out', () => {
  test('opens without a redirect, with appearance in full and one log in prompt', async ({ page }) => {
    const response = await page.goto(PREFERENCES);
    expect(response?.status()).toBe(200);
    expect(new URL(page.url()).pathname).toBe(PREFERENCES);

    await expect(page.getByRole('heading', { level: 1, name: 'Preferences' })).toBeVisible();
    const appearance = page.getByRole('region', { name: 'Appearance' });
    await expect(appearance.getByRole('radiogroup', { name: 'Colour theme' })).toBeVisible();
    await expect(appearance.getByRole('combobox', { name: 'Pride flag' })).toBeVisible();

    const account = page.getByRole('region', { name: 'Account settings' });
    const login = account.getByRole('link', { name: 'Log in to manage notifications and more' });
    await expect(login).toBeVisible();
    expect(await login.getAttribute('href')).toContain('return_to=%2Faccount%2Fpreferences');

    await expect(page.getByRole('heading', { name: 'Notifications' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Enable notifications' })).toHaveCount(0);
    await expect(page.getByLabel('Claude API key')).toHaveCount(0);
  });

  test('keeps the pride flag across reloads and on every page', async ({ page }) => {
    const cspErrors: string[] = [];
    page.on('console', (message) => {
      if (/Content Security Policy/i.test(message.text())) cspErrors.push(message.text());
    });

    await page.goto(PREFERENCES);
    expect(await prideStripeHeight(page)).toBe(0);
    await choosePrideFlag(page, 'Trans', 'trans');
    expect(await prideStripeHeight(page)).toBe(6);

    await page.reload();
    await expect(page.locator('body')).toHaveAttribute('data-pride', 'trans');
    await expect(page.getByRole('combobox', { name: 'Pride flag' })).toHaveValue('trans');

    for (const path of ['/', '/lines', '/stations']) {
      await page.goto(path);
      await expect(page.locator('body')).toHaveAttribute('data-pride', 'trans');
      expect(await prideStripeHeight(page)).toBe(6);
      // The trans flag's sparkle trio around the wordmark, decorative only.
      const sparkles = page.locator('nav[aria-label="Main"] .prideSparkles:visible');
      await expect(sparkles).toHaveAttribute('aria-hidden', 'true');
      await expect(sparkles.locator('.prideSparkle')).toHaveText(['🩵', '🩷', '✨']);
    }

    // The flag is applied by an inline script before the first paint,
    // which needs the page's CSP nonce: no violation means it ran.
    expect(cspErrors).toEqual([]);

    await page.goto(PREFERENCES);
    // The picker's live preview carries the same trio.
    await expect(page.locator('.pridePreview')).toHaveAttribute('data-flag', 'trans');
    await expect(page.locator('main .prideSparkle')).toHaveText(['🩵', '🩷', '✨']);
    await choosePrideFlag(page, 'Off', 'off');
    // Gone at once, not only after a reload.
    await expect(page.locator('.prideSparkles:visible')).toHaveCount(0);
    await page.reload();
    expect(await prideStripeHeight(page)).toBe(0);
    await expect(page.locator('.prideSparkles:visible')).toHaveCount(0);
  });

  test('keeps the colour theme across reloads', async ({ page }) => {
    await page.goto(PREFERENCES);
    const dark = page.getByRole('radiogroup', { name: 'Colour theme' }).getByText('Dark', { exact: true });
    await expect(async () => {
      await dark.click();
      await expect(page.locator('html')).toHaveAttribute('data-mantine-color-scheme', 'dark', { timeout: 1_000 });
    }).toPass({ timeout: 20_000 });

    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-mantine-color-scheme', 'dark');
    await page.goto('/lines');
    await expect(page.locator('html')).toHaveAttribute('data-mantine-color-scheme', 'dark');
  });

  test.describe('on a desktop', () => {
    test.use({ viewport: { width: 1366, height: 900 } });

    test('links Preferences from the bar and the footer', async ({ page }) => {
      await page.goto('/lines');
      const footerLink = page.getByRole('navigation', { name: 'Site information' }).getByRole('link', {
        name: 'Preferences',
      });
      await expect(footerLink).toHaveAttribute('href', PREFERENCES);
      const gear = page.locator('nav[aria-label="Main"]').getByRole('link', { name: 'Preferences' });
      await expect(gear).toHaveAttribute('href', PREFERENCES);
      await gear.click();
      await expect(page.getByRole('heading', { level: 1, name: 'Preferences' })).toBeVisible();
    });
  });

  test.describe('on a phone', () => {
    test.use({ viewport: { width: 390, height: 844 } });

    test('links Preferences from the drawer, and no account pages', async ({ page }) => {
      await page.goto('/lines');
      const drawer = await openNavDrawer(page);
      await expect(drawer.getByRole('link', { name: 'Preferences', exact: true })).toHaveAttribute('href', PREFERENCES);
      await expect(drawer.getByRole('link', { name: 'Account & data' })).toHaveCount(0);
      await expect(drawer.getByRole('link', { name: 'Groups' })).toHaveCount(0);
    });
  });
});

test.describe('Preferences, logged in', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value');

  test('shows the account sections in place of the log in prompt', async ({ page, context }) => {
    await logIn(context);
    await page.goto(PREFERENCES);
    await expect(page.getByRole('region', { name: 'Notifications' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Appearance' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Account settings' })).toHaveCount(0);
    await expect(page.getByRole('link', { name: /^Log in/ })).toHaveCount(0);
  });

  test('keeps the choices made while logged out', async ({ page, context }) => {
    await page.goto(PREFERENCES);
    await choosePrideFlag(page, 'Rainbow', 'rainbow');

    await logIn(context);
    await page.goto(PREFERENCES);
    await expect(page.getByRole('region', { name: 'Notifications' })).toBeVisible();
    await expect(page.locator('body')).toHaveAttribute('data-pride', 'rainbow');
    await expect(page.getByRole('combobox', { name: 'Pride flag' })).toHaveValue('rainbow');
  });

  test.describe('on a desktop', () => {
    test.use({ viewport: { width: 1366, height: 900 } });

    test('links Preferences from the account menu and the footer, not a gear', async ({ page, context }) => {
      await logIn(context);
      await page.goto('/lines');
      // No gear when logged in: the account menu is the bar's way there.
      await expect(page.locator('nav[aria-label="Main"]').getByRole('link', { name: 'Preferences' })).toHaveCount(0);
      await page
        .locator('nav[aria-label="Main"]')
        .getByRole('button', { name: /, account menu for / })
        .click();
      await expect(page.getByRole('menu').getByRole('menuitem', { name: 'Preferences' })).toHaveAttribute(
        'href',
        PREFERENCES,
      );
      await page.keyboard.press('Escape');
      await expect(
        page.getByRole('navigation', { name: 'Site information' }).getByRole('link', { name: 'Preferences' }),
      ).toHaveAttribute('href', PREFERENCES);
    });
  });

  test.describe('on a phone', () => {
    test.use({ viewport: { width: 390, height: 844 } });

    test('links Preferences from the drawer, beside the account pages', async ({ page, context }) => {
      await logIn(context);
      await page.goto('/lines');
      const drawer = await openNavDrawer(page);
      await expect(drawer.getByRole('link', { name: 'Preferences', exact: true })).toHaveAttribute('href', PREFERENCES);
      await expect(drawer.getByRole('link', { name: 'Account & data' })).toBeVisible();
    });
  });
});
