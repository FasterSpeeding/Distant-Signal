#!/usr/bin/env node
// Dependency-free build-time script (see
// docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md
// Decision 5) -- Node's own fs/path/url modules only, no new
// devDependency. Run as an added step in package.json's `build` script,
// AFTER `next build` (so .next/BUILD_ID exists) and BEFORE the Docker
// image layer finalizes (so the stamped file, not the placeholder, is
// what ships) -- substitutes sw.js's CACHE_NAME placeholder with the real
// per-build id, so sw.js's own byte content changes on every deploy. This
// is what the browser's native SW-update check (a byte-for-byte
// comparison against the currently-installed worker) and sw.js's own
// `activate` purge (Task 4) both depend on to actually invalidate a prior
// deploy's cache.
//
// NOTE: this rewrites frontend/public/sw.js IN PLACE, a tracked source
// file. Running `npm run build` outside Docker (e.g. locally, to sanity-
// check this script) leaves a real-BUILD_ID diff in your working tree --
// `git checkout -- frontend/public/sw.js` to discard it before committing
// anything else.

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const buildIdPath = path.join(__dirname, '..', '.next', 'BUILD_ID');
const swPath = path.join(__dirname, '..', 'public', 'sw.js');

const PLACEHOLDER = '__BUILD_ID__';

const buildId = readFileSync(buildIdPath, 'utf8').trim();
const swSource = readFileSync(swPath, 'utf8');

if (!swSource.includes(PLACEHOLDER)) {
  throw new Error(
    `stamp-sw-version: ${swPath} does not contain the ${PLACEHOLDER} placeholder -- ` +
      "either it was already stamped by a previous run of this script against the same " +
      "checkout (see this file's own top comment), or sw.js's CACHE_NAME constant was " +
      'edited without preserving the placeholder.',
  );
}

writeFileSync(swPath, swSource.replace(PLACEHOLDER, buildId));
console.log(`stamp-sw-version: stamped public/sw.js with BUILD_ID ${buildId}`);
