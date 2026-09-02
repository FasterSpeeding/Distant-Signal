import { describe, it, expect } from 'vitest';
import cacheRules from './sw-cache-rules.js';

const { isCacheable } = cacheRules;

describe('isCacheable', () => {
  it.each([
    ['/_next/static/chunks/main-abc123.js', true],
    ['/_next/static/css/app-def456.css', true],
    ['/icon-192.png', true],
    ['/icon-512.png', true],
    ['/manifest.webmanifest', true],
    ['/offline.html', true],
  ])('%s is cacheable', (pathname, expected) => {
    expect(isCacheable(pathname)).toBe(expected);
  });

  it.each([
    ['/', false],
    ['/lines/123', false],
    ['/api/preferences', false],
    ['/api/Train/track', false],
    // Close-but-not-actually-matching shapes, guarding against an
    // overly loose prefix/substring check rather than an exact
    // pathname comparison:
    ['/icon-192.png/evil', false],
    ['/notmanifest.webmanifest', false],
    ['/sw.js', false],
  ])('%s is NOT cacheable', (pathname, expected) => {
    expect(isCacheable(pathname)).toBe(expected);
  });
});
