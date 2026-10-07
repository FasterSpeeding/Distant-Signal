import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { DEFAULT_THEME } from '@mantine/core';

// public/offline.html is static (the service worker serves it with no
// connection and no React runtime), so its colours are inlined hex rather
// than read from the theme. These tests are what keeps them honest: each
// token must equal the Mantine value (plus app/globals.css override) the
// app itself renders with -- docs/style-guide.md "Surface and text tokens".
const html = readFileSync('public/offline.html', 'utf8');
const { colors, white, black } = DEFAULT_THEME;

function block(selector: string): Record<string, string> {
  const start = html.indexOf(selector);
  expect(start, `missing ${selector}`).toBeGreaterThanOrEqual(0);
  const body = html.slice(html.indexOf('{', start) + 1, html.indexOf('}', start));
  return Object.fromEntries(
    [...body.matchAll(/(--ds-[a-z-]+):\s*([^;]+);/g)].map(([, name, value]) => [name, value!.trim().toLowerCase()]),
  );
}

/** Mantine writes some shades as 3-digit or uppercase hex. */
function hex(value: string): string {
  const v = value.toLowerCase();
  return v.length === 4 ? `#${v[1]}${v[1]}${v[2]}${v[2]}${v[3]}${v[3]}` : v;
}

const LIGHT = {
  '--ds-body': hex(white),
  '--ds-text': hex(black),
  '--ds-dimmed': colors.gray[7],
  '--ds-border': colors.gray[3],
  '--ds-filled': colors.grape[7],
  '--ds-filled-hover': colors.grape[8],
  '--ds-anchor': colors.grape[7],
  '--ds-wash': colors.grape[6],
};

const DARK = {
  '--ds-body': colors.dark[7],
  '--ds-text': colors.dark[0],
  '--ds-dimmed': colors.dark[1],
  '--ds-border': colors.dark[4],
  '--ds-filled': colors.grape[8],
  '--ds-filled-hover': colors.grape[9],
  '--ds-anchor': colors.grape[4],
};

function lower(tokens: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(tokens).map(([k, v]) => [k, hex(v)]));
}

describe('offline.html palette', () => {
  it('uses the Mantine light tokens and globals.css overrides', () => {
    expect(block(':root {')).toEqual(lower(LIGHT));
  });

  it('uses the Mantine dark tokens for both the OS preference and an explicit dark choice', () => {
    expect(block("html:not([data-mantine-color-scheme='light']) {")).toEqual(lower(DARK));
    expect(block("html[data-mantine-color-scheme='dark'] {")).toEqual(lower(DARK));
  });

  it('has no other colour literals outside the token blocks', () => {
    const style = html.slice(html.indexOf('<style>'), html.indexOf('</style>'));
    const allowed = new Set([...Object.values(lower(LIGHT)), ...Object.values(lower(DARK)), '#ffffff']);
    for (const [literal] of style.matchAll(/#[0-9a-fA-F]{3,8}\b/g)) {
      expect(allowed, `off-palette colour ${literal}`).toContain(hex(literal));
    }
  });

  it('uses the default 8px radius on its button', () => {
    expect(html).toMatch(/button \{[^}]*border-radius: 8px;/);
  });

  it('loads nothing from the network, so it still renders offline', () => {
    expect(html).not.toMatch(/\b(src|href)=["']https?:/);
    expect(html).not.toMatch(/@import|url\(/);
  });

  it('has the brand bar but no navigation landmark (e2e/service-worker.spec.ts)', () => {
    expect(html).toContain('class="site-header"');
    expect(html).toContain('>Distant Signal<');
    expect(html).not.toMatch(/<nav\b/);
  });
});

describe('offline.html last-connected line', () => {
  const lastScript = (): string => {
    const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(([, body]) => body!);
    return scripts[scripts.length - 1]!;
  };

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-09-29T08:44:30Z'));
    document.body.innerHTML =
      '<p id="offline-message">Distant Signal needs a connection to show current line status.</p>';
  });

  afterEach(() => {
    vi.useRealTimers();
    localStorage.clear();
  });

  it('says "4m ago", like the app\'s relative times, with the exact UK time in the tooltip', () => {
    localStorage.setItem('lastSuccessfulLoadAt', '2026-09-29T08:40:00Z');
    // eslint-disable-next-line @typescript-eslint/no-implied-eval -- runs offline.html's own inline script, the code under test
    new Function(lastScript())();
    const message = document.getElementById('offline-message')!;
    expect(message.textContent).toBe(
      'Distant Signal needs a connection to show current line status. Last connected 4m ago.',
    );
    expect(message.title).toBe('Last connected 29 Sept 2026, 09:40 (UK time)');
  });

  it('uses hours and days for older loads', () => {
    localStorage.setItem('lastSuccessfulLoadAt', '2026-09-29T05:40:00Z');
    // eslint-disable-next-line @typescript-eslint/no-implied-eval -- runs offline.html's own inline script, the code under test
    new Function(lastScript())();
    expect(document.getElementById('offline-message')!.textContent).toContain('Last connected 3h ago.');
  });

  it('leaves the base message alone when nothing was ever loaded', () => {
    // eslint-disable-next-line @typescript-eslint/no-implied-eval -- runs offline.html's own inline script, the code under test
    new Function(lastScript())();
    expect(document.getElementById('offline-message')!.textContent).toBe(
      'Distant Signal needs a connection to show current line status.',
    );
  });
});
