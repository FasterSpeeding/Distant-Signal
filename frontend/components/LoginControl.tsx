'use client';

import { Suspense, type ReactNode } from 'react';
import Link from 'next/link';
import { Button, Stack } from '@mantine/core';
import { useLoginHref } from './useLoginHref';
import { TextLink } from './TextLink';
import { LoginConsentNote } from './LoginConsentNote';

type TextSize = 'xs' | 'sm' | 'md' | 'lg' | 'xl';

/** The app's one login control: a link to the backend's login redirect
 * carrying a `return_to` for the current page (`useLoginHref`), so
 * `GET /auth/callback` (`crates/api/src/routes/auth.rs`) sends the visitor
 * back here.
 *
 * - `variant="link"` (the default) is a `TextLink`: "Log in to delete this
 *   ticket" under a failed action, or "Log in" in the nav bar.
 * - `variant="button"` is a filled `Button` rendered as the same link, for
 *   a page whose one anonymous action is logging in (`/groups/join`,
 *   `/groups`, `/account`), so it carries the weight the logged-in
 *   equivalent's button does. It is followed by the LEG-1 consent note
 *   unless `consentNote={false}` (`LoginPromptModal` places its own).
 *
 * `prefetch={false}` is required, not decorative: the href is not a page
 * but `crates/api/src/routes/auth.rs`'s `login` handler, which writes a
 * `login_state` row and overwrites the `login_state` cookie. Next's
 * viewport prefetch would fire it for every anonymous page view and race a
 * real login click's state.
 *
 * A Client Component because `useLoginHref` reads `usePathname()` and
 * `useSearchParams()`. The button variant isolates that read behind its
 * own `<Suspense>` so a statically prerendered page still builds (the
 * fallback is the same button, disabled). A URL fragment can't round-trip
 * through the OIDC redirect; that is a known limitation. */
export function LoginControl({
  children,
  variant = 'link',
  underline,
  size,
  title,
  consentNote = true,
}: {
  children: ReactNode;
  variant?: 'link' | 'button' | undefined;
  /** Link variant only: `TextLink`'s underline. */
  underline?: 'hover' | 'always' | undefined;
  /** Link variant only: `TextLink`'s text size. */
  size?: TextSize | undefined;
  /** Button variant only: a native tooltip, for a button whose own text
   * doesn't say it needs an account. */
  title?: string | undefined;
  /** Button variant only: render the LEG-1 consent note under it. */
  consentNote?: boolean | undefined;
}) {
  if (variant === 'link') {
    return (
      <LoginTextLink underline={underline} size={size}>
        {children}
      </LoginTextLink>
    );
  }
  const button = (
    <Suspense
      fallback={
        <Button disabled title={title}>
          {children}
        </Button>
      }
    >
      <LoginButtonLink title={title}>{children}</LoginButtonLink>
    </Suspense>
  );
  if (!consentNote) return button;
  return (
    <Stack gap={4}>
      {button}
      {/* LEG-1: renders nothing until the legal pages are published. */}
      <LoginConsentNote />
    </Stack>
  );
}

function LoginTextLink({
  children,
  underline,
  size,
}: {
  children: ReactNode;
  underline: 'hover' | 'always' | undefined;
  size: TextSize | undefined;
}) {
  const href = useLoginHref();
  return (
    <TextLink href={href} underline={underline} prefetch={false} size={size}>
      {children}
    </TextLink>
  );
}

function LoginButtonLink({ children, title }: { children: ReactNode; title: string | undefined }) {
  const href = useLoginHref();
  return (
    <Button component={Link} href={href} prefetch={false} title={title}>
      {children}
    </Button>
  );
}
