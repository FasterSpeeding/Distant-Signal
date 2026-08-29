'use client';

import { ActionIcon } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useEffect, useState } from 'react';

const STORAGE_KEY = 'pride-mode';

type PrideMode = 'off' | 'rainbow' | 'trans';

const NEXT_MODE: Record<PrideMode, PrideMode> = {
  off: 'rainbow',
  rainbow: 'trans',
  trans: 'off',
};

const EMOJI: Record<PrideMode, string> = {
  off: '🏳️‍🌈',
  rainbow: '🏳️‍🌈',
  trans: '🏳️‍⚧️',
};

const SPARKLES: Record<Exclude<PrideMode, 'off'>, [string, string, string]> = {
  rainbow: ['✨', '💖', '✨'],
  trans: ['🩵', '🩷', '✨'],
};

/** Reads a stored value from before this toggle grew a third state, when
 * `localStorage[STORAGE_KEY]` only ever held `'true'`/`'false'`. Anyone
 * with that old value keeps their prior on/off preference (mapped to
 * `'rainbow'`, the only flag that used to exist) rather than silently
 * resetting to `'off'`. */
function parseStoredMode(raw: string | null): PrideMode {
  if (raw === 'rainbow' || raw === 'trans' || raw === 'off') return raw;
  if (raw === 'true') return 'rainbow';
  return 'off';
}

/** Purely decorative, off by default, and entirely separate from
 * `lib/severity.ts`'s `GROUP_COLOR` map — see
 * docs/superpowers/specs/2026-08-18-grape-theme-design.md's non-goal that
 * status colour stays semantic, never decorative. This toggle only ever
 * sets `document.body.dataset.pride`, which `globals.css` uses to paint a
 * flag-striped bar above the page; it never touches a `StatusBadge` or any
 * other status-carrying colour.
 *
 * Cycles off -> rainbow -> trans -> off, the same three-state shape
 * `ThemeToggle` already uses for light/dark/auto, rather than a second
 * toggle button next to this one — one control, one flag on screen at a
 * time, no ambiguity about which wins if both were somehow on together.
 *
 * Same hydration-safety shape as `ThemeToggle`: the real preference lives
 * in `localStorage`, which isn't available during SSR, so the server (and
 * the client's first pre-mount render) always renders the "off" state and
 * the stored preference takes over only after `useMounted` flips. */
export function PrideToggle() {
  const mounted = useMounted();
  const [mode, setMode] = useState<PrideMode>('off');

  useEffect(() => {
    if (!mounted) return;
    setMode(parseStoredMode(localStorage.getItem(STORAGE_KEY)));
  }, [mounted]);

  useEffect(() => {
    if (!mounted) return;
    document.body.dataset.pride = mode;
  }, [mode, mounted]);

  const displayedMode = mounted ? mode : 'off';

  return (
    <span style={{ position: 'relative', display: 'inline-flex' }}>
      <ActionIcon
        variant="outline"
        onClick={() => setMode((prev) => {
          const next = NEXT_MODE[prev];
          localStorage.setItem(STORAGE_KEY, next);
          return next;
        })}
        aria-pressed={displayedMode !== 'off'}
        aria-label={`Pride mode: ${displayedMode}. Click to toggle.`}
      >
        {EMOJI[displayedMode]}
      </ActionIcon>
      {/* Decorative only (`aria-hidden`): a scatter of sparkles that float
       * around the button once a pride mode is on. `globals.css`'s
       * `pride-sparkle-float` keyframes handle the animation (and its own
       * `prefers-reduced-motion` fallback to a static, non-flashing
       * sparkle) — this component only decides *whether* they're in the
       * DOM at all (matching `displayedMode`'s existing hydration-safe
       * state, so the server never renders sparkles a stored "on"
       * preference wouldn't yet justify) and which three emoji to use. */}
      {displayedMode !== 'off' && (
        <span className="prideSparkles" aria-hidden="true">
          <span className="prideSparkle">{SPARKLES[displayedMode][0]}</span>
          <span className="prideSparkle">{SPARKLES[displayedMode][1]}</span>
          <span className="prideSparkle">{SPARKLES[displayedMode][2]}</span>
        </span>
      )}
    </span>
  );
}
