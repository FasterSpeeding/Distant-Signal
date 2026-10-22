import { test, expect } from '@playwright/test';

// Mid-journey changes (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md
// §8): "I missed this train", taking another train from the suggestions
// sheet, the booked plan kept under "Earlier plans", and Undo. Runs against
// the seeded e2e stack, logged in as the seed's owner, on a journey it
// creates for itself on tomorrow's fixture trains (seed.sql: E2EMJ1 09:00,
// E2EMJ2 09:30, E2EMJ3 10:00, ZMA to ZMB).

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

test.describe('mid-journey changes', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  test('marks a train missed, takes the next one, and undoes it', async ({ page, context, request }) => {
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
    const londonDate = (offsetDays: number) =>
      new Intl.DateTimeFormat('en-CA', { timeZone: 'Europe/London' }).format(
        new Date(Date.now() + offsetDays * 24 * 3600 * 1000),
      );
    const tomorrow = londonDate(1);
    const created = await request.post('/api/Journeys/itinerary', {
      headers: {
        cookie: `distant_signal_session=${SESSION_COOKIE}`,
        origin: base.origin,
        'content-type': 'application/json',
      },
      data: {
        customName: 'E2E missed train',
        legs: [{ trainUid: 'E2EMJ1', serviceDate: tomorrow, originCrs: 'ZMA', destinationCrs: 'ZMB' }],
      },
    });
    expect(created.ok(), await created.text()).toBe(true);
    const { journeyId } = (await created.json()) as { journeyId: number };

    await page.goto(`/journeys/${journeyId}`);
    const main = page.locator('#main-content');
    await main.getByRole('button', { name: 'I missed this train' }).click();

    const sheet = page.getByRole('dialog');
    const take = sheet.getByRole('button', { name: 'Take this one' }).first();
    await expect(take).toBeVisible();
    await expect(sheet.getByText('Your booked plan stays saved for Delay Repay.')).toBeVisible();
    await take.click();

    await expect(page.getByText('Journey updated. Booked plan saved.')).toBeVisible();
    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(page.getByText('Journey updated. Booked plan saved.')).toBeHidden();

    // The plans so far: booked, the change, and the Undo back to it.
    await page.goto(`/journeys/${journeyId}`);
    await expect(main.getByText(/Earlier plans/)).toBeVisible();

    await request.delete(`/api/Journeys/${journeyId}`, {
      headers: { cookie: `distant_signal_session=${SESSION_COOKIE}`, origin: base.origin },
    });
  });
});
