'use client';

import { NativeSelect } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useEffect, useState } from 'react';

const STORAGE_KEY = 'pride-mode';

export type PrideMode =
  'off' | 'rainbow' | 'trans' | 'nonbinary' | 'bisexual' | 'pansexual' | 'asexual' | 'sapphic' | 'lesbian';

/** The picker's options, in the order the old nav toggle cycled through
 * them: the umbrella rainbow and trans flags first, the gender-identity
 * flags together, then the attraction flags. */
export const PRIDE_MODE_OPTIONS: readonly { value: PrideMode; label: string }[] = [
  { value: 'off', label: 'Off' },
  { value: 'rainbow', label: 'Rainbow' },
  { value: 'trans', label: 'Trans' },
  { value: 'nonbinary', label: 'Non-binary' },
  { value: 'bisexual', label: 'Bisexual' },
  { value: 'pansexual', label: 'Pansexual' },
  { value: 'asexual', label: 'Asexual' },
  { value: 'sapphic', label: 'Sapphic' },
  { value: 'lesbian', label: 'Lesbian' },
];

const STORED_MODES = new Set<string>(PRIDE_MODE_OPTIONS.map((option) => option.value));

/** Reads the saved mode. `'true'` is the value from before the toggle had
 * more than one flag, and still means the rainbow. */
export function parseStoredMode(raw: string | null): PrideMode {
  if (raw !== null && STORED_MODES.has(raw)) return raw as PrideMode;
  if (raw === 'true') return 'rainbow';
  return 'off';
}

function readStoredMode(): PrideMode {
  try {
    return parseStoredMode(localStorage.getItem(STORAGE_KEY));
  } catch {
    return 'off';
  }
}

/** Applies the saved pride mode (`document.body.dataset.pride`, painted by
 * `globals.css`) on every page without showing a control. Mounted by the
 * nav. Decorative only, off by default, and never touches a status colour
 * (docs/superpowers/specs/2026-08-18-grape-theme-design.md). */
export function PrideModeApplier(): null {
  const mounted = useMounted();
  useEffect(() => {
    if (!mounted) return;
    document.body.dataset.pride = readStoredMode();
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
    setMode(readStoredMode());
  }, [mounted]);

  const displayedMode = mounted ? mode : 'off';

  function choose(next: PrideMode) {
    setMode(next);
    document.body.dataset.pride = next;
    try {
      localStorage.setItem(STORAGE_KEY, next);
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
