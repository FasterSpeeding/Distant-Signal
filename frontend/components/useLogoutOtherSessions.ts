'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';

/** "Log out everywhere else" -- the user-reachable half of the M14/L6 fix
 * (2026-09-26 Repeater Signal review): before this, authz data (`groups`)
 * was frozen at login for the whole `session_ttl_days` session lifetime
 * with no way to end a session early, and there was no way at all for a
 * visitor to end their OTHER sessions (a lost/stolen device, a shared
 * computer, or simply "I don't recognise that login") short of waiting out
 * every other session's own TTL.
 *
 * Posts to `POST /api/auth/sessions/revoke-others` -- same same-origin
 * `/api/*` proxy every other browser-initiated mutation in this app goes
 * through (see `useLogout`'s own doc comment for why: this runs in the
 * browser and cannot reach the `api` service directly). Unlike `logout`,
 * a successful response carries a NEW session cookie for this same browser
 * (the backend reissues one so the visitor triggering this isn't logged
 * out of their own request along with every other session) -- so this
 * calls `router.refresh()` on success to pick up the fresh cookie's
 * server-rendered state, but deliberately does NOT swallow a failure the
 * way `useLogout` does: unlike logout (idempotent, and "the cookie is
 * gone either way" regardless of the request's outcome), a failed request
 * here means nothing was actually revoked, which is worth surfacing
 * rather than silently refreshing as if it had worked. */
export function useLogoutOtherSessions() {
  const router = useRouter();
  const [loggingOut, setLoggingOut] = useState(false);
  const [error, setError] = useState(false);

  async function logoutOtherSessions() {
    setLoggingOut(true);
    setError(false);
    try {
      const response = await fetch('/api/auth/sessions/revoke-others', { method: 'POST' });
      if (!response.ok) {
        setError(true);
        return;
      }
      router.refresh();
    } catch {
      setError(true);
    } finally {
      setLoggingOut(false);
    }
  }

  return { logoutOtherSessions, loggingOut, error };
}
