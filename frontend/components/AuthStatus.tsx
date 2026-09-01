import { Group, Text } from '@mantine/core';
import { LoginLink } from './LoginLink';
import { LogoutButton } from './LogoutButton';
import type { SessionInfo } from '@/lib/types';

/** Nav-bar auth control. Takes `session` as a prop (rather than fetching
 * it itself) so it stays a plain, server-renderable function — the actual
 * fetch lives in `app/layout.tsx`'s `AuthNavItem`, following the same
 * split `DataFreshnessNavItem`/`DataFreshnessInfo` already use. Only
 * `LogoutButton` needs `'use client'`; a name/email is static text once
 * it's on the page, so it stays here.
 *
 * Logged out: a plain nav link to `/api/auth/login` — a full browser
 * navigation is enough to kick off the OIDC redirect, no client JS
 * required (see `crates/api/src/routes/auth.rs`'s `login` handler).
 * Logged in: whichever of name/email is present (name preferred), or
 * "Signed in" if the OIDC provider sent neither — `SessionInfo` allows
 * both to be `null` even when `authenticated` is `true`. */
export function AuthStatus({ session }: { session: SessionInfo }) {
  if (!session.authenticated) {
    return <LoginLink>Log in</LoginLink>;
  }

  const label = session.name ?? session.email ?? 'Signed in';
  return (
    <Group gap="xs" wrap="nowrap">
      <Text size="sm" c="dimmed">
        {label}
      </Text>
      <LogoutButton />
    </Group>
  );
}
