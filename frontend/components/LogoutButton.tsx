'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button } from '@mantine/core';

/** Posts to the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * rather than `lib/api.ts` — this is a Client Component and cannot reach
 * the `api` service directly (same reasoning as `PinToggle`/
 * `DeleteLineButton`). `/auth/logout` is documented as idempotent even
 * with no session, so this doesn't need to branch on the response status
 * before refreshing — either way the session cookie is gone (or was
 * already gone) once the request completes, so `router.refresh()` in
 * `finally` re-renders the nav's server-side session check regardless. */
export function LogoutButton() {
  const router = useRouter();
  const [busy, setBusy] = useState(false);

  async function handleLogout() {
    setBusy(true);
    try {
      await fetch('/api/auth/logout', { method: 'POST' });
    } finally {
      setBusy(false);
      router.refresh();
    }
  }

  return (
    <Button variant="subtle" size="xs" onClick={handleLogout} loading={busy}>
      Log out
    </Button>
  );
}
