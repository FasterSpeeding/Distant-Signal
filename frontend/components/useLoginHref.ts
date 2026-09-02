'use client';

import { usePathname, useSearchParams } from 'next/navigation';

/** Builds the `/api/auth/login?return_to=...` href from the current page's
 * path + query string. Extracted out of `LoginLink.tsx` (see that file's own
 * doc comment on why a URL fragment can never be captured this way) so
 * `LoginPromptModal` doesn't duplicate or reimplement this three-line
 * calculation — see
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md Decision 1. */
export function useLoginHref(): string {
  const pathname = usePathname();
  const search = useSearchParams().toString();
  const returnTo = search ? `${pathname}?${search}` : pathname;
  return `/api/auth/login?return_to=${encodeURIComponent(returnTo)}`;
}
