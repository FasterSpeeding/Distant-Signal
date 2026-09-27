import { describe, it, expect } from 'vitest';
// Next's own serializer for `MetadataRoute.Robots` -- the same function the
// built `/robots.txt` route calls -- so these assertions check the text a
// crawler actually receives, not just the object. It's an internal module
// path; if a Next upgrade moves it, update this import.
import { resolveRobots } from 'next/dist/build/webpack/loaders/metadata/resolve-route-data';
import { AI_TRAINING_CRAWLERS, BLOCK_AI_TRAINING_CRAWLERS, buildRobots } from './robots';

const render = (...args: Parameters<typeof buildRobots>) => resolveRobots(buildRobots(...args));
const lines = (text: string) => text.split('\n');
/** Lines of the `User-Agent: *` group only: from its header up to the blank
 * line that ends it. The AI-crawler group's `Disallow: /` must not count
 * against the general rules. */
const starGroup = (text: string) => {
  const all = lines(text);
  const start = all.indexOf('User-Agent: *');
  const end = all.indexOf('', start);
  return all.slice(start, end === -1 ? undefined : end);
};

describe('buildRobots', () => {
  it('allows the site by default and re-allows only /journeys/new under /journeys/', () => {
    const out = lines(render({ origin: undefined }));
    expect(out[0]).toBe('User-Agent: *');
    expect(out).toContain('Allow: /');
    expect(out).toContain('Allow: /journeys/new$');
  });

  it.each([
    '/api/',
    '/chat',
    '/connect-claude/authorize',
    '/groups',
    '/journeys/',
    '/track/mine',
    '/track/tickets',
    '/train/by-id/',
    '/lines/new',
    '/lines/*/edit',
    '/*?',
  ])('disallows %s', (path) => {
    expect(lines(render({ origin: undefined }))).toContain(`Disallow: ${path}`);
  });

  it('keeps token-bearing share and invite links behind a disallowed prefix', () => {
    const disallowed = starGroup(render({ origin: undefined }))
      .filter((l) => l.startsWith('Disallow: '))
      .map((l) => l.slice('Disallow: '.length));
    for (const tokenPath of ['/journeys/shared/abc123', '/groups/join/abc123', '/chat/callback']) {
      expect(disallowed.some((prefix) => tokenPath.startsWith(prefix))).toBe(true);
    }
  });

  it('does not disallow the public pages', () => {
    const disallowed = starGroup(render({ origin: undefined }))
      .filter((l) => l.startsWith('Disallow: '))
      .map((l) => l.slice('Disallow: '.length))
      // Wildcard rules are checked separately above.
      .filter((p) => !p.includes('*'));
    for (const publicPath of [
      '/',
      '/status',
      '/lines',
      '/lines/bakerloo',
      '/lines/bakerloo/history',
      '/operators',
      '/network/history',
      '/stations',
      '/stations/KGX',
      '/trains',
      '/train/C12345/2026-09-27',
      '/incidents',
      '/incidents/42',
      '/connect-claude',
      '/track',
      '/attribution',
      '/privacy',
      '/terms',
      '/cookies',
    ]) {
      expect(disallowed.filter((prefix) => publicPath.startsWith(prefix))).toEqual([]);
    }
  });

  it('omits Host and Sitemap when no origin is configured', () => {
    const text = render({ origin: undefined, sitemapPath: '/sitemap.xml' });
    expect(text).not.toMatch(/^Host:/m);
    expect(text).not.toMatch(/^Sitemap:/m);
    expect(text).not.toContain('undefined');
  });

  it('emits Host from the configured origin, and no Sitemap while the app has none', () => {
    const text = render({ origin: 'https://ds.example' });
    expect(text).toMatch(/^Host: https:\/\/ds\.example$/m);
    expect(text).not.toMatch(/^Sitemap:/m);
  });

  it('emits an absolute Sitemap line when a sitemap path and origin are both set', () => {
    const text = render({ origin: 'https://ds.example', sitemapPath: '/sitemap.xml' });
    expect(text).toMatch(/^Sitemap: https:\/\/ds\.example\/sitemap\.xml$/m);
  });

  it('blocks AI training crawlers by default (owner decision, 2026-09-27)', () => {
    expect(BLOCK_AI_TRAINING_CRAWLERS).toBe(true);
    const out = lines(render({ origin: undefined }));
    for (const ua of AI_TRAINING_CRAWLERS) {
      expect(out).toContain(`User-Agent: ${ua}`);
    }
    const lastUa = out.lastIndexOf(`User-Agent: ${AI_TRAINING_CRAWLERS.at(-1)}`);
    expect(out[lastUa + 1]).toBe('Disallow: /');
  });

  it('keeps the general rules for every other crawler when AI crawlers are blocked', () => {
    const out = lines(render({ origin: undefined }));
    expect(out[0]).toBe('User-Agent: *');
    expect(out).toContain('Allow: /');
  });

  it('does not block AI training crawlers when the toggle is off', () => {
    const text = render({ origin: undefined, blockAiTrainingCrawlers: false });
    for (const ua of AI_TRAINING_CRAWLERS) {
      expect(text).not.toContain(`User-Agent: ${ua}`);
    }
  });

  it('blocks every AI training crawler entirely when the toggle is on', () => {
    const out = lines(render({ origin: undefined, blockAiTrainingCrawlers: true }));
    for (const ua of AI_TRAINING_CRAWLERS) {
      expect(out).toContain(`User-Agent: ${ua}`);
    }
    const lastUa = out.lastIndexOf(`User-Agent: ${AI_TRAINING_CRAWLERS.at(-1)}`);
    expect(out[lastUa + 1]).toBe('Disallow: /');
  });
});
