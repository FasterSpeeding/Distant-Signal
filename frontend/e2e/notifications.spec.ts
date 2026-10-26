import { test, expect, type BrowserContext, type Page } from '@playwright/test';

// /account/notifications (app/account/notifications/page.tsx, DS-153): every
// notification setting on one page, against the real api. Logged out it is
// the one log in prompt; logged in, each change saves on its own and is
// still there after a reload. Each test puts back what it changed, so the
// seed user's settings are the defaults for every other spec.
const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const NOTIFICATIONS = '/account/notifications';

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

/** Puts the seed user's settings back to the defaults. */
async function resetSettings(page: Page): Promise<void> {
  const response = await page.request.put('/api/preferences/notifications', {
    data: {
      journeySuggestions: true,
      lineStatus: true,
      trainDelay: true,
      trainCancel: true,
      predictedStart: true,
      stationSkip: true,
      templateAttention: true,
      delayThresholdMinutes: null,
      quietHours: null,
    },
    headers: { Origin: new URL(page.url()).origin },
  });
  expect(response.ok()).toBe(true);
}

test.describe('Notifications, logged out', () => {
  test('asks to log in', async ({ page }) => {
    const response = await page.goto(NOTIFICATIONS);
    expect(response?.status()).toBe(200);
    await expect(page.getByRole('heading', { level: 1, name: 'Notifications' })).toBeVisible();
    await expect(
      page.getByRole('region', { name: 'Account settings' }).getByRole('link', {
        name: 'Log in to manage notifications and more',
      }),
    ).toBeVisible();
    await expect(page.getByRole('region', { name: 'What to send' })).toHaveCount(0);
  });
});

test.describe('Notifications, logged in', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value');

  test.beforeEach(async ({ context }) => {
    await logIn(context);
  });

  test('is linked from Preferences and lists every section', async ({ page }) => {
    await page.goto('/account/preferences');
    await page.getByRole('link', { name: 'Choose which notifications you get' }).click();
    await expect(page).toHaveURL(new RegExp(`${NOTIFICATIONS}$`));
    for (const name of ['This device', 'What to send', 'Quiet hours', 'Lines', 'Devices']) {
      await expect(page.getByRole('region', { name })).toBeVisible();
    }
    // Push is not on for any of the seed user's devices.
    await expect(page.getByText("Notifications aren't on for any device yet.")).toBeVisible();
  });

  test('saves a switch, the delay threshold and quiet hours', async ({ page }) => {
    await page.goto(NOTIFICATIONS);
    try {
      const send = page.getByRole('region', { name: 'What to send' });
      const delays = send.getByRole('switch', { name: /^Train delays/ });
      await expect(delays).toBeEnabled();
      await expect(delays).toBeChecked();

      // The switch's input is visually hidden under its track: force the click.
      const skips = send.getByRole('switch', { name: /^Skipped stops/ });
      await expect(async () => {
        await skips.setChecked(false, { force: true, timeout: 2_000 });
        await expect(skips).not.toBeChecked({ timeout: 2_000 });
      }).toPass({ timeout: 20_000 });
      await send.getByRole('combobox', { name: 'Delay threshold' }).click();
      await page.getByRole('option', { name: '30 min' }).click();
      await expect(send.getByRole('combobox', { name: 'Delay threshold' })).toHaveValue('30 min');

      const quiet = page.getByRole('region', { name: 'Quiet hours' });
      await quiet.getByRole('switch', { name: 'Quiet hours' }).setChecked(true, { force: true });
      await expect(quiet.getByRole('switch', { name: 'Quiet hours' })).toBeChecked();

      await page.reload();
      await expect(send.getByRole('switch', { name: /^Skipped stops/ })).not.toBeChecked();
      await expect(send.getByRole('switch', { name: /^Train delays/ })).toBeChecked();
      await expect(send.getByRole('combobox', { name: 'Delay threshold' })).toHaveValue('30 min');
      await expect(quiet.getByRole('switch', { name: 'Quiet hours' })).toBeChecked();
      await expect(quiet.getByLabel('From')).toHaveValue('22:00');
      await expect(quiet.getByLabel('Until')).toHaveValue('07:00');
    } finally {
      await resetSettings(page);
    }
  });
});
