import { test, expect, type Page } from '@playwright/test';

// The train page's delay and platform badges at phone widths
// (components/JourneyTimeline.tsx, JourneyProgress.tsx, DelayBadge.tsx,
// PlatformBadge.tsx). Mantine's Badge cuts its label short with an
// ellipsis, and in the calling-point table that left "4 …" and
// "Platform …" at every phone width. The seed's train (e2e/fixtures/
// seed.sql) has a 4 minute delay at York and a changed origin platform,
// "Platform 9 (changed from 6)", the longest badge there is.

const TRAIN_UID = process.env.E2E_TRAIN_UID ?? 'W12345';
const TRAIN_DATE = process.env.E2E_TRAIN_DATE ?? '2026-09-17';
const PATH = `/train/${TRAIN_UID}/${TRAIN_DATE}`;

const BADGES = '[data-delay-badge], [data-platform-badge]';

interface BadgeFit {
  text: string;
  root: [scroll: number, client: number];
  label: [scroll: number, client: number];
}

/** Every displayed delay and platform badge, with the root's and the
 * label's `scrollWidth` against `clientWidth`: a badge whose text is cut
 * short has a scroll width wider than its box. Badges under `display:
 * none` (the Delay column below `sm`) are left out. */
async function displayedBadges(page: Page): Promise<BadgeFit[]> {
  return page.locator(BADGES).evaluateAll((badges) =>
    badges
      .filter((badge) => badge.getClientRects().length > 0)
      .map((badge) => {
        const label = badge.querySelector('.mantine-Badge-label') ?? badge;
        return {
          text: badge.textContent,
          root: [badge.scrollWidth, badge.clientWidth] as [number, number],
          label: [label.scrollWidth, label.clientWidth] as [number, number],
        };
      }),
  );
}

async function pageScrollsSideways(page: Page): Promise<boolean> {
  return page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
}

for (const width of [320, 360, 390]) {
  test.describe(`train page badges at ${width}px`, () => {
    test.use({ viewport: { width, height: 844 } });

    test('no delay or platform badge is cut short, and the page never scrolls sideways', async ({ page }) => {
      await page.goto(PATH);
      const table = page.getByRole('table', { name: 'Journey timeline' });
      await expect(table.getByRole('row')).toHaveCount(4);

      const badges = await displayedBadges(page);
      // The summary's delay, the progress line's platform, and the table's
      // delay and platform: a vacuous pass is not a pass.
      expect(badges.length).toBeGreaterThanOrEqual(4);
      for (const badge of badges) {
        expect(badge.label[0], `label of "${badge.text}"`).toBeLessThanOrEqual(badge.label[1]);
        expect(badge.root[0], `badge "${badge.text}"`).toBeLessThanOrEqual(badge.root[1]);
      }
      expect(await pageScrollsSideways(page)).toBe(false);

      // The table's short forms, with the full words still what a screen
      // reader gets.
      await expect(table.getByText('9 (was 6)')).toBeVisible();
      // The delay is in the DOM twice, in the hidden Delay column and
      // under the Actual time; only the second is displayed.
      await expect(table.getByText('+4 min').filter({ visible: true })).toHaveCount(1);
      await expect(table.getByRole('columnheader', { name: 'Platform' })).toBeVisible();
      await expect(table.getByRole('columnheader', { name: 'Delay' })).toBeHidden();
    });

    // 360 and up: the whole table fits, so no badge sits outside the table's
    // own scroll box either. At 320 a changed platform plus "London" can
    // still need a short sideways swipe inside the table; the page itself
    // never scrolls (above).
    if (width >= 360) {
      test('the calling-point table fits without scrolling', async ({ page }) => {
        await page.goto(PATH);
        const viewport = page.locator('.journeyTimelineScroll .mantine-ScrollArea-viewport');
        await expect(page.getByRole('table', { name: 'Journey timeline' }).getByRole('row')).toHaveCount(4);
        const [scroll, client] = await viewport.evaluate((el) => [el.scrollWidth, el.clientWidth]);
        expect(scroll).toBeLessThanOrEqual(client);
        const box = (await viewport.boundingBox())!;
        for (const badge of await page.locator(`.journeyTimeline :is(${BADGES})`).all()) {
          if (!(await badge.isVisible())) continue;
          const badgeBox = (await badge.boundingBox())!;
          expect(badgeBox.x).toBeGreaterThanOrEqual(box.x);
          expect(badgeBox.x + badgeBox.width).toBeLessThanOrEqual(box.x + box.width + 0.5);
        }
      });
    }
  });
}

test.describe('train page badges on a desktop', () => {
  test.use({ viewport: { width: 1366, height: 900 } });

  test('keeps the full words and the Delay column', async ({ page }) => {
    await page.goto(PATH);
    const table = page.getByRole('table', { name: 'Journey timeline' });
    await expect(table.getByText('Platform 9 (changed from 6)')).toBeVisible();
    await expect(table.getByText('4 min late').filter({ visible: true })).toHaveCount(1);
    await expect(table.getByText('+4 min').filter({ visible: true })).toHaveCount(0);
    await expect(table.getByRole('columnheader', { name: 'Delay' })).toBeVisible();
    for (const badge of await displayedBadges(page)) {
      expect(badge.label[0], `label of "${badge.text}"`).toBeLessThanOrEqual(badge.label[1]);
    }
  });
});
