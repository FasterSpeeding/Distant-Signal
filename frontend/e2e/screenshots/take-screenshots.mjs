#!/usr/bin/env node
// take-screenshots.mjs
//
// Standalone batch screenshotter for the running app, driven by Playwright
// directly (not the @playwright/test runner/reporter) so it can be invoked
// as a plain `node` script, repeatedly, by different follow-up agents
// targeting different page groups -- without needing a test-runner
// context and without clobbering each other's output.
//
// Reuses this project's browser-launch conventions from
// frontend/playwright.config.ts (Desktop Chrome / Desktop Firefox device
// descriptors) and the session-cookie shape from e2e/nav.spec.ts and
// e2e/accessibility.spec.ts (`sessionState` / `SESSION_COOKIE`).
//
// -----------------------------------------------------------------------
// Usage
// -----------------------------------------------------------------------
//   node e2e/screenshots/take-screenshots.mjs <shots-config.json>
//
// Env vars (all optional):
//   E2E_BASE_URL       Base URL shots' `url` is resolved against.
//                       Default: http://127.0.0.1:3000
//   E2E_SESSION_COOKIE Raw `distant_signal_session` cookie value, set on
//                       the browser context before navigating for any
//                       shot with `authenticated: true`.
//                       Default: preview-demo-session-token-for-demo-user
//   SCREENSHOTS_OUT_DIR Directory screenshots + manifest are written to.
//                       Default: e2e/screenshots/output (relative to this
//                       file), i.e. frontend/e2e/screenshots/output/
//
// -----------------------------------------------------------------------
// Config file shape (JSON array of "shots")
// -----------------------------------------------------------------------
//   [
//     {
//       "name": "homepage-desktop",              // required, kebab-case,
//                                                 // becomes <name>.png
//       "url": "/",                               // required, path
//                                                 // appended to base URL
//       "viewport": { "width": 1440, "height": 900 }, // required
//       "browser": "chromium",                    // required:
//                                                 // "chromium" | "firefox"
//       "authenticated": true,                    // optional, default
//                                                 // false. Sets the
//                                                 // session cookie
//                                                 // before navigating.
//       "waitFor": "nav",                         // optional. A string
//                                                 // is treated as a CSS
//                                                 // selector to wait
//                                                 // for. A number is a
//                                                 // plain delay in ms.
//                                                 // An object form is
//                                                 // also accepted:
//                                                 // { "selector": "...",
//                                                 //   "timeout": 5000 }
//                                                 // or { "delay": 1500 }.
//       "fullPage": false,                        // optional, default
//                                                 // false (viewport-only
//                                                 // capture).
//       "description": "What this shot verifies." // required, one
//                                                 // sentence, becomes
//                                                 // the manifest entry's
//                                                 // `description`.
//     },
//     ...
//   ]
//
// -----------------------------------------------------------------------
// Output
// -----------------------------------------------------------------------
//   <out-dir>/<name>.png       One PNG per shot.
//   <out-dir>/manifest.ndjson  One JSON object appended per shot, in the
//                              form:
//                                { name, url, viewport, browser,
//                                  authenticated, description, filePath,
//                                  timestamp, error? }
//                              filePath is repo-root-relative. Appends
//                              use O_APPEND so concurrent/sequential
//                              invocations across different shot-config
//                              files accumulate into the same manifest
//                              rather than overwriting it -- no shared
//                              lock needed, since each append is a single
//                              write() of one line.
//
// Both the *.png files and manifest.ndjson are git-ignored (see
// e2e/screenshots/.gitignore) -- these are large, disposable, per-run
// binary artifacts, not something to commit.
//
// -----------------------------------------------------------------------
// Failure handling
// -----------------------------------------------------------------------
// A failure on one shot (404, navigation timeout, crashed page, missing
// waitFor selector, etc.) is caught, logged to stderr, and recorded as a
// manifest entry with an `error` field (and no `filePath`) -- the script
// moves on to the next shot rather than aborting the batch. The process
// exits non-zero if any shot failed, so callers can detect a partial run,
// but every shot in the config is still attempted.

import { chromium, firefox, devices } from '@playwright/test';
import { mkdir, appendFile, readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
// Repo root, for repo-relative filePath entries in the manifest.
const REPO_ROOT = path.resolve(__dirname, '../../../');

const BASE_URL = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:3000';
const SESSION_COOKIE =
  process.env.E2E_SESSION_COOKIE ?? 'preview-demo-session-token-for-demo-user';
const OUT_DIR = process.env.SCREENSHOTS_OUT_DIR
  ? path.resolve(process.env.SCREENSHOTS_OUT_DIR)
  : path.join(__dirname, 'output');
const MANIFEST_PATH = path.join(OUT_DIR, 'manifest.ndjson');

/** @typedef {import('@playwright/test').Browser} Browser */
/** @typedef {import('@playwright/test').Page} Page */
/** @typedef {keyof typeof BROWSER_LAUNCHERS} BrowserName */
/** @typedef {Partial<Record<BrowserName, Browser>>} Browsers */
/**
 * A shot's optional `waitFor` (see "Config file shape" above).
 * @typedef {string | number | { selector?: string, timeout?: number, delay?: number }} WaitFor
 */
/**
 * One entry of a shots config file. The file is unvalidated JSON, so
 * validateShot() checks the required fields at runtime.
 * @typedef {object} Shot
 * @property {string} name
 * @property {string} url
 * @property {{ width: number, height: number }} viewport
 * @property {BrowserName} browser
 * @property {boolean} [authenticated]
 * @property {WaitFor} [waitFor]
 * @property {boolean} [fullPage]
 * @property {string} description
 */
/**
 * One manifest.ndjson line (see "Output" above).
 * @typedef {object} ManifestEntry
 * @property {string} name
 * @property {string} url
 * @property {{ width: number, height: number }} viewport
 * @property {BrowserName} browser
 * @property {boolean} authenticated
 * @property {string} description
 * @property {string} [filePath]
 * @property {string} timestamp
 * @property {string} [error]
 */
/**
 * A shot as read from the config file, before validateShot() has checked it.
 * @typedef {{ [K in keyof Shot]?: unknown } & { viewport?: { width?: unknown, height?: unknown } }} UnvalidatedShot
 */

const BROWSER_LAUNCHERS = { chromium, firefox };
const DEVICE_PRESETS = {
  chromium: devices['Desktop Chrome'],
  firefox: devices['Desktop Firefox'],
};

/** Same cookie shape as e2e/nav.spec.ts's `sessionState` /
 * e2e/accessibility.spec.ts: not Secure, because the backend only marks
 * the cookie Secure when served over HTTPS (crates/api/src/routes/auth.rs
 * `cookie_secure`), and a Secure cookie would never be sent to a local
 * http:// origin. Domain is derived from the base URL's hostname (not
 * hardcoded to "localhost") so this also works against 127.0.0.1.
 * @param {string} hostname
 * @param {string} value */
function sessionCookie(hostname, value) {
  return {
    name: 'distant_signal_session',
    value,
    domain: hostname,
    path: '/',
    httpOnly: true,
    secure: false,
    sameSite: /** @type {const} */ ('Lax'),
  };
}

/** @param {string} name */
function kebabCheck(name) {
  return /^[a-z0-9]+(-[a-z0-9]+)*$/.test(name);
}

/**
 * @param {string} configPath
 * @returns {Promise<unknown[]>} the shots, not yet validated
 */
async function loadShots(configPath) {
  const resolved = path.resolve(configPath);
  if (!existsSync(resolved)) {
    throw new Error(`Config file not found: ${resolved}`);
  }
  const raw = await readFile(resolved, 'utf8');
  /** @type {unknown} */
  const shots = JSON.parse(raw);
  if (!Array.isArray(shots)) {
    throw new Error('Config file must contain a JSON array of shots.');
  }
  /** @type {unknown[]} */
  const elements = shots;
  return elements;
}

/**
 * @param {unknown} value - one element of the config file's array
 * @param {number} index
 * @returns {Shot} `value`, once every check has passed
 */
function validateShot(value, index) {
  const shot = /** @type {UnvalidatedShot} */ (value);
  const problems = [];
  if (!shot.name || typeof shot.name !== 'string') problems.push('missing `name`');
  else if (!kebabCheck(shot.name)) problems.push(`\`name\` "${shot.name}" is not kebab-case`);
  if (!shot.url || typeof shot.url !== 'string') problems.push('missing `url`');
  if (
    !shot.viewport ||
    typeof shot.viewport.width !== 'number' ||
    typeof shot.viewport.height !== 'number'
  ) {
    problems.push('missing/invalid `viewport` ({width, height})');
  }
  if (typeof shot.browser !== 'string' || !Object.hasOwn(BROWSER_LAUNCHERS, shot.browser)) {
    problems.push(`\`browser\` must be "chromium" or "firefox", got "${String(shot.browser)}"`);
  }
  if (!shot.description || typeof shot.description !== 'string') {
    problems.push('missing `description`');
  }
  if (problems.length > 0) {
    // eslint-disable-next-line @typescript-eslint/no-base-to-string -- echoes whatever the malformed config held, as before
    throw new Error(`shots[${index}] (${String(shot.name ?? '?')}): ${problems.join('; ')}`);
  }
  return /** @type {Shot} */ (shot);
}

/** Applies a shot's optional `waitFor` after navigation. String -> CSS
 * selector wait. Number -> plain delay (ms). Object -> {selector,
 * timeout} or {delay}.
 * @param {Page} page
 * @param {WaitFor | undefined} waitFor */
async function applyWaitFor(page, waitFor) {
  if (waitFor == null) return;
  if (typeof waitFor === 'string') {
    await page.waitForSelector(waitFor, { timeout: 10_000 });
    return;
  }
  if (typeof waitFor === 'number') {
    await page.waitForTimeout(waitFor);
    return;
  }
  if (typeof waitFor === 'object') {
    if (waitFor.selector) {
      await page.waitForSelector(waitFor.selector, {
        timeout: waitFor.timeout ?? 10_000,
      });
    }
    if (typeof waitFor.delay === 'number') {
      await page.waitForTimeout(waitFor.delay);
    }
    return;
  }
  throw new Error(`Unrecognized waitFor shape: ${JSON.stringify(waitFor)}`);
}

/** @param {ManifestEntry} entry */
async function appendManifestEntry(entry) {
  await appendFile(MANIFEST_PATH, JSON.stringify(entry) + '\n', 'utf8');
}

/**
 * @param {Browsers} browsers - one launched browser per `browser` the
 *   config names
 * @param {Shot} shot
 * @returns {Promise<ManifestEntry & { filePath: string }>}
 */
async function takeShot(browsers, shot) {
  const targetUrl = new URL(shot.url, BASE_URL).toString();
  const hostname = new URL(BASE_URL).hostname;
  const launcher = browsers[shot.browser];
  if (!launcher) throw new Error(`no ${shot.browser} browser was launched`);

  const context = await launcher.newContext({
    ...DEVICE_PRESETS[shot.browser],
    viewport: { width: shot.viewport.width, height: shot.viewport.height },
  });

  try {
    if (shot.authenticated) {
      await context.addCookies([sessionCookie(hostname, SESSION_COOKIE)]);
    }

    const page = await context.newPage();
    await page.goto(targetUrl, { waitUntil: 'load', timeout: 30_000 });
    await applyWaitFor(page, shot.waitFor);

    const filePath = path.join(OUT_DIR, `${shot.name}.png`);
    await page.screenshot({
      path: filePath,
      fullPage: Boolean(shot.fullPage),
    });

    return {
      name: shot.name,
      url: shot.url,
      viewport: shot.viewport,
      browser: shot.browser,
      authenticated: Boolean(shot.authenticated),
      description: shot.description,
      filePath: path.relative(REPO_ROOT, filePath),
      timestamp: new Date().toISOString(),
    };
  } finally {
    await context.close();
  }
}

async function main() {
  const configPath = process.argv[2];
  if (!configPath) {
    console.error(
      'Usage: node e2e/screenshots/take-screenshots.mjs <shots-config.json>'
    );
    process.exit(1);
  }

  // Fail fast on a malformed config, before launching any browser.
  const shots = (await loadShots(configPath)).map(validateShot);

  await mkdir(OUT_DIR, { recursive: true });

  const neededBrowsers = new Set(shots.map((s) => s.browser));
  /** @type {Browsers} */
  const browsers = {};
  for (const name of neededBrowsers) {
    browsers[name] = await BROWSER_LAUNCHERS[name].launch();
  }

  let succeeded = 0;
  let failed = 0;

  try {
    for (const shot of shots) {
      try {
        const entry = await takeShot(browsers, shot);
        await appendManifestEntry(entry);
        succeeded += 1;
        console.log(`[ok]   ${shot.name} -> ${entry.filePath}`);
      } catch (err) {
        failed += 1;
        // Stripping ANSI color codes Playwright embeds in its own error
        // messages (e.g. from a waitForSelector timeout's call log), so the
        // manifest stays plain text/JSON-clean rather than carrying
        // terminal escape sequences.
        const rawMessage = err instanceof Error ? err.message : String(err);
        const message = rawMessage.replace(/\x1b\[[0-9;]*m/g, '');
        console.error(`[fail] ${shot.name}: ${message}`);
        await appendManifestEntry({
          name: shot.name,
          url: shot.url,
          viewport: shot.viewport,
          browser: shot.browser,
          authenticated: Boolean(shot.authenticated),
          description: shot.description,
          timestamp: new Date().toISOString(),
          error: message,
        });
      }
    }
  } finally {
    for (const browser of Object.values(browsers)) {
      await browser.close();
    }
  }

  console.log(`\nDone: ${succeeded} succeeded, ${failed} failed.`);
  console.log(`Manifest: ${path.relative(REPO_ROOT, MANIFEST_PATH)}`);
  if (failed > 0) process.exit(1);
}

main().catch((/** @type {unknown} */ err) => {
  console.error(err instanceof Error ? err.stack : err);
  process.exit(1);
});
