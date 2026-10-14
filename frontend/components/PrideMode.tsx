'use client';

import { Group, NativeSelect } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useEffect, useState } from 'react';
import {
  PRIDE_MODE_OPTIONS,
  PRIDE_MODE_STORAGE_KEY,
  PRIDE_SPARKLES,
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
    <Group gap="lg" align="flex-end" wrap="nowrap">
      <NativeSelect
        label="Pride flag"
        description="A flag stripe and sparkles at the top of every page."
        data={PRIDE_MODE_OPTIONS.map((option) => ({ value: option.value, label: option.label }))}
        value={displayedMode}
        onChange={(event) => choose(parseStoredMode(event.currentTarget.value))}
        leftSection={<span className="prideSwatch" data-flag={displayedMode} aria-hidden="true" />}
        maw={320}
        flex={1}
      />
      <PridePreview mode={displayedMode} />
    </Group>
  );
}

/** A live preview beside the picker, in the old nav toggle's shape: the
 * flag's swatch in a round badge with its spinning ring and sparkles.
 * Decorative (`aria-hidden`); the select carries the choice. */
function PridePreview({ mode }: { mode: PrideMode }) {
  return (
    <span className="prideToggle" aria-hidden="true">
      <span className="pridePreview" data-flag={mode}>
        <span className="prideSwatch" data-flag={mode} />
      </span>
      <SparkleScatter mode={mode} />
    </span>
  );
}

/** The scatter itself: three emoji that float around their anchor
 * (`globals.css`'s `pride-sparkle-float`; static under reduced motion).
 * Decorative (`aria-hidden`). The preview renders it only while a flag is
 * on; the nav's copies carry `data-flag` and are shown by CSS. */
function SparkleScatter({ mode, flagged = false }: { mode: PrideMode; flagged?: boolean }) {
  if (mode === 'off') return null;
  const [first, second, third] = PRIDE_SPARKLES[mode];
  return (
    <span className="prideSparkles" aria-hidden="true" data-flag={flagged ? mode : undefined}>
      <span className="prideSparkle">{first}</span>
      <span className="prideSparkle">{second}</span>
      <span className="prideSparkle">{third}</span>
    </span>
  );
}

/** The decorative sparkle scatter on every page, around the wordmark in
 * the nav (`AppNavBar`), restored from the old nav toggle when the pride
 * controls moved to /account/preferences. Every flag's trio is in the
 * server HTML, hidden, and `globals.css` shows the one matching
 * `body[data-pride]`: so, like the stripe, it is there on the first paint
 * for anyone with a flag saved, logged in or out, and follows the picker
 * the moment it changes. */
export function PrideSparkles() {
  return (
    <>
      {PRIDE_MODE_OPTIONS.map(({ value }) => (
        <SparkleScatter key={value} mode={value} flagged />
      ))}
    </>
  );
}
