import { test, expect, type BrowserContext, type Page } from '@playwright/test';

// Following a journey shared with you (DS-146), against the seeded backend
// (e2e/fixtures/seed.sql): fixture-member shares "Fixture walk and Tube"
// (2099) into fixture-group-1, and fixture-owner, the session here,
// follows it from the group page. It then lists among their own journeys,
// labelled with whose it is, and opens read-only; unfollowing puts it back.
// Serial, and unfollowed again at the end: other specs and the visual
// screenshots read the same group and Journeys page.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const GROUP_ID = process.env.E2E_GROUP_ID ?? 'fixture-group-1';
const JOURNEY_NAME = 'Fixture walk and Tube';

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

/** The group page's card for the shared journey. */
function groupCard(page: Page) {
  return page.locator('.mantine-Card-root').filter({ has: page.getByRole('link', { name: JOURNEY_NAME }) });
}

test.describe.configure({ mode: 'serial' });

test.describe('following a shared journey', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  let journeyPath: string | null = null;

  test.afterAll(async ({ browser }) => {
    if (!journeyPath) return;
    const context = await browser.newContext();
    await logIn(context);
    const page = await context.newPage();
    await page.goto(journeyPath);
    // Leave the seed as it was, whatever happened above.
    await page.evaluate(async (path) => {
      await fetch(`/api${path.replace('/journeys/', '/Journeys/')}/follow`, { method: 'DELETE' });
    }, journeyPath);
    await context.close();
  });

  test('follow from the group, see it with your own journeys read-only, then unfollow', async ({ page, context }) => {
    await logIn(context);
    await page.goto(`/groups/${GROUP_ID}`);
    const card = groupCard(page);
    journeyPath = await card.getByRole('link', { name: JOURNEY_NAME }).getAttribute('href');
    expect(journeyPath).toMatch(/^\/journeys\/\d+$/);
    await card.getByRole('button', { name: 'Follow this journey' }).click();
    await expect(card.getByRole('button', { name: 'Unfollow' })).toBeVisible();

    // The Journeys page lists it with the caller's own, labelled, not as a
    // group-shared row.
    await page.goto('/journeys');
    const upcoming = page.getByRole('region', { name: 'Upcoming' });
    const row = upcoming.locator('.mantine-Card-root').filter({ has: page.getByRole('link', { name: JOURNEY_NAME }) });
    await expect(row).toHaveCount(1);
    await expect(row.getByText('Shared by Rider Fixture')).toBeVisible();
    await expect(row.getByText('From Fixture Family')).toHaveCount(0);

    // And so does the home page.
    await page.goto('/');
    const home = page.getByRole('region', { name: 'Upcoming journeys' });
    // The card's link is named by the journey alone; the rest is in the card.
    const homeRow = home
      .locator('[data-clickable-card]')
      .filter({ has: page.getByRole('link', { name: JOURNEY_NAME }) });
    await expect(homeRow).toBeVisible();
    await expect(homeRow.getByText(/Shared by Rider Fixture/)).toBeVisible();

    // Its page is read-only, with Unfollow.
    await page.goto(journeyPath!);
    await expect(page.getByText('Shared by Rider Fixture')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Delete journey' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Add a leg' })).toHaveCount(0);
    await page.getByRole('button', { name: 'Unfollow' }).click();
    await expect(page.getByRole('button', { name: 'Follow this journey' })).toBeVisible();

    // Unfollowed: back to a group-shared row on the Journeys page.
    await page.goto('/journeys');
    const shared = page
      .getByRole('region', { name: 'Upcoming' })
      .locator('.mantine-Card-root')
      .filter({ has: page.getByRole('link', { name: JOURNEY_NAME }) });
    await expect(shared.getByText('From Fixture Family')).toBeVisible();
    await expect(shared.getByRole('button', { name: 'Follow this journey' })).toBeVisible();
  });
});
