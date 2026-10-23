import { test, expect, type BrowserContext, type Locator, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// Clickable cards (`components/ClickableCard.tsx`, docs/style-guide.md
// "Clickable cards"): every box the whole of which is clickable holds one
// real link or button around its title, stretched over the card. Against
// the seeded backend (e2e/fixtures/seed.sql).

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const GROUP_ID = process.env.E2E_GROUP_ID;

async function logIn(context: BrowserContext) {
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
}

/** The clickable card holding the link named `name`. */
function cardWithLink(page: Page, name: string): Locator {
  return page.locator('[data-clickable-card]').filter({ has: page.getByRole('link', { name, exact: true }) });
}

/** Tab through the page until `target` has focus, as a keyboard user would. */
async function tabTo(page: Page, target: Locator) {
  await page.locator('body').click({ position: { x: 1, y: 1 } });
  for (let i = 0; i < 80; i += 1) {
    await page.keyboard.press('Tab');
    if (await target.evaluate((el) => el === document.activeElement)) return;
  }
  throw new Error('Tab never reached the card');
}

/** The computed link colour, the focus ring's colour. */
async function anchorColour(page: Page): Promise<string> {
  return page.evaluate(() => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--mantine-color-anchor)';
    document.body.append(probe);
    const colour = getComputedStyle(probe).color;
    probe.remove();
    return colour;
  });
}

// The pages with clickable cards, and the cards' targets checked on each.
const PAGES = ['/', '/journeys', '/groups', '/status', '/transit', '/transit/metrolink'];

test.describe('clickable cards', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  test.beforeEach(async ({ context }) => {
    await logIn(context);
  });

  for (const path of [...PAGES, ...(GROUP_ID ? [`/groups/${GROUP_ID}`] : [])]) {
    test(`${path}: every clickable card has one real target, the pointer, and passes axe`, async ({ page }) => {
      await page.goto(path);
      const cards = page.locator('[data-clickable-card]');
      await expect(cards.first()).toBeVisible();
      // One snapshot of every card, so a list that re-renders (the page
      // refreshes itself) can't shift the cards between checks.
      const found = await cards.evaluateAll((all) =>
        all.map((card) => ({
          targets: [...card.querySelectorAll('[data-card-primary]')].map((el) => el.tagName),
          cursor: getComputedStyle(card).cursor,
        })),
      );
      for (const card of found) {
        expect(card.targets).toHaveLength(1);
        expect(['A', 'BUTTON', 'INPUT']).toContain(card.targets[0]);
        expect(card.cursor).toBe('pointer');
      }
      // Card markup only: the full-page sweep is e2e/accessibility.spec.ts.
      const results = await new AxeBuilder({ page }).include('[data-clickable-card]').analyze();
      expect(results.violations).toEqual([]);
    });
  }

  for (const colorScheme of ['light', 'dark'] as const) {
    test(`a card is keyboard-focusable with a visible focus ring round it, and Enter opens it (${colorScheme})`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme });
      await page.goto('/journeys');
      const link = page.getByRole('link', { name: 'Fixture door to door', exact: true });
      const card = cardWithLink(page, 'Fixture door to door');
      await tabTo(page, link);
      const outline = await card.evaluate((el) => {
        const style = getComputedStyle(el);
        return { style: style.outlineStyle, width: style.outlineWidth, colour: style.outlineColor };
      });
      expect(outline).toEqual({ style: 'solid', width: '2px', colour: await anchorColour(page) });
      // The ring is on the card, not the title.
      expect(await link.evaluate((el) => getComputedStyle(el).outlineStyle)).toBe('none');
      await page.keyboard.press('Enter');
      await expect(page).toHaveURL(/\/journeys\/\d+$/);
    });
  }

  for (const colorScheme of ['light', 'dark'] as const) {
    test(`at rest a clickable card differs from a static one (${colorScheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme });
      await page.goto('/journeys');
      const card = cardWithLink(page, 'Fixture door to door');
      const flat = page.locator('.mantine-Card-root[data-with-border]:not([data-clickable-card])').first();
      const look = (target: Locator) =>
        target.evaluate((el) => {
          const style = getComputedStyle(el);
          return `${style.borderTopColor} ${style.boxShadow}`;
        });
      await expect(flat).toBeVisible();
      expect(await look(card)).not.toBe(await look(flat));
    });
  }

  test('a click anywhere on a card opens it, and its link is named by the title alone', async ({ page }) => {
    await page.goto('/journeys');
    const link = page.getByRole('link', { name: 'Fixture door to door', exact: true });
    await expect(link).toHaveAccessibleName('Fixture door to door');
    const card = cardWithLink(page, 'Fixture door to door');
    // Its other text is in the card, not the link.
    await expect(card).toContainText('2 legs');
    const box = (await card.boundingBox())!;
    await card.click({ position: { x: box.width - 12, y: box.height - 8 } });
    await expect(page).toHaveURL(/\/journeys\/\d+$/);
  });

  test('a control inside a card works on its own and does not open the card', async ({ page }) => {
    await page.goto('/journeys');
    const card = page
      .locator('[data-clickable-card]')
      .filter({ has: page.getByRole('button', { name: /^More actions for / }) })
      .first();
    const menu = card.getByRole('button', { name: /^More actions for / });
    await menu.click();
    await expect(page.getByRole('menu')).toBeVisible();
    await expect(page).toHaveURL(/\/journeys$/);
    await page.keyboard.press('Escape');
    await expect(page.getByRole('menu')).toBeHidden();
  });

  test('hover shifts the background, and nothing animates under reduced motion', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/journeys');
    const card = cardWithLink(page, 'Fixture door to door');
    const background = () => card.evaluate((el) => getComputedStyle(el).backgroundColor);
    const resting = await background();
    expect(await card.evaluate((el) => getComputedStyle(el).transitionDuration)).toBe('0s');
    await card.hover();
    expect(await background()).not.toBe(resting);
    expect(await card.evaluate((el) => getComputedStyle(el).borderTopColor)).toBe(await anchorColour(page));
  });
});
