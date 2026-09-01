'use client';

import { useState } from 'react';

/** Shared client-side "show a control to everyone, prompt on the real
 * 401" state (§Policy Tier 2 /
 * docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md
 * §Reusable pattern). `PinToggle.tsx`, `TrackTrainForm.tsx`, and
 * (pre-extraction) `CustomLineForm.tsx`/`DeleteLineButton.tsx` each
 * hand-rolled this same three-line shape independently -- once already
 * slightly differently (CustomLineForm/DeleteLineButton's original,
 * pre-fix version had no needsLogin handling at all). This hook exists
 * purely so the shape can't drift again, not because the previous
 * hand-rolled versions were broken.
 *
 * Deliberately minimal: does not wrap the fetch call itself (each call
 * site's request shape differs too much -- a DELETE, a POST with a JSON
 * body, a PUT against a whole list -- to usefully share that part). Just
 * the state a caller resets at the start of every attempt and sets when a
 * response comes back 401. */
export function useNeedsLogin() {
  const [needsLogin, setNeedsLogin] = useState(false);

  return {
    needsLogin,
    /** Call at the start of every fresh attempt, before the request. */
    reset: () => setNeedsLogin(false),
    /** Call when a response comes back 401. */
    markNeedsLogin: () => setNeedsLogin(true),
  };
}
