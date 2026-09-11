'use client';

import { useState } from 'react';
import { LoginPromptModal } from '@/components/LoginPromptModal';

/** Colocated copy of `app/track/mine/AutoOpenLoginPrompt.tsx` for the
 * `/groups` list page -- same "a Server Component can't hold the
 * `useState` a controlled `LoginPromptModal` needs" reasoning, kept
 * page-local rather than shared cross-directory, matching this codebase's
 * existing colocation convention for this exact component. */
export function AutoOpenLoginPrompt({ children }: { children: React.ReactNode }) {
  const [opened, setOpened] = useState(true);
  return (
    <LoginPromptModal opened={opened} onClose={() => setOpened(false)}>
      {children}
    </LoginPromptModal>
  );
}
