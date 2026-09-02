import Link from 'next/link';
import { Text } from '@mantine/core';

/** The app's in-page text link.
 *
 * Plain `<Link>` wrapping Mantine's `Text`, not `component={Link}` on a
 * Mantine polymorphic prop: nearly every call site is a Server Component,
 * and passing the `Link` component reference into a Mantine `component`
 * prop from a Server Component previously broke `next build`'s
 * Server/Client boundary serialization check (see the comment in
 * `app/layout.tsx`). This component has no `'use client'` of its own for
 * the same reason — it must stay renderable on the server.
 *
 * `underline` controls the non-colour affordance. `'hover'` (the default)
 * suits links whose position already identifies them — nav items, a
 * right-aligned action beside a section heading, a name in a table
 * column of names. `'always'` is for a link sitting in the ordinary flow
 * of body text, where colour would otherwise be the only thing marking it
 * (WCAG 1.4.1). Both underline on `:focus-visible`, so keyboard users get
 * the cue either way. The rules themselves are in `app/globals.css`;
 * `:hover`/`:focus-visible` can't be expressed as a style object. */
export function TextLink({
  href,
  children,
  underline = 'hover',
  target,
  rel,
  prefetch,
}: {
  href: string;
  children: React.ReactNode;
  underline?: 'hover' | 'always';
  target?: string;
  rel?: string;
  // Passed straight through to `next/link`'s own `prefetch` prop.
  // Deliberately omitted (left `undefined`, i.e. Next's own default) for
  // every ordinary in-app page link -- only `LoginLink` overrides this to
  // `false`. See `LoginLink.tsx`'s own doc comment for why: its href is
  // never a real page, so letting Next prefetch it fires a real,
  // side-effecting request with no user interaction at all.
  prefetch?: boolean;
}) {
  return (
    // The undecorated resting state comes from the stylesheet rather than
    // the `style={{ textDecoration: 'none' }}` these call sites used to
    // carry: an inline style outranks every selector, so a hover rule
    // would never have got a look in.
    <Link href={href} data-text-link={underline} target={target} rel={rel} prefetch={prefetch}>
      <Text c="var(--mantine-color-anchor)">{children}</Text>
    </Link>
  );
}
