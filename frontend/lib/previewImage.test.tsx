// @vitest-environment node
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import { signalArmBounds } from './brand';
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

/** A non-interlaced 8-bit RGB or RGBA PNG (what next/og writes), decoded
 * to rows of RGB(A) bytes: enough to look at where the mark landed. */
function decodePng(png: Buffer): { width: number; height: number; channels: number; pixels: Buffer } {
  const width = png.readUInt32BE(16);
  const height = png.readUInt32BE(20);
  const [bitDepth, colourType, , , interlace] = png.subarray(24, 29);
  expect({ bitDepth, interlace }).toEqual({ bitDepth: 8, interlace: 0 });
  const channels = colourType === 6 ? 4 : colourType === 2 ? 3 : 0;
  expect(channels, `colour type ${colourType}`).toBeGreaterThan(0);
  const idat: Buffer[] = [];
  for (let at = 8; at < png.length;) {
    const length = png.readUInt32BE(at);
    if (png.toString('ascii', at + 4, at + 8) === 'IDAT') idat.push(png.subarray(at + 8, at + 8 + length));
    at += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw.readUInt8(y * (stride + 1));
    for (let i = 0; i < stride; i++) {
      const value = raw.readUInt8(y * (stride + 1) + 1 + i);
      const left = i >= channels ? pixels.readUInt8(y * stride + i - channels) : 0;
      const up = y > 0 ? pixels.readUInt8((y - 1) * stride + i) : 0;
      const upLeft = y > 0 && i >= channels ? pixels.readUInt8((y - 1) * stride + i - channels) : 0;
      let predictor = 0;
      if (filter === 1) predictor = left;
      else if (filter === 2) predictor = up;
      else if (filter === 3) predictor = (left + up) >> 1;
      else if (filter === 4) {
        const p = left + up - upLeft;
        const [pa, pb, pc] = [Math.abs(p - left), Math.abs(p - up), Math.abs(p - upLeft)];
        predictor = pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
      }
      pixels.writeUInt8((value + predictor) & 0xff, y * stride + i);
    }
  }
  return { width, height, channels, pixels };
}

/** The bounding box of the arm's yellow (#ffd43b) in a preview: nothing
 * else on the grape-and-white card is that colour. */
function yellowArmBox(png: Buffer) {
  const { width, height, channels, pixels } = decodePng(png);
  let [left, top, right, bottom, count] = [width, height, -1, -1, 0];
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const at = (y * width + x) * channels;
      if (pixels.readUInt8(at) >= 230 && pixels.readUInt8(at + 1) >= 190 && pixels.readUInt8(at + 2) <= 120) {
        count++;
        left = Math.min(left, x);
        right = Math.max(right, x);
        top = Math.min(top, y);
        bottom = Math.max(bottom, y);
      }
    }
  }
  return { width, height, count, left, top, right, bottom };
}

/** The whole arm sits inside the card with room to spare, and its box has
 * the shape's own proportions: a crop across the raised fishtail tip
 * (the bug a hand-picked viewBox once caused) makes it ~15% squatter. */
function expectWholeArm(png: Buffer) {
  const box = yellowArmBox(png);
  expect(box.count).toBeGreaterThan(200);
  const margin = 48;
  expect(box.left).toBeGreaterThanOrEqual(margin);
  expect(box.top).toBeGreaterThanOrEqual(margin);
  expect(box.right).toBeLessThan(box.width - margin);
  expect(box.bottom).toBeLessThan(box.height - margin);
  const arm = signalArmBounds();
  const drawn = (box.bottom - box.top + 1) / (box.right - box.left + 1);
  expect(drawn / (arm.height / arm.width)).toBeCloseTo(1, 1);
}

describe('the mark in a preview', () => {
  it.each([null, 'Status', 'Great Western Railway: London Paddington to Bristol Temple Meads'])(
    'shows the whole raised arm for %s',
    async (title) => {
      expectWholeArm(Buffer.from(await renderPreviewImage(title).arrayBuffer()));
    },
    30_000,
  );

  it('shows the whole raised arm in the saved site-wide image too', () => {
    // public/opengraph-image.png is renderPreviewImage(null), saved: re-save it when the template changes.
    expectWholeArm(readFileSync('public/opengraph-image.png'));
  });
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
    expect(routes.length).toBeGreaterThanOrEqual(9);
    for (const route of routes) {
      const source = readFileSync(route, 'utf8');
      expect(source, route).toContain('renderPreviewImage(');
      expect(source, route).not.toMatch(/searchParams/);
      // Its page must not list the site-wide image, or that one wins.
      const page = readFileSync(route.replace('opengraph-image.tsx', 'page.tsx'), 'utf8');
      expect(page, route).toContain('OWN_PREVIEW_IMAGE');
    }
  });
});
