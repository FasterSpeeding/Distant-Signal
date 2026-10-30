import { test, expect } from '@playwright/test';

// A journey leg that boards and alights partway along its train
// (components/JourneyProgress.tsx's `legRange`, JourneyTimeline.tsx's
// collapsed sections). The seed's journey 900002 (e2e/fixtures/seed.sql)
// rides W90901 from Fixture Brook (ZLB) to Fixture Dale (ZLD); the train
// runs Fixture Ashby to Fixture End and has only left Fixture Ashby. Read
// through its unlisted share link, so no login is needed.

const SHARE_TOKEN = 'fixture-leg-progress-token-12345';

test.describe('journey leg progress and calling points', () => {
  test("shows only the leg's stops, with the rest of the train collapsed", async ({ page }) => {
    await page.goto(`/journeys/shared/${SHARE_TOKEN}`);
    const main = page.locator('#main-content');

    // The progress line: Fixture Brook to Fixture Dale only, not started.
    const progress = main.getByRole('group', { name: /Journey progress/ });
    await expect(progress.locator('[data-journey-node]')).toHaveCount(3);
    await expect(progress).toContainText('Fixture Brook');
    await expect(progress).toContainText('Fixture Dale');
    await expect(progress).not.toContainText('Fixture Ashby');
    await expect(progress).not.toContainText('Fixture End');
    await expect(
      main.getByText('Not started. The train is on its way to Fixture Brook, last reported at Fixture Ashby.'),
    ).toBeVisible();
    // The train calls on both sides of the leg: a stub and a note at each
    // end, and the same in words for a screen reader.
    await expect(progress.locator('[data-leg-continues="before"]')).toContainText('Fixture Brook');
    await expect(progress.locator('[data-leg-continues="after"]')).toContainText('Fixture Dale');
    await expect(progress.locator('[data-leg-continues-note]')).toHaveText(['+1 stop', '+1 stop']);
    await expect(progress).toHaveAccessibleDescription(
      'The train starts at Fixture Ashby, 1 stop before you board, and continues to Fixture End, 1 stop after you get off.',
    );

    // The calling points: the leg's three stops, then the two toggles.
    const table = main.getByRole('table', { name: 'Journey timeline' });
    await expect(table.getByText('Fixture Cross')).toBeVisible();
    await expect(table.getByText('Fixture Ashby', { exact: true })).toHaveCount(0);
    const earlier = table.getByRole('button', { name: 'Earlier stops from Fixture Ashby (1)' });
    const later = table.getByRole('button', { name: 'Later stops to Fixture End (1)' });
    await expect(earlier).toHaveAttribute('aria-expanded', 'false');
    await expect(later).toHaveAttribute('aria-expanded', 'false');
    // Where the train is, without opening the section.
    await expect(table.getByText('Train last reported at Fixture Ashby')).toBeVisible();

    await earlier.click();
    await expect(earlier).toHaveAttribute('aria-expanded', 'true');
    await expect(table.getByText('Fixture Ashby', { exact: true })).toBeVisible();
    await later.click();
    await expect(table.getByText('Fixture End', { exact: true })).toBeVisible();
    await later.click();
    await expect(later).toHaveAttribute('aria-expanded', 'false');
    await expect(table.getByText('Fixture End', { exact: true })).toHaveCount(0);
  });

  test('keeps the whole train on its own page', async ({ page }) => {
    await page.goto('/train/W90901/2099-02-02');
    const main = page.locator('#main-content');
    const progress = main.getByRole('group', { name: /Journey progress/ });
    await expect(progress.locator('[data-journey-node]')).toHaveCount(5);
    await expect(progress.locator('[data-leg-continues]')).toHaveCount(0);
    await expect(progress).not.toHaveAttribute('aria-describedby');
    const table = main.getByRole('table', { name: 'Journey timeline' });
    await expect(table.getByText('Fixture End')).toBeVisible();
    await expect(table.getByRole('button', { name: /stops (from|to)/ })).toHaveCount(0);
  });
});
