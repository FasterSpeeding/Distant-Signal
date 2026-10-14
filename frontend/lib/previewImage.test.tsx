// @vitest-environment node
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { previewTitleFontSize, previewTitleFromMetadata, renderPreviewImage } from './previewImage';

describe('previewTitleFromMetadata', () => {
  it('takes a plain or absolute title, and nothing else', () => {
    expect(previewTitleFromMetadata({ title: 'Woking (WOK)' })).toBe('Woking (WOK)');
    expect(previewTitleFromMetadata({ title: { absolute: 'Terms of use · Distant Signal' } })).toBe(
      'Terms of use · Distant Signal',
    );
    expect(previewTitleFromMetadata({})).toBeNull();
  });
});

describe('previewTitleFontSize', () => {
  it('steps down for longer titles', () => {
    expect(previewTitleFontSize('Status')).toBeGreaterThan(previewTitleFontSize('London Kings Cross (KGX)!'));
    expect(previewTitleFontSize('x'.repeat(30))).toBeGreaterThan(previewTitleFontSize('x'.repeat(60)));
  });
});

describe('renderPreviewImage', () => {
  it.each([null, 'Status', 'Great Western Railway: London Paddington to Bristol Temple Meads'])(
    'renders a 1200x630 PNG for %s',
    async (title) => {
      const response = renderPreviewImage(title);
      expect(response.headers.get('content-type')).toBe('image/png');
      const png = Buffer.from(await response.arrayBuffer());
      expect(png.subarray(1, 4).toString('ascii')).toBe('PNG');
      // IHDR: width and height, big-endian, at bytes 16 to 23.
      expect(png.readUInt32BE(16)).toBe(1200);
      expect(png.readUInt32BE(20)).toBe(630);
    },
    30_000,
  );
});

describe('the routes with their own preview', () => {
  /** Every opengraph-image route under app/, relative to it. */
  function previewRoutes(dir = 'app', found: string[] = []): string[] {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) previewRoutes(path, found);
      else if (entry.name === 'opengraph-image.tsx') found.push(path);
    }
    return found;
  }

  it('all use the shared template, never a title from the request', () => {
    const routes = previewRoutes();
    expect(routes.length).toBeGreaterThanOrEqual(10);
    for (const route of routes) {
      const source = readFileSync(route, 'utf8');
      expect(source, route).toContain('renderPreviewImage(');
      expect(source, route).not.toMatch(/searchParams/);
    }
  });
});
