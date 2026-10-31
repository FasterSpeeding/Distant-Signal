import { test, expect } from '@playwright/test';

// A saved journey's operator filter (operator filtering spec 2026-10-31
// §6.9) on its page, against the seeded backend: the page is
// server-rendered, so the journey can't be stood in for with `page.route`.
// The test gives the seed's planned journey (fixture-owner's 900001) a
// filter through `PUT /Journeys/{id}/operator-filter`, and always clears it
// again, so the other specs see the journey as seeded. A group entry is
// used because only ATOC codes named directly are checked against the
// timetable, which the seed's journey dates don't run.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const JOURNEY_ID = process.env.E2E_PLANNED_JOURNEY_ID ?? '900001';

test.describe('a journey keeps its operator filter', () => {
  test.skip(!SESSION_COOKIE, 'E2E_SESSION_COOKIE is unset');

  test.beforeEach(async ({ context }) => {
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
  });

  test('the page names the filter, and Remove clears it', async ({ page }) => {
    const url = `/api/Journeys/${JOURNEY_ID}/operator-filter`;
    try {
      const set = await page.request.put(url, {
        data: { operatorFilter: { mode: 'only', entries: ['group:GTR'] } },
      });
      expect(set.status()).toBe(204);

      await page.goto(`/journeys/${JOURNEY_ID}`);
      const line = page.locator('[data-journey-operator-filter]');
      await expect(line.getByText('Planned using only GTR. Suggestions after a disruption follow it.')).toBeVisible();

      await line.getByRole('button', { name: 'Remove' }).click();
      await expect(page.locator('[data-journey-operator-filter]')).toHaveCount(0);
    } finally {
      await page.request.put(url, { data: { operatorFilter: null } });
    }
  });
});
