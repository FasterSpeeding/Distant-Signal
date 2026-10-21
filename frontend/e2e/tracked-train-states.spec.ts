import { test, expect, type BrowserContext } from '@playwright/test';

// Tracked trains with no timetable match, and timetabled trains whose first
// Network Rail report isn't due yet (spec
// docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
// §3 and §4). Both pages render on the server from the api, so this runs
// against the seeded e2e stack (frontend/e2e/fixtures/seed.sql), logged in
// as the seed's owner:
//
//  - E2E_TRACKING_ID: a station and time pin that never matched a
//    timetable and has no report: "Not matched to a timetable".
//  - W77777 on 2099-01-05: tracked by UID, timetabled, no report, its
//    first report due 45 minutes before a 07:10 departure in 2099:
//    "Planned", whatever today is.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const TRACKING_ID = process.env.E2E_TRACKING_ID;
const PLANNED_UID = 'W77777';

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

test.describe('tracked train states', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  test.beforeEach(async ({ context }) => {
    await logIn(context);
  });

  test('a train with no timetable match says it cannot be tracked', async ({ page }) => {
    test.skip(!TRACKING_ID, "set E2E_TRACKING_ID to the seed's unresolved pin");
    await page.goto(`/train/by-id/${TRACKING_ID}`);
    const main = page.locator('#main-content');
    await expect(main.getByText('Not matched to a timetable').first()).toBeVisible();
    await expect(
      main
        .getByText(/We can't find a timetabled train from .* so we can't track it\. Check the station, time and date\./)
        .first(),
    ).toBeVisible();
    await expect(page.getByText('Waiting to hear from Network Rail')).toHaveCount(0);
  });

  test('a timetabled train shows as planned before its first report is due', async ({ page, request }) => {
    const mine = await request.get('/api/Train/mine', {
      headers: { cookie: `distant_signal_session=${SESSION_COOKIE}` },
    });
    expect(mine.ok()).toBe(true);
    const trains = (await mine.json()) as { id: number; trainUid: string | null; trackingPhase?: string | null }[];
    const planned = trains.find((train) => train.trainUid === PLANNED_UID);
    expect(planned, `the seed tracks ${PLANNED_UID}`).toBeDefined();
    expect(planned?.trackingPhase).toBe('planned');

    await page.goto(`/train/by-id/${planned?.id}`);
    const main = page.locator('#main-content');
    await expect(main.getByText('Planned', { exact: true }).first()).toBeVisible();
    // 45 minutes before the 07:10 departure (GMT in January).
    await expect(main.getByText(/We expect Network Rail's first report by 06:25\./).first()).toBeVisible();
    await expect(page.getByText('Waiting to hear from Network Rail')).toHaveCount(0);
    await expect(page.getByText('Awaiting first report')).toHaveCount(0);
  });

  test('the track page badges both states', async ({ page }) => {
    await page.goto('/journeys');
    await expect(page.getByText('Planned', { exact: true }).first()).toBeVisible();
    await expect(page.getByText('Not in the timetable').first()).toBeVisible();
  });
});
