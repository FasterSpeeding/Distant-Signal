import { test, expect, type BrowserContext } from '@playwright/test';

// The Journeys page (DS-115): `/journeys`, which was the Track page at
// `/track/mine`, and "New journey", which is reached from it and from the
// home page's "Upcoming journeys" instead of from the nav. Against the
// seeded backend (e2e/fixtures/seed.sql).

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

async function logIn(context: BrowserContext) {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  await context.addCookies([
    {
      name: 'distant_signal_session',
      value: SESSION_COOKIE!,
      domain: base.hostname,
      path: '/',
      expires: -1,
      httpOnly: true,
      secure: base.protocol === 'https:',
      sameSite: 'Lax',
    },
  ]);
}

test.describe('the Journeys page, logged out', () => {
  test('the old Track page and tickets URLs redirect to it for good, in one hop', async ({ page, request }) => {
    for (const path of ['/track/mine', '/track/tickets']) {
      const response = await request.get(path, { maxRedirects: 0 });
      expect(response.status(), path).toBe(308);
      expect(new URL(response.headers().location!, 'http://x').pathname, path).toBe('/journeys');
    }
    await page.goto('/track/mine');
    await expect(page).toHaveURL(/\/journeys$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Journeys');
  });

  test('offers New journey and a log in prompt, and New journey opens the form', async ({ page }) => {
    await page.goto('/journeys');
    const main = page.getByRole('main');
    await expect(main.getByRole('link', { name: 'Log in' })).toBeVisible();
    await main.getByRole('link', { name: 'New journey' }).click();
    await expect(page).toHaveURL(/\/journeys\/new$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('New journey');
  });

  test('the nav links Journeys, and has no new-journey entry of its own', async ({ page }) => {
    await page.setViewportSize({ width: 1366, height: 900 });
    await page.goto('/lines');
    const nav = page.locator('nav[aria-label="Main"]');
    await expect(nav.getByRole('link', { name: 'Journeys', exact: true })).toHaveAttribute('href', '/journeys');
    await expect(nav.getByRole('link', { name: 'New journey' })).toHaveCount(0);
    await expect(nav.getByRole('link', { name: 'Track a journey' })).toHaveCount(0);
    await expect(nav.getByRole('link', { name: 'Track', exact: true })).toHaveCount(0);
  });
});

test.describe('the Journeys page, logged in', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  test.beforeEach(async ({ context }) => {
    await logIn(context);
  });

  test('lists Upcoming first, with New journey at the top', async ({ page }) => {
    await page.goto('/journeys');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Journeys');
    const upcoming = page.getByRole('region', { name: 'Upcoming' });
    await expect(upcoming).toBeVisible();
    const headings = await page.getByRole('main').getByRole('heading').allTextContents();
    expect(headings[0]).toBe('Journeys');
    expect(headings[1]).toBe('Upcoming');
    // "Your reliability", when shown, sits between Upcoming and Past.
    const reliability = headings.indexOf('Your reliability');
    const past = headings.indexOf('Past');
    if (reliability !== -1 && past !== -1) expect(reliability).toBeLessThan(past);
    await page.getByRole('main').getByRole('link', { name: 'New journey' }).click();
    await expect(page).toHaveURL(/\/journeys\/new$/);
  });

  // DS-178: Past is one list, newest first, with journeys shared into the
  // caller's groups among their own (the seed's 900101 on 18 Sep, the
  // caller's own 900001 on 17 Sep, 900102 on 10 Sep), each shared one
  // still tagged with its group and sharer.
  test('lists Past newest first, with shared journeys among the caller’s own', async ({ page }) => {
    await page.goto('/journeys');
    const past = page.getByRole('region', { name: 'Past' });
    const hrefs = await past.getByRole('link').evaluateAll((links) => links.map((link) => link.getAttribute('href')));
    const later = hrefs.indexOf('/journeys/900101');
    const own = hrefs.indexOf('/journeys/900001');
    const earlier = hrefs.indexOf('/journeys/900102');
    expect(later, hrefs.join(' ')).toBeGreaterThanOrEqual(0);
    expect(later).toBeLessThan(own);
    expect(own).toBeLessThan(earlier);
    for (const name of ['Fixture shared later walk', 'Fixture shared earlier walk']) {
      const row = past.locator('.mantine-Card-root').filter({ has: page.getByRole('link', { name }) });
      await expect(row.getByText('From Fixture Family')).toBeVisible();
      await expect(row.getByText('Shared by Rider Fixture')).toBeVisible();
    }
    const ownRow = past.locator('.mantine-Card-root').filter({ has: page.locator('a[href="/journeys/900001"]') });
    await expect(ownRow.getByText('From Fixture Family')).toHaveCount(0);
  });

  test("the home page's Upcoming journeys offers New journey", async ({ page }) => {
    await page.goto('/');
    const upcoming = page.getByRole('region', { name: 'Upcoming journeys' });
    await upcoming.getByRole('link', { name: 'New journey' }).click();
    await expect(page).toHaveURL(/\/journeys\/new$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('New journey');
  });
});
