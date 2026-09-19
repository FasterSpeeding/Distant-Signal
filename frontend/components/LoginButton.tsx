'use client';

import { Suspense } from 'react';
import Link from 'next/link';
import { Button } from '@mantine/core';
import { useLoginHref } from './useLoginHref';

/** Isolates the one `useLoginHref()` call behind its own `<Suspense>`
 * boundary -- same reasoning as `LoginPromptModal.tsx`'s own internal
 * `LoginButtonLink`/`<Suspense>` pair (see that file's doc comment for the
 * `next build` static-prerendering failure this works around). Exported
 * here, rather than duplicated a third time, because review §2.16's
 * anonymous-CTA promotion (see `LoginButton`'s own doc comment below) needs
 * this same shape at three more call sites
 * (`app/groups/join/[token]/page.tsx`, `app/connect-claude/page.tsx`,
 * `app/groups/page.tsx`) -- all Server Components, none of which can call
 * `useLoginHref` directly. */
function LoginButtonLink({ children, title }: { children: React.ReactNode; title?: string }) {
  const href = useLoginHref();
  return (
    <Link href={href} style={{ textDecoration: 'none' }} prefetch={false}>
      <Button title={title}>{children}</Button>
    </Link>
  );
}

/** Filled-button counterpart to `LoginLink` (see that component's own doc
 * comment for the shared `return_to`/`prefetch={false}` reasoning, which
 * applies here unchanged -- this wraps the exact same href).
 *
 * Review §2.16's design decision: on `/groups/join/[token]`,
 * `/connect-claude` and `/groups`, the anonymous call-to-action was an
 * underlined text link (`LoginLink`) sitting next to -- or in place of --
 * an authenticated equivalent rendered as a filled, full-width `Button`
 * (`JoinGroupButton`, `CustomLineForm`'s and `TrackTrainForm`'s own submit
 * buttons). A `Button` here, not a differently-styled `TextLink`, so the
 * anonymous visitor's one action on the page reads with the same visual
 * weight the logged-in equivalent gets -- consistent with the "show the
 * control to everyone" Tier-2 pattern `useNeedsLogin.ts` already documents
 * for a control whose backing request needs a session. Dropped into a
 * `Stack` (every call site's own layout), it stretches to the Stack's full
 * width the same way `JoinGroupButton`'s plain `<Button>` already does --
 * no `fullWidth` prop needed for that half of the parity.
 *
 * `title`, optional: a native-tooltip hint that the action needs an
 * account, for a call site whose own button text doesn't already say "Log
 * in" (mirrors the `title="Pin -- needs an account"` fix review §3.4
 * recommends for the anonymous pin star, `PinToggle.tsx` -- that specific
 * control is unchanged by this task, out of its stated blast radius, but
 * the wording convention is reused here for the new call sites this task
 * does own). */
export function LoginButton({ children, title }: { children: React.ReactNode; title?: string }) {
  return (
    <Suspense fallback={<Button disabled title={title}>{children}</Button>}>
      <LoginButtonLink title={title}>{children}</LoginButtonLink>
    </Suspense>
  );
}
