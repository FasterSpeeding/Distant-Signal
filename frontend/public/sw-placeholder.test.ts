import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

describe('public/sw.js as committed', () => {
  it('keeps the __BUILD_ID__ placeholder for scripts/stamp-sw-version.mjs to replace at build time', () => {
    // A build stamps the id into this file in place. Committing a stamped
    // copy (it happened once, 2026-09-29) leaves the stamp script nothing to
    // replace, so the cache name stops changing per deploy and old caches are
    // never purged.
    const source = readFileSync(join(__dirname, 'sw.js'), 'utf8');
    expect(source).toContain("const CACHE_NAME = 'distant-signal-__BUILD_ID__';");
  });
});
