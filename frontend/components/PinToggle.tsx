'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Tooltip } from '@mantine/core';
import type { Preferences } from '@/lib/types';

type PinKind = 'line' | 'station';

/** Same star glyph in both states so the shape reads as "star" either way;
 * the pinned/unpinned distinction itself comes from `fill` (none vs
 * `currentColor`) combined with the `ActionIcon`'s `variant`/`color`, which
 * is set differently per state below — fill is deliberately not the only
 * signal, since a squashed icon at small sizes can make a fill-only
 * distinction hard to see. */
function StarIcon({ filled }: { filled: boolean }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill={filled ? 'currentColor' : 'none'}
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
    </svg>
  );
}

/** Calls the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * rather than `lib/api.ts` — this is a Client Component, which cannot read
 * the server-only `API_BASE_URL` env var `lib/api.ts`'s functions rely on. */
export function PinToggle({ kind, id, initiallyPinned }: { kind: PinKind; id: string; initiallyPinned: boolean }) {
  const router = useRouter();
  const [pinned, setPinned] = useState(initiallyPinned);
  const [busy, setBusy] = useState(false);

  /** Known tradeoff: this is a full read-modify-write against the whole
   * pinned list, not a per-item mutation. Each toggle re-fetches
   * `/api/preferences`, computes the next array locally, then `PUT`s the
   * entire list back — there's no server-side reconciliation (no
   * version/ETag, the PUT is an unconditional whole-list replace). The
   * `busy` state only disables *this* button, so clicking pin A then pin B
   * in quick succession (before A's PUT lands) means B reads the list
   * before A's write is visible, and B's PUT silently drops A's change —
   * last write wins, earlier changes can be lost. Accepted for now given
   * this app's single-user, low-cardinality scale; a proper fix would need
   * per-item add/remove endpoints or optimistic state lifted to the page,
   * both out of scope for this plan. */
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

  // States both the action and the current state, per accessibility
  // review, so a screen reader user can tell pinned from unpinned without
  // relying on the icon fill (which they can't see).
  const label = pinned ? 'Unpin (currently pinned)' : 'Pin (currently not pinned)';

  return (
    <Tooltip label={label}>
      <ActionIcon
        variant={pinned ? 'filled' : 'outline'}
        // Distinct hues (not just filled vs. outline of the same yellow)
        // so pinned/unpinned don't rely on icon fill alone.
        color={pinned ? 'yellow' : 'gray'}
        onClick={toggle}
        disabled={busy}
        aria-label={label}
      >
        <StarIcon filled={pinned} />
      </ActionIcon>
    </Tooltip>
  );
}
