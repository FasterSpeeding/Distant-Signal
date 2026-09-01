'use client';

import { useComputedColorScheme } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useEffect } from 'react';

/** Side-effect-only component (renders nothing), mounted once in the root
 * layout alongside AutoRefresh — keeps the <meta name="color-scheme"> tag
 * Next's `viewport` export renders at SSR time (see app/layout.tsx) in
 * sync with the app's actually-resolved theme after every client-side
 * change. Dark Reader's own maintainer named this single-value tag,
 * specifically, as the current opt-out signal it checks (see
 * docs/superpowers/specs/2026-09-01-dynamic-color-scheme-meta-design.md).
 *
 * Same useMounted()-gated imperative-DOM-mutation shape PrideToggle.tsx
 * already uses for document.body.dataset.pride, and the same
 * useComputedColorScheme('light') hook/fallback ThemeToggle.tsx already
 * uses — no new hook, no new gating pattern, no new fallback constant.
 *
 * Deliberately never renders the tag in this component's own JSX: doing so
 * would fight the tag Next's `viewport` export already renders at the same
 * <head> position, and would reintroduce the hydration-mismatch bug class
 * already fixed in ThemeToggle/PrideToggle/LastUpdated. Because this only
 * runs inside a useEffect body gated on mounted, it never fires before
 * hydration completes and never runs at all during SSR. */
export function ColorSchemeMeta() {
  const computedColorScheme = useComputedColorScheme('light');
  const mounted = useMounted();

  useEffect(() => {
    if (!mounted) return;
    let tag = document.querySelector('meta[name="color-scheme"]');
    if (!tag) {
      tag = document.createElement('meta');
      tag.setAttribute('name', 'color-scheme');
      document.head.appendChild(tag);
    }
    tag.setAttribute('content', computedColorScheme);
  }, [mounted, computedColorScheme]);

  return null;
}
