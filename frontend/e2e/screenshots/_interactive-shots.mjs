#!/usr/bin/env node
// _interactive-shots.mjs
//
// Companion to take-screenshots.mjs for shots that need a UI interaction
// (e.g. clicking TrackTrainForm's "Search a time window" segmented-control
// option) before the screenshot is taken -- something the plain
// navigate-and-screenshot config shape in take-screenshots.mjs doesn't
// support. Deliberately a separate script rather than an edit to
// take-screenshots.mjs, since that file is shared with sibling
// screenshot-taking agents running concurrently against the same
// manifest.ndjson.
//
// Writes the exact same manifest.ndjson entry shape as take-screenshots.mjs
// (name, url, viewport, browser, authenticated, description, filePath,
// timestamp, [error]) and appends (not overwrites) to the same file, so a
// reviewer skimming the manifest sees one consistent log regardless of
// which script produced a given shot.
//
// Usage: node e2e/screenshots/_interactive-shots.mjs <shots-config.json>
//
// Config shape: same as take-screenshots.mjs, plus a required `actions`
// array applied after navigation and before the screenshot. Each action:
//   { "type": "click", "text": "Search a time window" }   -- click first
//                                                             element whose
//                                                             text matches
//   { "type": "click", "selector": "#some-id" }            -- click by CSS
//   { "type": "fill", "selector": "input[...]", "value": "KGX" }
//   { "type": "wait", "ms": 500 }

import { chromium, firefox } from '@playwright/test';
import { mkdir, appendFile, readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '../../../');

const BASE_URL = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:3000';
const SESSION_COOKIE =
  process.env.E2E_SESSION_COOKIE ?? 'preview-demo-session-token-for-demo-user';
const OUT_DIR = process.env.SCREENSHOTS_OUT_DIR
  ? path.resolve(process.env.SCREENSHOTS_OUT_DIR)
  : path.join(__dirname, 'output');
const MANIFEST_PATH = path.join(OUT_DIR, 'manifest.ndjson');

const BROWSER_LAUNCHERS = { chromium, firefox };

function sessionCookie(hostname, value) {
  return {
    name: 'distant_signal_session',
    value,
    domain: hostname,
    path: '/',
    httpOnly: true,
    secure: false,
    sameSite: 'Lax',
  };
}

async function applyAction(page, action) {
  if (action.type === 'click') {
    if (action.text) {
      await page.getByText(action.text, { exact: action.exact ?? true }).first().click();
    } else if (action.selector) {
      await page.locator(action.selector).first().click();
    }
  } else if (action.type === 'fill') {
    await page.locator(action.selector).first().fill(action.value);
  } else if (action.type === 'wait') {
    await page.waitForTimeout(action.ms ?? 500);
  } else {
    throw new Error(`Unknown action type: ${action.type}`);
  }
}

async function appendManifestEntry(entry) {
  await appendFile(MANIFEST_PATH, JSON.stringify(entry) + '\n', 'utf8');
}

async function takeShot(browsers, shot) {
  const targetUrl = new URL(shot.url, BASE_URL).toString();
  const hostname = new URL(BASE_URL).hostname;
  const launcher = browsers[shot.browser];

  const context = await launcher.newContext({
    viewport: { width: shot.viewport.width, height: shot.viewport.height },
  });

  try {
    if (shot.authenticated) {
      await context.addCookies([sessionCookie(hostname, SESSION_COOKIE)]);
    }
    const page = await context.newPage();
    await page.goto(targetUrl, { waitUntil: 'load', timeout: 30_000 });
    await page.waitForTimeout(1000);
    for (const action of shot.actions ?? []) {
      await applyAction(page, action);
    }
    if (shot.waitAfterMs) {
      await page.waitForTimeout(shot.waitAfterMs);
    }

    const filePath = path.join(OUT_DIR, `${shot.name}.png`);
    await page.screenshot({ path: filePath, fullPage: Boolean(shot.fullPage) });

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
    console.error('Usage: node e2e/screenshots/_interactive-shots.mjs <shots-config.json>');
    process.exit(1);
  }
  const resolved = path.resolve(configPath);
  if (!existsSync(resolved)) throw new Error(`Config file not found: ${resolved}`);
  const shots = JSON.parse(await readFile(resolved, 'utf8'));

  await mkdir(OUT_DIR, { recursive: true });

  const neededBrowsers = new Set(shots.map((s) => s.browser));
  const browsers = {};
  for (const name of neededBrowsers) browsers[name] = await BROWSER_LAUNCHERS[name].launch();

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
    for (const browser of Object.values(browsers)) await browser.close();
  }

  console.log(`\nDone: ${succeeded} succeeded, ${failed} failed.`);
  console.log(`Manifest: ${path.relative(REPO_ROOT, MANIFEST_PATH)}`);
  if (failed > 0) process.exit(1);
}

main().catch((err) => {
  console.error(err instanceof Error ? err.stack : err);
  process.exit(1);
});
