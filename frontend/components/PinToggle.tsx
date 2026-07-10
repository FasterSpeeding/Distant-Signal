'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon } from '@mantine/core';
import type { Preferences } from '@/lib/types';

type PinKind = 'line' | 'station';

/** Calls the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * rather than `lib/api.ts` — this is a Client Component, which cannot read
 * the server-only `API_BASE_URL` env var `lib/api.ts`'s functions rely on. */
export function PinToggle({ kind, id, initiallyPinned }: { kind: PinKind; id: string; initiallyPinned: boolean }) {
  const router = useRouter();
  const [pinned, setPinned] = useState(initiallyPinned);
  const [busy, setBusy] = useState(false);

  async function toggle() {
    setBusy(true);
    try {
      const prefsResponse = await fetch('/api/preferences');
      const prefs: Preferences = await prefsResponse.json();
      const key = kind === 'line' ? 'pinnedLines' : 'pinnedStations';
      const endpoint = kind === 'line' ? '/api/preferences/pinned-lines' : '/api/preferences/pinned-stations';
      const current = prefs[key];
      const next = pinned ? current.filter((existing) => existing !== id) : [...current, id];
      await fetch(endpoint, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(next),
      });
      setPinned(!pinned);
      router.refresh();
    } finally {
      setBusy(false);
    }
  }

  return (
    <ActionIcon
      variant={pinned ? 'filled' : 'outline'}
      color="yellow"
      onClick={toggle}
      disabled={busy}
      aria-label={pinned ? 'Unpin' : 'Pin'}
    >
      {pinned ? '★' : '☆'}
    </ActionIcon>
  );
}
