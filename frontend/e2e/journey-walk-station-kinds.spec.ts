import { test, expect } from '@playwright/test';

// DS-239: a walk between a National Rail station and its Tube station,
// saved before the planner named each end ("Zed Bee to Zed Bee"), says
// which end is which on the journey page, from the legs either side: the
// train before it arrives at the National Rail station, the Tube ride
// after it leaves from the Tube station. Runs against the seeded e2e
// stack, logged in as the seed's owner, on a journey it creates for itself
// on tomorrow's fixture train (seed.sql: E2EMJ1, ZMA 09:00 to ZMB 09:40),
// and removes.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

test.describe('walk station kinds on the journey page', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  test('says which end of an alike-named walk is National Rail and which the Tube', async ({
    page,
    context,
    request,
  }) => {
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
    const headers = { cookie: `distant_signal_session=${SESSION_COOKIE}`, origin: base.origin };
    const tomorrow = new Intl.DateTimeFormat('en-CA', { timeZone: 'Europe/London' }).format(
      new Date(Date.now() + 24 * 3600 * 1000),
    );
    const untracked = (transitMode: string, originName: string, destinationName: string, from: string, to: string) => ({
      kind: 'untracked',
      transitMode,
      originName,
      destinationName,
      plannedDeparture: `${tomorrow}T${from}:00Z`,
      plannedArrival: `${tomorrow}T${to}:00Z`,
    });
    const created = await request.post('/api/Journeys/itinerary', {
      headers: { ...headers, 'content-type': 'application/json' },
      data: {
        customName: 'E2E walk station kinds',
        legs: [
          { trainUid: 'E2EMJ1', serviceDate: tomorrow, originCrs: 'ZMA', destinationCrs: 'ZMB' },
          untracked('walk', 'Zed Bee', 'Zed Bee', '09:45', '09:50'),
          untracked('tube', 'Zed Bee', 'Zed Cee', '09:50', '10:05'),
        ],
      },
    });
    expect(created.ok(), await created.text()).toBe(true);
    const { journeyId } = (await created.json()) as { journeyId: number };

    try {
      await page.goto(`/journeys/${journeyId}`);
      const walk = page.locator('[data-untracked-leg]').first();
      await expect(walk).toContainText('Walk');
      await expect(walk.getByText('Zed Bee (National Rail) to Zed Bee (Tube)', { exact: true })).toBeVisible();
      // The Tube ride's names differ: as saved.
      await expect(page.locator('[data-untracked-leg]').nth(1).getByText('Zed Bee to Zed Cee')).toBeVisible();
    } finally {
      await request.delete(`/api/Journeys/${journeyId}`, { headers });
    }
  });
});
