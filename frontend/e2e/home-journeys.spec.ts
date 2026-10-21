import { test, expect, type APIRequestContext, type BrowserContext } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// The logged-in home page's journey sections: "Current journey" for the
// journey under way, and "Upcoming journeys" for the ones still to come; a
// train tracked on its own is a one-leg journey in them, and there is no
// "Tracked trains" list. The page renders on the
// server from the api, so this runs against the seeded e2e stack
// (frontend/e2e/fixtures/seed.sql), logged in as the seed's owner, whose
// 2099 journey "Fixture door to door" is always upcoming.
//
// A journey under way needs times around the server's own now, which a
// fixed seed can't give, so the test makes one through the api (a walk and
// a Tube ride, which need no timetable) and deletes it afterwards.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const CURRENT_NAME = 'E2E journey under way';

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

const fromNow = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();

function untrackedLeg(mode: string, from: string, to: string, departIn: number, arriveIn: number) {
  return {
    mode: 'untracked',
    transitMode: mode,
    originName: from,
    destinationName: to,
    plannedDeparture: fromNow(departIn),
    plannedArrival: fromNow(arriveIn),
  };
}

/** A walk under way now, then a Tube ride: returns the journey's id.
 * Named per test, so a leftover from a failed run can't be mistaken for
 * this one. */
async function createJourneyUnderWay(request: APIRequestContext, name: string): Promise<number> {
  const headers = { cookie: `distant_signal_session=${SESSION_COOKIE}` };
  const created = await request.post('/api/Journeys', {
    headers,
    data: { customName: name, leg: untrackedLeg('walk', 'Islington', 'Angel', -10, 20) },
  });
  expect(created.ok(), await created.text()).toBe(true);
  const { journeyId } = (await created.json()) as { journeyId: number };
  const added = await request.post(`/api/Journeys/${journeyId}/legs`, {
    headers,
    data: { ...untrackedLeg('tube', 'Angel', 'Bank', 25, 45), lineName: 'Northern line' },
  });
  expect(added.ok(), await added.text()).toBe(true);
  return journeyId;
}

test.describe('home page journeys (logged in)', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');
  // One at a time: each test's journey would otherwise be under way during
  // the other's, and show as "Also in progress" there.
  test.describe.configure({ mode: 'serial' });

  let journeyId: number | null = null;

  test.beforeEach(async ({ context }) => {
    await logIn(context);
  });

  test.afterEach(async ({ request }) => {
    if (journeyId === null) return;
    await request.delete(`/api/Journeys/${journeyId}`, {
      headers: { cookie: `distant_signal_session=${SESSION_COOKIE}` },
    });
    journeyId = null;
  });

  test('upcoming journeys lead the page, with no Tracked trains list', async ({ page }) => {
    await page.goto('/');
    const upcoming = page.getByRole('region', { name: 'Upcoming journeys' });
    await expect(upcoming.getByRole('link', { name: /Fixture door to door/ })).toBeVisible();
    await expect(upcoming.getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/journeys');
    await expect(page.getByRole('heading', { name: 'Tracked trains' })).toHaveCount(0);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Home');
  });

  for (const colorScheme of ['light', 'dark'] as const) {
    test(`a journey under way is the current journey (${colorScheme})`, async ({ page, request }) => {
      await page.emulateMedia({ colorScheme });
      const name = `${CURRENT_NAME} (${colorScheme})`;
      journeyId = await createJourneyUnderWay(request, name);
      await page.goto('/');

      const current = page.getByRole('region', { name: 'Current journey' });
      const card = current.getByRole('link', { name: new RegExp(`^${name.replace(/[()]/g, '\\$&')}`) });
      await expect(card).toBeVisible();
      await expect(card).toContainText('Now, leg 1 of 2');
      await expect(card).toContainText('Walk, Islington to Angel');
      await expect(card).toContainText('Next change');
      // It sits above the upcoming list.
      const currentBox = await current.boundingBox();
      const upcomingBox = await page.getByRole('region', { name: 'Upcoming journeys' }).boundingBox();
      expect(currentBox && upcomingBox && currentBox.y < upcomingBox.y).toBe(true);

      const results = await new AxeBuilder({ page }).include('main').analyze();
      expect(results.violations).toEqual([]);

      // Keyboard: the card is one link to the journey page.
      await card.focus();
      await page.keyboard.press('Enter');
      await expect(page).toHaveURL(new RegExp(`/journeys/${journeyId}$`));
    });
  }
});
