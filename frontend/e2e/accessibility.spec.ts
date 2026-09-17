import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// A repeatable axe-core sweep over EVERY route in `app/`, in both colour
// schemes, logged out and logged in, and across the interactive sub-states
// (tabs, accordions, modals, open comboboxes) whose DOM does not exist on
// first paint.
//
// Like every other spec in this directory (see e2e/chat.spec.ts's own
// comment), this drives the REAL app through `playwright.config.ts`'s
// `webServer` (a real `next dev`) or a real deployed target
// (`E2E_BASE_URL`) -- it does not stand up its own backend, so every route
// here needs a real, reachable `api` service to render anything beyond a
// skeleton/error state.
//
// ---------------------------------------------------------------------------
// Why this is no longer scoped to five rules
// ---------------------------------------------------------------------------
// This file used to run `.withRules(['color-contrast', 'landmark-one-main',
// 'region', 'heading-order', 'page-has-heading-one'])`, with the reason
// stated in-file as: "The five rule IDs this plan's tasks fix. Not axe's
// full default ruleset -- a broader failure (e.g. a rule this plan never
// touched) shouldn't fail this spec and get miscategorized as a regression
// in this work."
//
// That reasoning was about ONE plan's blast radius
// (docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md, Step
// 6), not a judgement that the other rules don't apply here -- the audit
// that plan came from ran axe's full default ruleset, and the same file's
// own comment went on to NAME the rules the narrow list was missing
// (`scrollable-region-focusable`, `nested-interactive`, `aria-hidden-focus`,
// `button-name`). With that plan long merged, the constraint now only hides
// defects, so it is lifted: this runs the full default ruleset, and every
// rule that fires against this app today has been triaged and either fixed
// or explicitly waived in `WAIVED_RULES` below, with the evidence.
//
// There is no documented per-rule exclusion anywhere in
// docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md
// to carry forward; its "Explicitly out of scope" list is about things
// tooling cannot check at all (real-AT testing, full keyboard-trap sweeps,
// manual WCAG 2.2 AA), not about rules to suppress. Two things from it DO
// carry forward and are now discharged rather than dropped: it flagged
// `TrackedTrainStatusBadge`'s contrast as never measured (the test account
// had no tracked trains) and dark mode as never audited at all. Both are
// covered below, and both turned out to be real failures -- see
// app/globals.css's dark-scheme block.
//
// The deterministic palette assertions in `app/globals.test.ts` remain the
// PRIMARY net for contrast (they can check colour pairings that no live
// page happens to render this minute); this is the secondary, real-DOM one.
//
// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------
// Anonymous, always-on routes default to identifiers that exist on this
// project's own deployment; override them for a different `E2E_BASE_URL`.
const REAL_LINE_ID = process.env.E2E_REAL_LINE_ID ?? 'gwr-main-line';
const REAL_INCIDENT_ID = process.env.E2E_REAL_INCIDENT_ID ?? 'E879EB6C791C470AB6C2A7458AE68C3B';
const REAL_STATION_CRS = process.env.E2E_REAL_STATION_CRS ?? 'PAD';

// Everything below has NO default on purpose, and the tests that need one
// `test.skip()` themselves when it is unset. Two different reasons:
//
//  - `E2E_SESSION_COOKIE` and the per-user ids (`E2E_GROUP_ID`, ...) are
//    account-specific. There is no shareable "real" value the way there is
//    for a public line or station, and hardcoding one would make the
//    logged-in half of this suite fail on every deployment but one.
//    `E2E_SESSION_COOKIE` is the RAW `distant_signal_session` cookie value
//    (the `sessions` table stores its SHA-256 hex -- see
//    crates/api/src/auth.rs `hash_session_token`).
//
//  - `E2E_TRAIN_UID`/`E2E_TRAIN_DATE` carry forward, unchanged, the reason
//    the previous version of this file gave for omitting
//    `/train/[uid]/[date]` (and so `JourneyProgress`) altogether: a
//    `train_uid`/date pair is only resolvable for as long as that schedule
//    is live in the feed, so there is no stable fixture to hardcode or
//    default an env var to -- a route added with a default would start
//    failing on a date nobody changed anything on. The route is no longer
//    skipped outright, because "no stable default" is an argument against a
//    DEFAULT, not against covering the route when a caller supplies a live
//    pair. When it is not supplied, the component's own invariants (the
//    diagram's `role="group"` + per-state `aria-label`, every focusable
//    tooltip trigger carrying a real role and name, nothing focusable left
//    inside an ARIA-removed subtree) are still asserted deterministically
//    in `components/JourneyProgress.test.tsx`'s "keyboard reachability"
//    block, exactly as before.
const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;
const GROUP_ID = process.env.E2E_GROUP_ID;
const GROUP_INVITE_TOKEN = process.env.E2E_GROUP_INVITE_TOKEN;
const CUSTOM_LINE_ID = process.env.E2E_CUSTOM_LINE_ID;
// A tracked-train subscription id whose `resolution_status` is NOT
// 'resolved': `/train/by-id/[trackingId]` `redirect()`s a resolved one away
// to `/train/[uid]/[date]`, so a resolved id would silently audit the wrong
// page.
const TRACKING_ID = process.env.E2E_TRACKING_ID;
const TRAIN_UID = process.env.E2E_TRAIN_UID;
const TRAIN_DATE = process.env.E2E_TRAIN_DATE;

/**
 * Rules switched off, and only for the one assertion that needs it. Nothing
 * here is a blanket suppression: `waived` is passed per-call, so a rule
 * waived for an open combobox is still live on all ~60 other assertions.
 *
 * `region` / `scrollable-region-focusable` on an open Mantine combobox
 * dropdown. Both fire on the same element -- the `ScrollArea` viewport
 * inside `<div data-mantine-shared-portal-node>`, which Mantine renders as
 * a direct child of `<body>`, outside `<main>`.
 *
 *  - `scrollable-region-focusable` wants a scrollable container to be
 *    focusable or to contain focusable content. This one is neither, and
 *    correctly so: it is a `role="listbox"` driven by the standard ARIA
 *    combobox pattern, where focus stays on the `role="combobox"` input and
 *    the active option is tracked by `aria-activedescendant`. Options carry
 *    no `tabindex` BY DESIGN -- making them focusable would break the
 *    pattern. Verified empirically against this app rather than assumed:
 *    the `combobox keyboard reachability` test below drives the list with
 *    arrow keys and asserts both that `aria-activedescendant` moves and
 *    that the supposedly-unreachable region actually scrolls.
 *  - `region` (a best-practice rule, not WCAG) wants all page content
 *    inside a landmark. A transient popover owned by an in-landmark control
 *    via `aria-controls` is not loose page content, and the portal target
 *    is Mantine's, not this app's -- the only lever would be
 *    `withinPortal={false}` on every Combobox, which reintroduces the
 *    overflow-clipping this app hit before (see
 *    `components/TrackTrainForm.tsx`'s "Deliberately NOT wrapped in a
 *    ScrollArea" comments for the same class of bug).
 */
const COMBOBOX_PORTAL_WAIVERS = ['region', 'scrollable-region-focusable'];

async function expectNoViolations(page: Page, waived: string[] = []) {
  const builder = new AxeBuilder({ page });
  const results = await (waived.length ? builder.disableRules(waived) : builder).analyze();
  expect(
    results.violations,
    JSON.stringify(
      results.violations.map((v) => ({
        id: v.id,
        impact: v.impact,
        help: v.help,
        nodes: v.nodes.map((n) => ({ target: n.target, html: n.html, why: n.failureSummary })),
      })),
      null,
      2,
    ),
  ).toEqual([]);
}

/** The session cookie for the logged-in projects. Not `Secure`: the backend
 * derives that flag from `SSO_REDIRECT_URL`'s scheme
 * (crates/api/src/routes/auth.rs `cookie_secure`), so over a local
 * `http://localhost:3000` it is unset, and marking it Secure here would
 * make the browser drop it. `domain` is taken from `E2E_BASE_URL` so this
 * works against a deployed target too. */
function sessionState(value: string) {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  return {
    cookies: [
      {
        name: 'distant_signal_session',
        value,
        domain: base.hostname,
        path: '/',
        expires: -1,
        httpOnly: true,
        secure: base.protocol === 'https:',
        sameSite: 'Lax' as const,
      },
    ],
    origins: [],
  };
}

/** Every route that renders the same thing to an anonymous visitor on any
 * deployment with data in it. Kept as data rather than 20 hand-written
 * `test()` calls so that adding a route to `app/` is a one-line change
 * here -- the gap this suite existed to have and didn't. */
const PUBLIC_ROUTES: [name: string, path: string][] = [
  ['/', '/'],
  ['/lines', '/lines'],
  [`/lines/${REAL_LINE_ID}`, `/lines/${REAL_LINE_ID}`],
  [`/lines/${REAL_LINE_ID}/history`, `/lines/${REAL_LINE_ID}/history`],
  ['/lines/new', '/lines/new'],
  ['/stations', '/stations'],
  [`/stations/${REAL_STATION_CRS}`, `/stations/${REAL_STATION_CRS}`],
  ['/incidents', '/incidents'],
  [`/incidents/${REAL_INCIDENT_ID}`, `/incidents/${REAL_INCIDENT_ID}`],
  ['/trains', '/trains'],
  ['/track', '/track'],
  ['/groups/new', '/groups/new'],
  ['/connect-claude', '/connect-claude'],
  // A client-only page, and the only route here reachable without any
  // backend state. Visited with no OAuth params on purpose: that is its
  // error branch, which is the state a user actually lands in when the
  // callback goes wrong, and it is one of the `<Text c="...">{error}</Text>`
  // sites the contrast work below covers.
  ['/chat/callback', '/chat/callback'],
  // Deliberate 404, exercising not-found.tsx + the `<main>` landmark every
  // not-found.tsx inherits from app/layout.tsx.
  ['a deliberate 404', '/lines/nonexistent-line-slug'],
  // These four gate on a session and, logged out, render a heading plus an
  // auto-opened `LoginPromptModal`. That modal state is a real, reachable,
  // first-paint DOM for an anonymous visitor -- and is where the Mantine
  // close-button `button-name` defect lived -- so it is audited here rather
  // than treated as "logged-in only".
  ['/track/mine (anonymous login prompt)', '/track/mine'],
  ['/track/mine/add-ticket (anonymous login prompt)', '/track/mine/add-ticket'],
  ['/groups (anonymous login prompt)', '/groups'],
  ['/chat (anonymous login prompt)', '/chat'],
];

test.describe('accessibility: anonymous, light scheme', () => {
  for (const [name, path] of PUBLIC_ROUTES) {
    test(name, async ({ page }) => {
      await page.goto(path);
      await expectNoViolations(page);
    });
  }
});

// Dark mode was never audited before (the research doc says so in as many
// words), and the first sweep that did found a real, previously-invisible
// regression in `--mantine-color-dimmed` -- the app's single most
// widespread text style. `colorScheme: 'dark'` is enough on its own:
// app/layout.tsx mounts `<ColorSchemeScript defaultColorScheme="auto">`, so
// with no stored preference the app follows `prefers-color-scheme`.
test.describe('accessibility: anonymous, dark scheme', () => {
  test.use({ colorScheme: 'dark' });
  for (const [name, path] of PUBLIC_ROUTES) {
    test(name, async ({ page }) => {
      await page.goto(path);
      await expectNoViolations(page);
    });
  }
});

test.describe('accessibility: logged in', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value');
  test.use({ storageState: SESSION_COOKIE ? sessionState(SESSION_COOKIE) : undefined });

  // The routes whose logged-in DOM differs meaningfully from the anonymous
  // one: `/` gains the pinned-lines/stations/tracked-trains sections,
  // `/track/mine` and `/groups` replace a login modal with real content,
  // `/lines` gains pin controls, `/chat` and `/connect-claude` swap their
  // whole body.
  for (const [name, path] of [
    ['/', '/'],
    ['/lines', '/lines'],
    [`/stations/${REAL_STATION_CRS}`, `/stations/${REAL_STATION_CRS}`],
    ['/track/mine', '/track/mine'],
    ['/track/mine/add-ticket', '/track/mine/add-ticket'],
    ['/groups', '/groups'],
    ['/chat', '/chat'],
    ['/connect-claude', '/connect-claude'],
  ] as const) {
    test(name, async ({ page }) => {
      await page.goto(path);
      await expectNoViolations(page);
    });
  }

  test('/groups/[id]', async ({ page }) => {
    test.skip(!GROUP_ID, 'set E2E_GROUP_ID to a group the E2E_SESSION_COOKIE user belongs to');
    await page.goto(`/groups/${GROUP_ID}`);
    await expectNoViolations(page);
  });

  test('/groups/join/[token]', async ({ page }) => {
    test.skip(!GROUP_INVITE_TOKEN, 'set E2E_GROUP_INVITE_TOKEN to a live invite token');
    await page.goto(`/groups/join/${GROUP_INVITE_TOKEN}`);
    await expectNoViolations(page);
  });

  test('/lines/[id] for a custom line', async ({ page }) => {
    test.skip(!CUSTOM_LINE_ID, 'set E2E_CUSTOM_LINE_ID to a custom line the user can read');
    await page.goto(`/lines/${CUSTOM_LINE_ID}`);
    await expectNoViolations(page);
  });

  test('/lines/[id]/edit', async ({ page }) => {
    // Distinct from `/lines/new`, and not redundant with it: the form here
    // is PREFILLED, so it renders the station chips (and their remove
    // buttons) and any tag pills that an empty form never shows.
    test.skip(!CUSTOM_LINE_ID, 'set E2E_CUSTOM_LINE_ID to a custom line the user owns');
    await page.goto(`/lines/${CUSTOM_LINE_ID}/edit`);
    await expectNoViolations(page);
  });

  test('/train/by-id/[trackingId]', async ({ page }) => {
    test.skip(!TRACKING_ID, 'set E2E_TRACKING_ID to an UNRESOLVED tracked-train subscription id');
    await page.goto(`/train/by-id/${TRACKING_ID}`);
    // A resolved subscription redirects to /train/[uid]/[date]; asserting
    // we are still here is what stops this silently auditing that page
    // instead and reporting a pass for a route it never visited.
    expect(page.url()).toContain(`/train/by-id/${TRACKING_ID}`);
    await expectNoViolations(page);
  });
});

test.describe('accessibility: /train/[uid]/[date]', () => {
  // See the `TRAIN_UID` comment at the top of this file for why this pair
  // has no default.
  test.skip(
    !TRAIN_UID || !TRAIN_DATE,
    'set E2E_TRAIN_UID and E2E_TRAIN_DATE (YYYY-MM-DD) to a schedule that is live right now',
  );

  test('anonymous', async ({ page }) => {
    await page.goto(`/train/${TRAIN_UID}/${TRAIN_DATE}`);
    await expectNoViolations(page);
  });

  test('as the tracking owner', async ({ page, browser }) => {
    test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE');
    // The owner view adds the rename/stop-tracking/share controls and the
    // ticket panel, none of which the anonymous view renders.
    const context = await browser.newContext({ storageState: sessionState(SESSION_COOKIE!) });
    const owned = await context.newPage();
    await owned.goto(`/train/${TRAIN_UID}/${TRAIN_DATE}`);
    await expectNoViolations(owned);
    await context.close();
  });
});

// The states below do not exist in any page's first-paint DOM, so a
// route-level sweep cannot see them however many rules it runs. Every one
// of these found at least one real defect on its first run.
test.describe('accessibility: interactive sub-states', () => {
  test(`/lines/${REAL_LINE_ID}/history, Trends tab`, async ({ page }) => {
    await page.goto(`/lines/${REAL_LINE_ID}/history`);
    await page.getByRole('tab', { name: 'Trends' }).click();
    await expect(page.getByRole('tabpanel')).toBeVisible();
    await expectNoViolations(page);
  });

  test(`/lines/${REAL_LINE_ID}, every issue accordion expanded`, async ({ page }) => {
    // `IssueList` is `keepMounted={false}`, so `DisruptionDetail` and its
    // badges and "View full incident details" link do not exist in the DOM
    // until the panel is opened.
    await page.goto(`/lines/${REAL_LINE_ID}`);
    await expandAllAccordions(page);
    await expectNoViolations(page);
  });

  test(`/stations/${REAL_STATION_CRS}, every disclosure expanded`, async ({ page }) => {
    // Two independent `keepMounted={false}` surfaces here: the scheduled-
    // departures accordion (which also fires a `/api/trains/search` fetch)
    // and every `StationAccessibilitySection` disclosure. The latter is
    // where duplicate `role="region"` landmark names showed up.
    await page.goto(`/stations/${REAL_STATION_CRS}`);
    await expandAllAccordions(page);
    await expectNoViolations(page);
  });

  test('/incidents, results rendered', async ({ page }) => {
    await page.goto('/incidents');
    await page.getByRole('button', { name: /^Search/ }).first().click();
    await expect(page.getByRole('button', { name: /^Search/ }).first()).toBeEnabled();
    await expectNoViolations(page);
  });

  test('/trains, results rendered', async ({ page }) => {
    await page.goto(`/trains?station=${REAL_STATION_CRS}`);
    await expectNoViolations(page);
  });

  test('/track, departure picker populated', async ({ page }) => {
    // The picker's cancelled rows are the only place in the app that dims a
    // whole row, which is how a `filled` badge -- already AA-safe at full
    // strength -- ended up composited below AA.
    await page.goto(`/track?origin=${REAL_STATION_CRS}`);
    await expectNoViolations(page);
  });

  test('/lines/new, advanced options expanded', async ({ page }) => {
    await page.goto('/lines/new');
    await page.getByRole('button', { name: /Show advanced options/i }).click();
    await expectNoViolations(page);
  });

  test('/lines, operator filter dropdown open', async ({ page }) => {
    await page.goto('/lines');
    await page.getByRole('combobox', { name: /Filter by operator/i }).click();
    await expect(page.getByRole('listbox', { name: /Filter by operator/i })).toBeVisible();
    await expectNoViolations(page, COMBOBOX_PORTAL_WAIVERS);
  });

  // `/lines`'s operator filter is not the only combobox in the app, and a
  // dropdown that is never opened is a dropdown that is never scanned --
  // the whole reason this describe block exists. These are the other three
  // surfaces that mount one.
  test('/incidents, operator and line comboboxes open', async ({ page }) => {
    await page.goto('/incidents');
    await page.getByRole('combobox', { name: /Operator/i }).click();
    await expect(page.getByRole('listbox').first()).toBeVisible();
    await expectNoViolations(page, COMBOBOX_PORTAL_WAIVERS);
    await page.keyboard.press('Escape');
    await page.getByRole('combobox', { name: /^Line/i }).click();
    await expect(page.getByRole('listbox').first()).toBeVisible();
    await expectNoViolations(page, COMBOBOX_PORTAL_WAIVERS);
  });

  test('/trains, station autocomplete open with suggestions', async ({ page }) => {
    await page.goto('/trains');
    await page.getByRole('combobox', { name: /Station/i }).first().fill('Lon');
    await expect(page.getByRole('listbox').first()).toBeVisible();
    await expectNoViolations(page, COMBOBOX_PORTAL_WAIVERS);
  });

  test('/lines/new, station autocomplete open with suggestions', async ({ page }) => {
    await page.goto('/lines/new');
    await page.getByRole('combobox', { name: /Add station/i }).first().fill('Lon');
    await expect(page.getByRole('listbox').first()).toBeVisible();
    await expectNoViolations(page, COMBOBOX_PORTAL_WAIVERS);
  });

  // A conditional banner, not an interaction: the yellow retention `Alert`
  // only renders when the requested range reaches past the server's real
  // `line_status_history` ceiling, so the default `/lines/[id]/history` view
  // never shows it -- and a whole failing `variant="light"` colour hid
  // behind exactly that. Included as the standing reminder that "every
  // route, every sub-state" has to mean data-conditional states too.
  test(`/lines/${REAL_LINE_ID}/history, retention shortfall banner`, async ({ page }) => {
    await page.goto(`/lines/${REAL_LINE_ID}/history?range=30d`);
    const banner = page.getByText(/isn't available/i);
    test.skip(
      (await banner.count()) === 0,
      "this deployment's history retention covers 30 days, so the banner never renders",
    );
    await expectNoViolations(page);
  });

  test('combobox keyboard reachability (the evidence behind COMBOBOX_PORTAL_WAIVERS)', async ({
    page,
  }) => {
    // Not a duplicate of the axe run above: this is the assertion that
    // earns the `scrollable-region-focusable` waiver, by demonstrating the
    // thing that rule cannot see. If Mantine ever changes its combobox away
    // from `aria-activedescendant`, or the list stops scrolling to the
    // active option, this fails and the waiver has to be re-argued.
    await page.goto('/lines');
    const combobox = page.getByRole('combobox', { name: /Filter by operator/i });
    await combobox.click();
    await expect(page.getByRole('listbox', { name: /Filter by operator/i })).toBeVisible();
    expect(await combobox.getAttribute('aria-activedescendant')).toBeNull();

    await page.keyboard.press('ArrowDown');
    expect(await combobox.getAttribute('aria-activedescendant')).not.toBeNull();

    // Scoped to the portal node, which is the exact element axe reports:
    // `.mantine-ScrollArea-viewport` on its own also matches in-page scroll
    // areas, and a page-level one that never scrolls would make this pass
    // or fail for the wrong reason.
    const viewport = page.locator(
      '[data-mantine-shared-portal-node] .mantine-ScrollArea-viewport',
    );
    const { scrollable, options } = await viewport.evaluate((el) => ({
      scrollable: el.scrollHeight > el.clientHeight,
      options: el.querySelectorAll('[role="option"]').length,
    }));
    // A deployment with few enough operators to fit the dropdown without
    // scrolling has nothing to prove here -- and `scrollable-region-focusable`
    // would not have fired against it in the first place.
    test.skip(!scrollable, 'operator list is short enough not to scroll on this deployment');
    expect(await viewport.evaluate((el) => el.scrollTop)).toBe(0);

    // Stop one short of the end: Mantine's combobox WRAPS from the last
    // option back to the first, which would scroll the viewport back to 0
    // and make an over-long loop here look like a failure.
    for (let i = 0; i < options - 2; i++) await page.keyboard.press('ArrowDown');
    await expect.poll(() => viewport.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  });
});

test.describe('accessibility: interactive sub-states, logged in', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value');
  test.use({ storageState: SESSION_COOKIE ? sessionState(SESSION_COOKIE) : undefined });

  test('/track/mine/add-ticket, .pkpass upload tab', async ({ page }) => {
    await page.goto('/track/mine/add-ticket');
    await page.getByRole('tab', { name: /pkpass/i }).click();
    await expectNoViolations(page);
  });

  test('/track/mine/add-ticket, PDF upload tab', async ({ page }) => {
    await page.goto('/track/mine/add-ticket');
    await page.getByRole('tab', { name: /PDF/i }).click();
    await expectNoViolations(page);
  });

  test('/track/mine, stop-tracking confirmation modal', async ({ page }) => {
    // Mantine's `Modal` close button has no accessible name of its own;
    // `lib/theme.ts` supplies one for every modal in the app through
    // `components.Modal.defaultProps`. This is the live check on that.
    await page.goto('/track/mine');
    await page.getByRole('button', { name: /^Delete$/ }).first().click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Close' })).toBeVisible();
    await expectNoViolations(page);
  });

  test('/groups/[id], share-a-train modal', async ({ page }) => {
    test.skip(!GROUP_ID, 'set E2E_GROUP_ID');
    await page.goto(`/groups/${GROUP_ID}`);
    await page.getByRole('button', { name: /Add one of my trains/i }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await expectNoViolations(page);
  });

  test('/groups/[id], share-a-custom-line modal', async ({ page }) => {
    test.skip(!GROUP_ID, 'set E2E_GROUP_ID');
    await page.goto(`/groups/${GROUP_ID}`);
    await page.getByRole('button', { name: /Share one of my custom lines/i }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await expectNoViolations(page);
  });

  test('/groups/[id], rename modal', async ({ page }) => {
    test.skip(!GROUP_ID, 'set E2E_GROUP_ID');
    await page.goto(`/groups/${GROUP_ID}`);
    await page.getByRole('button', { name: /^Rename$/ }).first().click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await expectNoViolations(page);
  });

  // The state class that got away the first time round. Sixteen components
  // render a failed action as an inline red `<Text>`, and NONE of them is
  // reachable by navigating or clicking -- the mutation has to actually
  // fail. Every one of those sixteen shipped below AA in both colour
  // schemes, and no amount of route coverage or modal-opening would have
  // found it, because axe can only see states the page is actually in.
  //
  // Failing the request at the network layer, rather than pointing the app
  // at a broken backend, keeps this a deterministic check of the ERROR
  // RENDERING and not of the API.
  test('a failed mutation renders its inline error text readably', async ({ page }) => {
    await page.goto('/track/mine');
    // Empty body on purpose: `DeleteTrainButton` renders the response body
    // as the message when there is one, so an empty 500 takes its
    // `Request failed: <status>` fallback -- a string this test can match
    // without depending on whatever prose a backend happens to return.
    await page.route('**/api/**', (route) =>
      route.request().method() === 'GET' ? route.fallback() : route.fulfill({ status: 500, body: '' }),
    );
    await page.getByRole('button', { name: /^Delete$/ }).first().click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await page.getByRole('button', { name: /Confirm delete|^Delete$/ }).last().click();
    // The assertion that keeps this from passing vacuously: if the error
    // text never rendered, there is nothing here to have measured.
    await expect(page.getByText(/Request failed/i).first()).toBeVisible();
    await expectNoViolations(page);
  });
});

test.describe('accessibility: logged in, dark scheme', () => {
  // The logged-in pages carry most of this app's `c="dimmed"` density (the
  // tracked-train and group cards), which is exactly where the dark-scheme
  // contrast regression showed up -- so the two halves are crossed rather
  // than each tested only in the light scheme.
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value');
  test.use({
    colorScheme: 'dark',
    storageState: SESSION_COOKIE ? sessionState(SESSION_COOKIE) : undefined,
  });

  for (const [name, path] of [
    ['/', '/'],
    ['/track/mine', '/track/mine'],
    ['/groups', '/groups'],
    [`/stations/${REAL_STATION_CRS}`, `/stations/${REAL_STATION_CRS}`],
    [`/lines/${REAL_LINE_ID}`, `/lines/${REAL_LINE_ID}`],
  ] as const) {
    test(name, async ({ page }) => {
      await page.goto(path);
      await expectNoViolations(page);
    });
  }

  test('/groups/[id]', async ({ page }) => {
    test.skip(!GROUP_ID, 'set E2E_GROUP_ID');
    await page.goto(`/groups/${GROUP_ID}`);
    await expectNoViolations(page);
  });
});

/** Opens every `Accordion` control on the page. Both accordion users in
 * this app are `keepMounted={false}`, so their panels are not merely hidden
 * -- they are not in the DOM at all, and axe cannot see them unopened. */
async function expandAllAccordions(page: Page) {
  const controls = page.locator('button.mantine-Accordion-control[aria-expanded="false"]');
  // Re-queried each pass rather than iterated once: opening one panel can
  // mount another accordion inside it (a station's accessibility groups do
  // exactly this), and a stale locator list would miss those.
  for (let pass = 0; pass < 4; pass++) {
    const count = await controls.count();
    if (count === 0) break;
    for (let i = 0; i < count; i++) {
      const control = controls.nth(0);
      if ((await control.count()) === 0) break;
      await control.click();
      await page.waitForTimeout(150);
    }
  }
  // Give any fetch a panel kicked off (the station timetable's
  // `/api/trains/search`) a chance to render before axe reads the DOM.
  await page.waitForLoadState('networkidle').catch(() => {});
}
