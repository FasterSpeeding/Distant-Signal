'use client';

import { NativeSelect } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useEffect, useState } from 'react';
import {
  PRIDE_MODE_OPTIONS,
  PRIDE_MODE_STORAGE_KEY,
  parseStoredMode,
  readStoredPrideMode,
  type PrideMode,
} from '@/lib/prideMode';

/** Applies the saved pride mode (`document.body.dataset.pride`, painted by
 * `globals.css`) on every page without showing a control. Mounted by the
 * nav. The root layout's `PRIDE_MODE_SCRIPT` (`lib/prideMode.ts`) has
 * normally set it already, before the first paint; this is the fallback
 * for a page served without that script's nonce. Decorative only, off by
 * default, and never touches a status colour
 * (docs/superpowers/specs/2026-08-18-grape-theme-design.md). */
export function PrideModeApplier(): null {
  const mounted = useMounted();
  useEffect(() => {
    if (!mounted) return;
    document.body.dataset.pride = readStoredPrideMode();
  }, [mounted]);
  return null;
}

/** The pride flag picker on /account/preferences. Saved in this browser
 * under the same key `PrideModeApplier` reads, and applied straight away.
 * Shows "Off" until mounted so the server and first client render agree. */
export function PrideModeSelect() {
  const mounted = useMounted();
  const [mode, setMode] = useState<PrideMode>('off');

  useEffect(() => {
    if (!mounted) return;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- reads localStorage only after mount, so the server and first client render agree
    setMode(readStoredPrideMode());
  }, [mounted]);

  const displayedMode = mounted ? mode : 'off';

  function choose(next: PrideMode) {
    setMode(next);
    document.body.dataset.pride = next;
    try {
      localStorage.setItem(PRIDE_MODE_STORAGE_KEY, next);
    } catch {
      // Storage blocked: the flag still shows for this visit.
    }
  }

  return (
    <NativeSelect
      label="Pride flag"
      description="A flag stripe along the top of every page."
      data={PRIDE_MODE_OPTIONS.map((option) => ({ value: option.value, label: option.label }))}
      value={displayedMode}
      onChange={(event) => choose(parseStoredMode(event.currentTarget.value))}
      leftSection={<span className="prideSwatch" data-flag={displayedMode} aria-hidden="true" />}
      maw={320}
    />
  );
}
