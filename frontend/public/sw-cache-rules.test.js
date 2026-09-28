import { describe, it, expect } from 'vitest';
import cacheRules from './sw-cache-rules.js';

const { isCacheable, parsePushPayload, sameOriginNotificationUrl } = cacheRules;

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

// FE-10
describe('parsePushPayload', () => {
  it('returns the parsed object for a JSON payload', () => {
    expect(parsePushPayload({ json: () => ({ title: 't', url: '/lines/x' }) })).toEqual({
      title: 't',
      url: '/lines/x',
    });
  });

  it('returns null, not a throw, for a non-JSON payload', () => {
    expect(
      parsePushPayload({
        json: () => {
          throw new SyntaxError('Unexpected token');
        },
      }),
    ).toBeNull();
  });

  it('returns null for a missing payload or a non-object JSON value', () => {
    expect(parsePushPayload(null)).toBeNull();
    expect(parsePushPayload({ json: () => 'just a string' })).toBeNull();
    expect(parsePushPayload({ json: () => [1, 2] })).toBeNull();
    expect(parsePushPayload({ json: () => null })).toBeNull();
  });
});

describe('sameOriginNotificationUrl', () => {
  const origin = 'https://ds.example';

  it('resolves an app-relative path on this origin', () => {
    expect(sameOriginNotificationUrl('/lines/bakerloo', origin)).toBe('https://ds.example/lines/bakerloo');
  });

  it('accepts an absolute URL on this same origin', () => {
    expect(sameOriginNotificationUrl('https://ds.example/track/42', origin)).toBe('https://ds.example/track/42');
  });

  it.each(['https://evil.example/', '//evil.example/x', 'http://ds.example/', 'javascript:alert(1)'])(
    'refuses %s',
    (url) => {
      expect(sameOriginNotificationUrl(url, origin)).toBeNull();
    },
  );

  it('falls back to the site root for a missing or non-string url', () => {
    expect(sameOriginNotificationUrl(undefined, origin)).toBe('https://ds.example/');
    expect(sameOriginNotificationUrl(42, origin)).toBe('https://ds.example/');
  });
});
