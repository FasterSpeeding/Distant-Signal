import { test, expect, type Page } from '@playwright/test';

// Task 1.1 (docs/superpowers/sdd/2026-09-17-ux-fixes-phase1-crosscutting):
// `<main>` (app/layout.tsx's `<Container component="main" size="lg" px={0}
// style={{ flex: 1 }}>`) used to shrink-wrap to its own content's width
// instead of matching the nav's Container immediately above it (`<Container
// size="lg" px={0}>` at layout.tsx:285 -- byte-for-byte the same `size`/
// `px`), so a page's content edge drifted away from the nav's on every
// route whose content didn't happen to measure exactly 1140px wide (the
// `lg` breakpoint). Confirmed against the installed
// `node_modules/@mantine/core/styles/Container.css`: the
// `[data-strategy='block']` rule this renders under sets only `max-width`
// + `margin-inline: auto`, no `width`. A flex item with `width: auto`
// normally stretches to fill the cross axis (`body`'s `display: flex;
// flex-direction: column` defaults `align-items` to `normal`, which
// computes to `stretch`), but per the Flexbox spec that stretch is skipped
// whenever the item's cross-axis margins are auto -- exactly what
// `margin-inline: auto` produces here -- falling back to shrink-to-fit
// sizing instead (verified against a live dev server: pre-fix, `<main>`
// measured a different width per route -- 1008px on `/`, 824px on
// `/train/[uid]/[date]`, 225px on `/groups/[id]` at 1440px viewport width
// -- tracking each page's own content rather than the nav's 1140px). This
// is also why `align-self: stretch` alone would NOT have fixed it: the
// same auto-margin carve-out suppresses an explicit `stretch` too. The fix
// (layout.tsx) adds `w="100%"` to give the Container an explicit,
// non-auto width, which `max-width: 1140px` then clamps identically to the
// nav's, with `margin-inline: auto` centering the clamped box the same
// way on both.
//
// This spec is the regression net: it asserts `<main>`'s bounding box
// matches the nav Container's exactly, so a future edit that drops
// `w="100%"` (or otherwise reintroduces shrink-wrap) fails a real,
// rendered-DOM assertion rather than drifting silently.
//
// Route selection and the `E2E_TRAIN_UID`/`E2E_TRAIN_DATE`/`E2E_GROUP_ID`
// skip pattern mirror e2e/accessibility.spec.ts's own (see that file's
// top-of-file comment for the full rationale -- a train UID/date pair has
// no stable fixture to default to, and a group id is account-specific).
const TRAIN_UID = process.env.E2E_TRAIN_UID;
const TRAIN_DATE = process.env.E2E_TRAIN_DATE;
const GROUP_ID = process.env.E2E_GROUP_ID;

async function expectMainMatchesNavWidth(page: Page) {
  const nav = page.locator('nav > .mantine-Container-root');
  const main = page.locator('main.mantine-Container-root');
  await expect(nav).toBeVisible();
  await expect(main).toBeVisible();

  const navBox = await nav.boundingBox();
  const mainBox = await main.boundingBox();
  if (!navBox || !mainBox) {
    throw new Error('Could not measure the nav Container or <main> -- one of them did not render.');
  }

  // Same box, not just same width: a Container that stretched but lost its
  // `margin-inline: auto` centering would still fail here, since its left
  // edge (`x`) would no longer line up with the nav's.
  expect(mainBox.width, 'main width should equal the nav Container width').toBeCloseTo(
    navBox.width,
    0
  );
  expect(mainBox.x, 'main left edge should equal the nav Container left edge').toBeCloseTo(
    navBox.x,
    0
  );
}

test.describe('<main> width matches the nav Container (desktop 1440x900)', () => {
  test.use({ viewport: { width: 1440, height: 900 } });

  test('/', async ({ page }) => {
    await page.goto('/');
    await expectMainMatchesNavWidth(page);
  });

  test('/train/[uid]/[date]', async ({ page }) => {
    test.skip(
      !TRAIN_UID || !TRAIN_DATE,
      'set E2E_TRAIN_UID and E2E_TRAIN_DATE (YYYY-MM-DD) to a schedule that is live right now'
    );
    await page.goto(`/train/${TRAIN_UID}/${TRAIN_DATE}`);
    await expectMainMatchesNavWidth(page);
  });

  test('/groups/[id]', async ({ page }) => {
    test.skip(!GROUP_ID, 'set E2E_GROUP_ID to a group id (the page renders a content column either way -- logged in or not)');
    await page.goto(`/groups/${GROUP_ID}`);
    await expectMainMatchesNavWidth(page);
  });
});
