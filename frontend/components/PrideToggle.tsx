'use client';

import { ActionIcon } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useEffect, useState } from 'react';

const STORAGE_KEY = 'pride-mode';

/** Purely decorative, off by default, and entirely separate from
 * `lib/severity.ts`'s `GROUP_COLOR` map — see
 * docs/superpowers/specs/2026-08-18-grape-theme-design.md's non-goal that
 * status colour stays semantic, never decorative. This toggle only ever
 * sets `document.body.dataset.pride`, which `globals.css` uses to paint a
 * flag-striped bar above the page; it never touches a `StatusBadge` or any
 * other status-carrying colour.
 *
 * Same hydration-safety shape as `ThemeToggle`: the real preference lives
 * in `localStorage`, which isn't available during SSR, so the server (and
 * the client's first pre-mount render) always renders the "off" state and
 * the stored preference takes over only after `useMounted` flips. */
export function PrideToggle() {
  const mounted = useMounted();
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    if (!mounted) return;
    setEnabled(localStorage.getItem(STORAGE_KEY) === 'true');
  }, [mounted]);

  useEffect(() => {
    if (!mounted) return;
    document.body.dataset.pride = String(enabled);
  }, [enabled, mounted]);

  const displayedEnabled = mounted && enabled;

  return (
    <span style={{ position: 'relative', display: 'inline-flex' }}>
      <ActionIcon
        variant="outline"
        onClick={() => setEnabled((prev) => {
          const next = !prev;
          localStorage.setItem(STORAGE_KEY, String(next));
          return next;
        })}
        aria-pressed={displayedEnabled}
        aria-label={`Pride mode: ${displayedEnabled ? 'on' : 'off'}. Click to toggle.`}
      >
        🏳️‍🌈
      </ActionIcon>
      {/* Decorative only (`aria-hidden`): a scatter of sparkles that float
       * around the button once pride mode is on. `globals.css`'s
       * `pride-sparkle-float` keyframes handle the animation (and its own
       * `prefers-reduced-motion` fallback to a static, non-flashing
       * sparkle) — this component only decides *whether* they're in the
       * DOM at all, matching `displayedEnabled`'s existing hydration-safe
       * state so the server never renders sparkles a stored "on"
       * preference wouldn't yet justify. */}
      {displayedEnabled && (
        <span className="prideSparkles" aria-hidden="true">
          <span className="prideSparkle">✨</span>
          <span className="prideSparkle">💖</span>
          <span className="prideSparkle">✨</span>
        </span>
      )}
    </span>
  );
}
