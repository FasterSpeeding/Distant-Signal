'use client';

import { useState } from 'react';
import { LoginPromptModal } from '@/components/LoginPromptModal';

/** `/track/mine`'s own page must stay a Server Component (it directly
 * awaits `getMyTrackedTrains()`/`getMyTickets()`), so it can't hold the
 * `useState` a controlled `LoginPromptModal` needs itself -- this small
 * sibling Client Component is the wrapper. See
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
 * Decision 6.
 *
 * Starts open (there is no click event on a Server Component to open it
 * from) and stays fully controlled to match `LoginPromptModal`'s own
 * contract exactly, the same as `DeleteLineButton`/`DeleteTrainButton`'s
 * controlled-`Modal` convention. Closing it (Escape, backdrop, or its own
 * close control) leaves the page showing just its heading -- a deliberate,
 * accepted simplification (Decision 6's Open Question 1), not a gap this
 * component tries to close. A fresh navigation back to `/track/mine`
 * reopens it, since `opened` re-initializes to `true` on every fresh
 * mount. */
export function AutoOpenLoginPrompt({ children }: { children: React.ReactNode }) {
  const [opened, setOpened] = useState(true);
  return (
    <LoginPromptModal opened={opened} onClose={() => setOpened(false)}>
      {children}
    </LoginPromptModal>
  );
}
