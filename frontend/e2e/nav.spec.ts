import { test, expect, type Page } from '@playwright/test';

// Task 1.2 (docs/superpowers/sdd/2026-09-17-ux-fixes-phase1-crosscutting):
// the nav bar's two layout defects, and the regression net for both.
//
// ---------------------------------------------------------------------------
// What was wrong, measured
// ---------------------------------------------------------------------------
// The bar was one flat, wrapping `Group` of ten items. Against a live
// backend at the viewports below, that produced:
//
//   390x900, logged out : 195px tall, five rows   (both engines)
//   1440x900, logged in : 104px tall, two rows    (Chromium)
//   1440x900, logged in :  63px tall, one row     (Firefox)
//   1440x900, logged out:  61px tall, one row     (both engines)
//
// The 1440px pair is the interesting one: the SAME markup wrapped in
// Chromium and did not in Firefox, because the content measured 1112px in
// Chromium and 1079px in Firefox inside a 1100px container -- Gecko's
// narrower text metrics, roughly 4%, straddling the threshold. That is
// why this file runs in both engines (see `playwright.config.ts`'s
// `firefox-nav` project) instead of trusting one: a fix verified only in
// Chromium could leave a wrap that only Firefox users see, or vice versa.
//
// ---------------------------------------------------------------------------
// What the fix is, and therefore what this asserts
// ---------------------------------------------------------------------------
// Below `md` the destinations move into a burger-opened drawer
// (`components/AppNavDrawer.tsx`); at `md` and up the per-account items
// move under an avatar-keyed menu (`components/AccountMenu.tsx`), and the
// bar gets a minimum height so the anonymous and authenticated versions
// agree. The three things worth pinning are therefore: the bar is ONE ROW
// at 1440 whoever is looking, the two sessions produce the SAME HEIGHT,
// and everything that left the bar below `md` is still reachable from the
// drawer.
//
// Like every other spec in this directory (see e2e/chat.spec.ts's own
// comment), this drives the REAL app through `playwright.config.ts`'s
// `webServer` or a real deployed target (`E2E_BASE_URL`).
//
// `E2E_SESSION_COOKIE` is the RAW `distant_signal_session` cookie value
// (the `sessions` table stores its SHA-256 hex -- see crates/api/src/auth.rs
// `hash_session_token`), exactly as e2e/accessibility.spec.ts uses it.
const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

/** Not `Secure`: the backend only marks the cookie Secure when it is
 * serving over HTTPS (crates/api/src/routes/auth.rs `cookie_secure`), so
 * over a local http:// origin a Secure cookie would simply never be sent.
 * `domain` is taken from `E2E_BASE_URL` (not hardcoded) because browsers
 * treat `localhost` and `127.0.0.1` as different cookie domains -- a
 * hardcoded `localhost` here silently drops the cookie, and with it the
 * account menu, whenever the app is served on `127.0.0.1` (as CI's
 * frontend-e2e job does). Copied from e2e/accessibility.spec.ts's own
 * `sessionState`. */
function sessionState(value: string) {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  return {
    cookies: [
      {
        name: 'distant_signal_session',
        value,
        domain: base.hostname,
        path: '/',
        httpOnly: true,
        secure: false,
        sameSite: 'Lax' as const,
        expires: -1,
      },
    ],
    origins: [],
  };
}

/** The nav's own rendered height, border included. */
async function navHeight(page: Page): Promise<number> {
  const box = await page.locator('nav').boundingBox();
  if (!box) throw new Error('The nav did not render at all.');
  return Math.round(box.height);
}

/** Every direct child of the bar's outer `Group` starts on the same row.
 * This is the real "did it wrap" question — a height assertion alone
 * could be satisfied by a taller single row, and a `flex-wrap` check
 * alone says nothing about whether wrapping actually happened. */
async function barRowCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const group = document.querySelector('nav .mantine-Group-root');
    if (!group) throw new Error('The nav Group did not render.');
    const tops = new Set<number>();
    for (const child of group.children) {
      const rect = child.getBoundingClientRect();
      // Children of different intrinsic heights are centred against each
      // other, so round to the nearest 10px band rather than comparing
      // exact tops: a genuine second row is ~40px down, far outside it.
      tops.add(Math.round(rect.top / 10));
    }
    return tops.size;
  });
}

/** Nothing in the page scrolls sideways. A "fixed height, no wrap" bar
 * that achieves it by overflowing its container would pass the two
 * assertions above and still be broken. */
async function hasHorizontalOverflow(page: Page): Promise<boolean> {
  return page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
}

test.describe('desktop nav bar (1440x900)', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test('renders on a single row, at the standard height, logged out', async ({ page }) => {
    await page.goto('/lines');
    await expect(page.locator('nav')).toBeVisible();
    expect(await barRowCount(page)).toBe(1);
    expect(await navHeight(page)).toBe(61);
    expect(await hasHorizontalOverflow(page)).toBe(false);
  });

  test('shows the primary links inline, and no burger', async ({ page }) => {
    await page.goto('/lines');
    const nav = page.locator('nav');
    for (const label of ['All Lines', 'Station Lookup', 'Find a Train', 'Incident Archive']) {
      await expect(nav.getByRole('link', { name: label })).toBeVisible();
    }
    await expect(nav.getByRole('button', { name: 'Navigation menu' })).toBeHidden();
  });

  test.describe('logged in', () => {
    test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value');
    test.use({ storageState: SESSION_COOKIE ? sessionState(SESSION_COOKIE) : undefined });

    // THE regression this whole task exists for: before the account
    // menu, this was 104px and two rows in Chromium while the logged-out
    // bar next to it was 61px and one row.
    test('renders on a single row, at the same height as the logged-out bar', async ({ page }) => {
      await page.goto('/lines');
      await expect(page.locator('nav').getByRole('button', { name: /^Account menu for/ })).toBeVisible();
      expect(await barRowCount(page)).toBe(1);
      expect(await navHeight(page)).toBe(61);
      expect(await hasHorizontalOverflow(page)).toBe(false);
    });

    test('keeps My Trains & Tickets, Groups and Log out reachable from the account menu', async ({
      page,
    }) => {
      await page.goto('/lines');
      await page.locator('nav').getByRole('button', { name: /^Account menu for/ }).click();

      const menu = page.getByRole('menu');
      await expect(menu.getByRole('menuitem', { name: 'My Trains & Tickets' })).toHaveAttribute(
        'href',
        '/track/mine',
      );
      await expect(menu.getByRole('menuitem', { name: 'Groups' })).toHaveAttribute('href', '/groups');
      await expect(menu.getByRole('menuitem', { name: 'Log out' })).toBeVisible();
    });

    test('keeps those three OUT of the bar itself', async ({ page }) => {
      await page.goto('/lines');
      const nav = page.locator('nav');
      await expect(nav.getByRole('link', { name: 'My Trains & Tickets' })).toHaveCount(0);
      await expect(nav.getByRole('link', { name: 'Groups' })).toHaveCount(0);
      await expect(nav.getByRole('button', { name: 'Log out' })).toHaveCount(0);
    });
  });
});

test.describe('phone nav bar (390x844)', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('collapses to one row instead of the old five', async ({ page }) => {
    await page.goto('/lines');
    expect(await barRowCount(page)).toBe(1);
    expect(await navHeight(page)).toBe(61);
    expect(await hasHorizontalOverflow(page)).toBe(false);
  });

  test('keeps the brand and the three always-on controls in the bar', async ({ page }) => {
    const nav = page.locator('nav');
    await page.goto('/lines');
    await expect(nav.getByRole('link', { name: 'Distant Signal' })).toBeVisible();
    await expect(nav.getByRole('button', { name: /^Theme:/ })).toBeVisible();
    await expect(nav.getByRole('button', { name: /^Pride mode:/ })).toBeVisible();
    await expect(nav.getByRole('link', { name: 'Log in' })).toBeVisible();
  });

  test('moves the destinations into a burger-opened drawer', async ({ page }) => {
    await page.goto('/lines');
    const nav = page.locator('nav');
    await expect(nav.getByRole('link', { name: 'All Lines' })).toBeHidden();

    await nav.getByRole('button', { name: 'Navigation menu' }).click();
    const drawer = page.getByRole('dialog', { name: 'Menu' });
    for (const label of [
      'All Lines',
      'Station Lookup',
      'Find a Train',
      'Incident Archive',
      'My Trains & Tickets',
    ]) {
      await expect(drawer.getByRole('link', { name: label })).toBeVisible();
    }
  });

  test('closes the drawer on navigating, rather than leaving it over the new page', async ({
    page,
  }) => {
    await page.goto('/lines');
    await page.locator('nav').getByRole('button', { name: 'Navigation menu' }).click();
    const drawer = page.getByRole('dialog', { name: 'Menu' });
    await drawer.getByRole('link', { name: 'Station Lookup' }).click();

    await expect(page).toHaveURL(/\/stations$/);
    await expect(drawer).toBeHidden();
  });
});

test('the drawer closes itself if the window grows past the breakpoint while it is open', async ({
  page,
}) => {
  // Hiding the burger does not close an already-open drawer, so without
  // the media-query effect in AppNavDrawer this leaves a modal overlay
  // over the page with no visible control that opened it.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/lines');
  await page.locator('nav').getByRole('button', { name: 'Navigation menu' }).click();
  await expect(page.getByRole('dialog', { name: 'Menu' })).toBeVisible();

  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(page.getByRole('dialog', { name: 'Menu' })).toBeHidden();
});

// The breakpoint itself, from both sides. 992px is Mantine's `md`; the
// swap must be complete at it and not yet started one pixel below.
test.describe('the md breakpoint the bar pivots on', () => {
  test('shows the inline links and no burger at exactly 992px', async ({ page }) => {
    await page.setViewportSize({ width: 992, height: 900 });
    await page.goto('/lines');
    const nav = page.locator('nav');
    await expect(nav.getByRole('link', { name: 'All Lines' })).toBeVisible();
    await expect(nav.getByRole('button', { name: 'Navigation menu' })).toBeHidden();
    expect(await barRowCount(page)).toBe(1);
    expect(await hasHorizontalOverflow(page)).toBe(false);
  });

  test('shows the burger and no inline links at 991px', async ({ page }) => {
    await page.setViewportSize({ width: 991, height: 900 });
    await page.goto('/lines');
    const nav = page.locator('nav');
    await expect(nav.getByRole('button', { name: 'Navigation menu' })).toBeVisible();
    await expect(nav.getByRole('link', { name: 'All Lines' })).toBeHidden();
    expect(await barRowCount(page)).toBe(1);
  });
});
