import { test, expect } from '@playwright/test';

// The one journey-naming rule (lib/journeyName.ts, docs/style-guide.md
// "Journey names") on /track/mine, against the seeded backend: the page is
// server-rendered, so its journey list can't be stood in for with
// `page.route`. The seed (e2e/fixtures/seed.sql) gives fixture-owner a
// PLANNED three-leg journey, EUS to MKC to PBO to KGX, with no train on any
// leg. Its row used to read "EUS to MKC": the first leg's destination.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const JOURNEY_ID = process.env.E2E_PLANNED_JOURNEY_ID ?? '900001';

test.describe('journey names on /track/mine', () => {
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

  test('a planned three-leg journey shows its real final destination', async ({ page }) => {
    await page.goto('/track/mine');

    const row = page.getByRole('link', {
      name: 'London Euston (EUS) to London Kings Cross (KGX), 17 Sep 2026',
    });
    await expect(row).toHaveAttribute('href', `/journeys/${JOURNEY_ID}`);
    // The row's own card: its leg count, and nothing naming the first
    // change as where the journey ends.
    const card = page.locator('.mantine-Card-root', { has: row });
    await expect(card.getByText('3 legs')).toBeVisible();
    await expect(card.getByText(/to MKC/)).toHaveCount(0);
  });

  test('the journey page heads it with the same name', async ({ page }) => {
    await page.goto(`/journeys/${JOURNEY_ID}`);

    await expect(
      page.getByRole('heading', {
        level: 1,
        name: 'London Euston (EUS) to London Kings Cross (KGX), 17 Sep 2026',
      }),
    ).toBeVisible();
  });
});
