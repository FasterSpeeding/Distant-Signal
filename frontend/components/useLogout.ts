'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';

/** The log-out action, as a hook rather than as a component — the
 * counterpart to `useLoginHref.ts`, and for the same reason: the control
 * that offers it is a `Menu.Item` inside `AccountMenu`, not a standalone
 * `Button`, so the behaviour has to be separable from any one piece of
 * chrome. (It previously lived in a `LogoutButton` component, removed
 * when the account menu became the nav's only log-out surface.)
 *
 * Posts to the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * rather than `lib/api.ts` — this runs in the browser and cannot reach the
 * `api` service directly (same reasoning as `PinToggle`/
 * `DeleteLineButton`). `/auth/logout` is documented as idempotent even
 * with no session, so this doesn't need to branch on the response status
 * before refreshing — either way the session cookie is gone (or was
 * already gone) once the request completes, so `router.refresh()` in
 * `finally` re-renders the nav's server-side session check regardless. */
export function useLogout() {
  const router = useRouter();
  const [loggingOut, setLoggingOut] = useState(false);

  async function logout() {
    setLoggingOut(true);
    try {
      await fetch('/api/auth/logout', { method: 'POST' });
    } catch {
      // Swallowed deliberately, not ignored: every caller wires this
      // straight to an `onClick`, so a rejection here is a promise
      // nothing is awaiting -- an unhandled rejection in the console
      // rather than anything a visitor can see or act on. There is also
      // nothing useful to do with it, because the `finally` below is
      // already the whole response: refresh, and let the server say
      // whether the session survived.
    } finally {
      setLoggingOut(false);
      router.refresh();
    }
  }

  return { logout, loggingOut };
}
