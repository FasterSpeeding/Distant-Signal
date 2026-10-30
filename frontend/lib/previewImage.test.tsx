// @vitest-environment node
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { inflateSync } from 'node:zlib';
import { DEFAULT_THEME } from '@mantine/core';
import { describe, expect, it } from 'vitest';
import { signalArmBounds } from './brand';
import { incidentPreviewCard, incidentStatusCard } from './incidentPreview';
import {
  fitLineChips,
  STATUS_BAND_COLORS,
  cardTitleFontSize,
  previewTitleFontSize,
  previewTitleFromMetadata,
  renderStatusPreviewImage,
  renderPreviewImage,
} from './previewImage';
import { linePreviewCard } from './linePreview';
import type { StatusPreviewCard } from './livePreview';
import { stationPreviewCard } from './stationPreview';
import { trainPreviewCard } from './trainPreview';
import {
  CANCELLED_TRAIN,
  DELAYED_TRAIN,
  ON_TIME_TRAIN,
  PLANNED_TRAIN,
  TRAIN_PREVIEW_NOW,
} from '@/test/trainPreviewFixtures';
import type { IncidentDetail, LineStatus, LineStatusReport, SampleStats } from './types';

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
    expect(routes.length).toBeGreaterThanOrEqual(7);
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

/** Sample live cards a reviewer can look at as PNGs
 * (`test/fixtures/preview-cards/`): three incidents (a live disruption, a
 * cleared one with no lines left, and upcoming planned work with a very
 * long summary and more lines than fit), a disrupted line, a station, and
 * four made-up trains (`test/trainPreviewFixtures.ts`: on time, late,
 * cancelled, and tomorrow's, not yet running).
 * Fixed `now`, so they render the same every time. Regenerate with
 * `UPDATE_PREVIEW_CARDS=1 npx vitest run lib/previewImage.test.tsx`. */
const PREVIEW_NOW = Date.parse('2026-10-30T14:00:00Z');
const PREVIEW_TOCS = new Map([
  ['VT', 'Avanti West Coast'],
  ['AW', 'Transport for Wales'],
  ['SN', 'Southern'],
  ['NT', 'Northern'],
]);
function previewIncident(overrides: Partial<IncidentDetail>): IncidentDetail {
  return {
    incidentId: 'FIXTURE',
    summary: '',
    description: '',
    operators: [],
    affectedStations: [],
    priority: 2,
    validityPeriods: [],
    isPlanned: false,
    isCleared: false,
    firstSeenAt: '2026-10-30T13:53:09Z',
    fetchedAt: '2026-10-30T13:58:10Z',
    sourceRemovedAt: null,
    currentlyAffectsLines: [],
    history: [],
    ...overrides,
  };
}
const INCIDENT_PREVIEWS: [name: string, incident: IncidentDetail][] = [
  [
    'live-disruption',
    previewIncident({
      summary: 'Disruption between Chester and Llandudno Junction expected until 15:00',
      operators: ['VT', 'AW'],
      state: 'active',
      validityPeriods: [{ fromDate: '2026-10-30T13:50:00Z', toDate: null, isNow: true }],
      currentlyAffectsLines: [
        { id: 'tfw-north-wales-coast', name: 'North Wales Coast Line' },
        { id: 'wcml-north-wales', name: 'West Coast Main Line: North Wales' },
      ],
    }),
  ],
  [
    'cleared-no-lines',
    previewIncident({
      summary: 'CLEARED: Delays between Haywards Heath and Lewes ',
      operators: ['SN'],
      isCleared: true,
      state: 'cleared',
      validityEndedAt: '2026-10-30T12:56:00Z',
      validityPeriods: [{ fromDate: '2026-10-30T12:20:00Z', toDate: '2026-10-30T12:56:00Z', isNow: false }],
    }),
  ],
  [
    'upcoming-long',
    previewIncident({
      summary:
        'Buses replace late night trains between Manchester Victoria / Salford Crescent and Wigan Wallgate via Atherton, Walkden, Hindley and Ince from Monday 25 to Thursday 28 January, with some journeys extended to Southport and Kirkby',
      operators: ['NT'],
      isPlanned: true,
      state: 'upcoming',
      startsAt: '2027-01-25T00:00:00Z',
      validityPeriods: [{ fromDate: '2027-01-25T00:00:00Z', toDate: '2027-01-28T05:00:00Z', isNow: false }],
      currentlyAffectsLines: [
        {
          id: 'northern-manchester-southport',
          name: 'Northern: Manchester to Southport via Wigan Wallgate and Atherton',
        },
        { id: 'northern-wigan-kirkby', name: 'Northern: Wigan to Kirkby' },
        { id: 'northern', name: 'Northern' },
        { id: 'northern-blackpool', name: 'Northern: Blackpool' },
        { id: 'northern-clitheroe', name: 'Northern: Clitheroe' },
      ],
    }),
  ],
];
function previewLineStatus(severity: number, reason: string, sampleStats?: SampleStats): LineStatus {
  return {
    statusSeverity: severity,
    statusSeverityDescription: '',
    reason,
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'available' },
    fullCoverageAvailability: { state: 'not-enabled' },
    ...(sampleStats ? { sampleStats } : {}),
  };
}
function previewLineReport(id: string, name: string, statuses: LineStatus[]): LineStatusReport {
  return {
    $type: 'Line',
    id,
    name,
    modeName: 'national-rail',
    operators: ['SE', 'TL'],
    lineStatuses: statuses,
    computedAt: '2026-10-30T14:03:00Z',
  };
}
/** Every sample card: the incidents, a disrupted line, a station and the trains. */
const PREVIEW_CARDS: [name: string, card: StatusPreviewCard][] = [
  ...INCIDENT_PREVIEWS.map(([name, incident]): [string, StatusPreviewCard] => [
    `incident-${name}`,
    incidentStatusCard(incidentPreviewCard(incident, { operatorNames: PREVIEW_TOCS, now: PREVIEW_NOW })),
  ]),
  [
    'line-minor-delays',
    linePreviewCard(
      'Chatham Main Line',
      previewLineReport('southeastern-chatham', 'Chatham Main Line', [
        previewLineStatus(9, 'Disruption through Rochester expected until 15:00', {
          total: 42,
          delayed: 5,
          cancelled: 1,
          skipped: 0,
          avgDelayMinutes: 3.2,
        }),
        previewLineStatus(5, 'Engineering works between Rainham and Gillingham'),
      ]),
      ['SE', 'TL'],
      new Map([
        ['SE', 'Southeastern'],
        ['TL', 'Thameslink'],
      ]),
    ),
  ],
  [
    'station-severe',
    stationPreviewCard(
      'Lewes (LWS)',
      {
        coverage: 'covered',
        reports: [
          previewLineReport('southern-brighton-main-line', 'Brighton Main Line', [
            previewLineStatus(6, 'Signalling fault at Lewes: trains may be cancelled or delayed by up to 30 minutes'),
          ]),
          previewLineReport('southern-coastway-east', 'Coastway East', [previewLineStatus(9, 'Minor delays')]),
          previewLineReport('southern-seaford-branch', 'Seaford Branch', [previewLineStatus(9, 'Minor delays')]),
        ],
      },
      PREVIEW_NOW,
    ),
  ],
  ['train-on-time', trainPreviewCard(ON_TIME_TRAIN, TRAIN_PREVIEW_NOW)],
  ['train-delayed', trainPreviewCard(DELAYED_TRAIN, TRAIN_PREVIEW_NOW)],
  ['train-cancelled', trainPreviewCard(CANCELLED_TRAIN, TRAIN_PREVIEW_NOW)],
  ['train-not-yet-running', trainPreviewCard(PLANNED_TRAIN, TRAIN_PREVIEW_NOW)],
];
const PREVIEW_FIXTURES = 'test/fixtures/preview-cards';

describe('renderStatusPreviewImage', () => {
  it.each(PREVIEW_CARDS)(
    'renders %s as a 1200x630 PNG under its coloured band',
    async (name, card) => {
      const response = renderStatusPreviewImage(card, {
        headers: { 'Cache-Control': 'public, max-age=60' },
      });
      expect(response.headers.get('content-type')).toBe('image/png');
      expect(response.headers.get('cache-control')).toBe('public, max-age=60');
      const png = Buffer.from(await response.arrayBuffer());
      const { width, height, channels, pixels } = decodePng(png);
      expect({ width, height }).toEqual({ width: 1200, height: 630 });
      // The band's top-left corner is the state's fill.
      const at = (8 * width + 8) * channels;
      const hex = `#${[0, 1, 2]
        .map((c) =>
          pixels
            .readUInt8(at + c)
            .toString(16)
            .padStart(2, '0'),
        )
        .join('')}`;
      expect(hex).toBe(STATUS_BAND_COLORS[card.band.tone].fill);
      const fixture = join(PREVIEW_FIXTURES, `${name}.png`);
      if (process.env.UPDATE_PREVIEW_CARDS) {
        mkdirSync(PREVIEW_FIXTURES, { recursive: true });
        writeFileSync(fixture, png);
      }
      // Saved for review, not compared byte for byte: only its size is checked.
      const saved = readFileSync(fixture);
      expect([saved.readUInt32BE(16), saved.readUInt32BE(20)]).toEqual([1200, 630]);
    },
    30_000,
  );

  it('shows the line chips that fit whole and counts the rest', () => {
    expect(fitLineChips(['North Wales Coast Line', 'West Coast Main Line: North Wales'], 0)).toEqual({
      names: ['North Wales Coast Line', 'West Coast Main Line: North Wales'],
      more: 0,
    });
    const long = 'Northern: Manchester to Southport via Wigan Wallgate and Atherton';
    expect(fitLineChips([long, 'Northern: Wigan to Kirkby', 'Northern'], 2)).toEqual({
      names: [long, 'Northern: Wigan to Kirkby'],
      more: 3,
    });
    // The first always shows, however long.
    expect(fitLineChips(['x'.repeat(200)], 0)).toEqual({ names: ['x'.repeat(200)], more: 0 });
    expect(fitLineChips([], 0)).toEqual({ names: [], more: 0 });
  });

  it('steps the title down for longer summaries', () => {
    expect(cardTitleFontSize('x'.repeat(40))).toBeGreaterThan(cardTitleFontSize('x'.repeat(80)));
    expect(cardTitleFontSize('x'.repeat(80))).toBeGreaterThan(cardTitleFontSize('x'.repeat(200)));
  });
});

describe('the incident band colours', () => {
  const css = readFileSync('app/globals.css', 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  const light = css.slice(css.indexOf("html:root[data-mantine-color-scheme='light']"));
  const palette = (value: string): string => {
    const shade = /^var\(--mantine-color-([a-z]+)-(\d)\)$/.exec(value);
    if (shade) return DEFAULT_THEME.colors[shade[1]!]![Number(shade[2])]!;
    if (value === 'var(--mantine-color-white)') return '#ffffff';
    if (value === 'var(--mantine-color-black)') return '#000000';
    return value;
  };
  const luminance = (hex: string) => {
    const [r, g, b] = [1, 3, 5].map((i) => {
      const c = parseInt(hex.slice(i, i + 2), 16) / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
  };

  /** A delay band takes `DelayBadge`'s hue: the light pill's own text colour. */
  const DELAY_HUES: Record<string, string> = { late: 'orange', early: 'teal' };

  it.each(Object.entries(STATUS_BAND_COLORS))('%s is the light badge pair, at 4.5:1 or more', (tone, pair) => {
    const value = (name: string) => palette(new RegExp(`${name}:\\s*([^;]+);`).exec(light)![1]!.trim()).toLowerCase();
    const hue = DELAY_HUES[tone];
    if (hue) {
      expect(pair).toEqual({ fill: value(`--mantine-color-${hue}-light-color`), text: '#ffffff' });
    } else {
      expect(pair).toEqual({
        fill: value(`--ds-status-badge-${tone}-fill`),
        text: value(`--ds-status-badge-${tone}-text`),
      });
    }
    const [hi, lo] = [luminance(pair.fill), luminance(pair.text)].sort((a, b) => b - a);
    expect((hi! + 0.05) / (lo! + 0.05)).toBeGreaterThanOrEqual(4.5);
  });
});

describe('the live preview routes', () => {
  const ROUTES: [route: string, read: string][] = [
    ['app/incidents/[id]/card.png/route.tsx', 'getPreviewIncident('],
    ['app/lines/[id]/card.png/route.tsx', 'getPreviewLineStatus('],
    ['app/stations/[crs]/card.png/route.tsx', 'getPreviewStopPointDisruption('],
    ['app/train/[uid]/[date]/card.png/route.tsx', 'getPreviewTrain('],
  ];

  it.each(ROUTES)('%s draws from preview reads only, never from the request', (route, read) => {
    const source = readFileSync(route, 'utf8').replace(/\/\/.*$/gm, '');
    expect(source).toContain('renderStatusPreviewImage(');
    expect(source).toContain(read);
    // A cookie, a header or a dynamic read would make the route dynamic
    // (re-rendered per hit) and split the shared data cache per requester.
    expect(source).not.toMatch(/searchParams|request\.|nextUrl|cookies\(|headers\(/);
    expect(source).not.toMatch(/import \{[^}]*\bget(?!Preview)[^}]*\} from '@\/lib\/api'/);
    expect(source).toMatch(/export const revalidate = 60;/);
    expect(source).toContain('LIVE_PREVIEW_CACHE_CONTROL');
    // Its page lists it, not a file-convention image that would win over it.
    expect(existsSync(route.replace('card.png/route.tsx', 'opengraph-image.tsx'))).toBe(false);
    const page = readFileSync(route.replace('card.png/route.tsx', 'page.tsx'), 'utf8');
    expect(page).toMatch(/image: \w+PreviewImage\(/);
    expect(page).not.toContain('OWN_PREVIEW_IMAGE');
  });

  it('gives a session-only tracked-train page no card: it keeps the site-wide image', () => {
    // `/train/by-id/{trackingId}` is one person's subscription (a custom
    // name, tickets), readable only with their session; its public form is
    // `/train/{uid}/{date}`, which it redirects to once matched. A chat app
    // unfurls anonymously, so a card here could only ever show the login
    // prompt, and must never show what the owner sees.
    const dir = 'app/train/by-id/[trackingId]';
    expect(existsSync(join(dir, 'card.png'))).toBe(false);
    expect(existsSync(join(dir, 'opengraph-image.tsx'))).toBe(false);
    const page = readFileSync(join(dir, 'page.tsx'), 'utf8');
    expect(page).not.toMatch(/PreviewImage|OWN_PREVIEW_IMAGE|getPreview/);
  });
});
