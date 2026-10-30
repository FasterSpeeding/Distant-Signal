import { test, expect, type APIRequestContext, type BrowserContext, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// Journeys shared with you (DS-199), against the seeded backend
// (e2e/fixtures/seed.sql): fixture-member shares two 2099 train journeys
// into fixture-group-1, "Fixture shared train" (900111, on the train they
// picked) and "Fixture shared train to pick" (900112, no train picked yet).
// fixture-owner, the session here, doesn't follow either. The home page
// lists them under "Shared with you", and both lists show the owner's
// train; "Needs a train picked" asks only the owner or a follower.
// Serial, and every follow and journey made here is undone: other specs
// and the visual screenshots read the same rows.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const GROUP_ID = process.env.E2E_GROUP_ID ?? 'fixture-group-1';
const SHARED_TRAIN = 900111;
const SHARED_TO_PICK = 900112;
const OWN_NAME = 'E2E own journey to pick';

async function logIn(context: BrowserContext) {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  await context.addCookies([
    {
      name: 'distant_signal_session',
      value: SESSION_COOKIE ?? '',
      domain: base.hostname,
      path: '/',
      expires: -1,
      httpOnly: true,
      secure: base.protocol === 'https:',
      sameSite: 'Lax',
    },
  ]);
}

const headers = () => ({ cookie: `distant_signal_session=${SESSION_COOKIE}` });

/** The card in `scope` whose link goes to journey `id`. */
function card(page: Page, scope: ReturnType<Page['getByRole']>, id: number) {
  return scope.locator('[data-clickable-card]').filter({ has: page.locator(`a[href="/journeys/${id}"]`) });
}

async function unfollow(request: APIRequestContext, id: number) {
  await request.delete(`/api/Journeys/${id}/follow`, { headers: headers() });
}

test.describe.configure({ mode: 'serial' });

test.describe('journeys shared with you', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  let ownJourneyId: number | null = null;

  test.beforeEach(async ({ context }) => {
    await logIn(context);
  });

  test.afterAll(async ({ request }) => {
    // Leave the seed as it was, whatever happened above.
    await unfollow(request, SHARED_TO_PICK);
    if (ownJourneyId !== null) {
      await request.delete(`/api/Journeys/${ownJourneyId}`, { headers: headers() });
    }
  });

  for (const colorScheme of ['light', 'dark'] as const) {
    test(`the home page lists them under Shared with you, with the owner's train (${colorScheme})`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme });
      await page.goto('/');
      const upcoming = page.getByRole('region', { name: 'Upcoming journeys' });
      const heading = upcoming.getByRole('heading', { level: 3, name: 'Shared with you' });
      await expect(heading).toBeVisible();

      const withTrain = card(page, upcoming, SHARED_TRAIN);
      await expect(withTrain).toBeVisible();
      await expect(withTrain).toContainText('Shared by Rider Fixture in Fixture Family');
      await expect(withTrain.getByText('Needs a train picked')).toHaveCount(0);
      await expect(withTrain.getByText('No train picked yet')).toHaveCount(0);

      const toPick = card(page, upcoming, SHARED_TO_PICK);
      await expect(toPick.getByText('No train picked yet')).toBeVisible();
      await expect(toPick.getByText('Needs a train picked')).toHaveCount(0);

      // Under the heading, after the caller's own.
      const headingBox = await heading.boundingBox();
      const rowBox = await withTrain.boundingBox();
      expect(headingBox && rowBox && headingBox.y < rowBox.y).toBe(true);

      const results = await new AxeBuilder({ page }).include('main').analyze();
      expect(results.violations).toEqual([]);
    });
  }

  test("the Journeys page's Upcoming shows the owner's train, and asks no one else to pick one", async ({ page }) => {
    await page.goto('/journeys');
    const upcoming = page.getByRole('region', { name: 'Upcoming' });
    const withTrain = card(page, upcoming, SHARED_TRAIN);
    await expect(withTrain.getByText('From Fixture Family')).toBeVisible();
    await expect(withTrain.getByText('Needs a train picked')).toHaveCount(0);
    await expect(withTrain.getByText('No train picked yet')).toHaveCount(0);
    const toPick = card(page, upcoming, SHARED_TO_PICK);
    await expect(toPick.getByText('No train picked yet')).toBeVisible();
    await expect(toPick.getByText('Needs a train picked')).toHaveCount(0);
  });

  test('followed, a journey with no train asks for one, and joins the caller’s own on the home page', async ({
    page,
    request,
  }) => {
    const followed = await request.post(`/api/Journeys/${SHARED_TO_PICK}/follow`, {
      headers: headers(),
      data: { groupId: GROUP_ID },
    });
    expect(followed.ok(), await followed.text()).toBe(true);

    await page.goto('/journeys');
    const row = card(page, page.getByRole('region', { name: 'Upcoming' }), SHARED_TO_PICK);
    await expect(row.getByText('Needs a train picked')).toBeVisible();
    await expect(row.getByText('Shared by Rider Fixture')).toBeVisible();

    await page.goto('/');
    const upcoming = page.getByRole('region', { name: 'Upcoming journeys' });
    const homeRow = card(page, upcoming, SHARED_TO_PICK);
    await expect(homeRow.getByText('Needs a train picked')).toBeVisible();
    // Above "Shared with you" now, with the caller's own.
    const headingBox = await upcoming.getByRole('heading', { level: 3, name: 'Shared with you' }).boundingBox();
    const rowBox = await homeRow.boundingBox();
    expect(headingBox && rowBox && rowBox.y < headingBox.y).toBe(true);

    await unfollow(request, SHARED_TO_PICK);
  });

  test('the caller’s own journey with no train asks for one on both pages', async ({ page, request }) => {
    const created = await request.post('/api/Journeys', {
      headers: headers(),
      data: {
        customName: OWN_NAME,
        leg: {
          mode: 'window',
          originCrs: 'EDB',
          destinationCrs: 'KGX',
          // A week from now: a train can be tracked up to 28 days ahead.
          serviceDate: new Date(Date.now() + 7 * 86_400_000).toISOString().slice(0, 10),
          departWindow: { after: '09:00:00' },
        },
      },
    });
    expect(created.ok(), await created.text()).toBe(true);
    ownJourneyId = ((await created.json()) as { journeyId: number }).journeyId;

    await page.goto('/journeys');
    const row = card(page, page.getByRole('region', { name: 'Upcoming' }), ownJourneyId);
    await expect(row.getByText('Needs a train picked')).toBeVisible();

    await page.goto('/');
    const homeRow = card(page, page.getByRole('region', { name: 'Upcoming journeys' }), ownJourneyId);
    await expect(homeRow.getByText('Needs a train picked')).toBeVisible();

    await request.delete(`/api/Journeys/${ownJourneyId}`, { headers: headers() });
    ownJourneyId = null;
  });
});
