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
 * `:hover`/`:focus-visible` can't be expressed as a style object.
 *
 * `inline` renders the wrapped `Text` as a `<span>` instead of Mantine's
 * default `<p>`. Leave it `false` (the default) for a positional link --
 * a nav item, an action beside a heading, a name in a table column --
 * where the surrounding layout already expects a block-ish element. Set
 * it `true` for a link sitting mid-sentence in a paragraph: a `<p>` there
 * forces everything after it onto its own line, breaking the sentence
 * across three lines instead of one. Pair `inline` with
 * `underline="always"` at those call sites -- the two problems (missing
 * non-colour cue, broken sentence flow) share the same "this link sits in
 * body text" root cause. */
export function TextLink({
  href,
  children,
  underline = 'hover',
  inline = false,
  size,
  target,
  rel,
  prefetch,
  onClick,
  onKeyDown,
}: {
  href: string;
  children: React.ReactNode;
  underline?: 'hover' | 'always';
  inline?: boolean;
  // Left `undefined` by default -- Mantine's own `Text` default (`md`,
  // 16px) is what every existing call site was already implicitly getting,
  // so adding this prop must not change any of them. `AuthStatus`'s
  // `LoginLink` is the one call site that now sets this explicitly, to
  // `'sm'` (14px) -- the size the shared chrome's other text-link-styled
  // controls converge on (review §2.16 "auth controls are inconsistently
  // sized"): `sm` is Mantine's own font-size floor before `xs` (12px, which
  // review §2.16 also flags as too small on the footer's own NationalRail
  // link), and it is already the size Mantine's `Menu.Item` renders "Log
  // out" at by default (`node_modules/@mantine/core/styles/Menu.css`'s
  // `--mantine-font-size-sm` on `.mantine-Menu-item`) -- so `sm` is not a
  // new convention invented here, it is the one the rest of the chrome
  // already landed on.
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  target?: string;
  rel?: string;
  // Passed straight through to `next/link`'s own `prefetch` prop.
  // Deliberately omitted (left `undefined`, i.e. Next's own default) for
  // every ordinary in-app page link -- only `LoginLink` overrides this to
  // `false`. See `LoginLink.tsx`'s own doc comment for why: its href is
  // never a real page, so letting Next prefetch it fires a real,
  // side-effecting request with no user interaction at all.
  prefetch?: boolean;
  // Optional escape hatches for a `TextLink` nested inside its own
  // separately-clickable/keyboard-handled container -- e.g. a
  // `role="button"` picker row in `TrackTrainForm.tsx` whose own
  // `onClick`/`onKeyDown` would otherwise also fire when this link is
  // activated (a click bubbles to the row; a `keydown` on the anchor
  // bubbles too, ahead of the browser's own synthesized click). A call
  // site in that position passes a handler that calls
  // `event.stopPropagation()` before letting the link behave normally. No
  // call site needs these outside that situation, so both are optional
  // and every existing use is unaffected.
  onClick?: (event: React.MouseEvent<HTMLAnchorElement>) => void;
  onKeyDown?: (event: React.KeyboardEvent<HTMLAnchorElement>) => void;
}) {
  return (
    // The undecorated resting state comes from the stylesheet rather than
    // the `style={{ textDecoration: 'none' }}` these call sites used to
    // carry: an inline style outranks every selector, so a hover rule
    // would never have got a look in.
    <Link
      href={href}
      data-text-link={underline}
      target={target}
      rel={rel}
      prefetch={prefetch}
      onClick={onClick}
      onKeyDown={onKeyDown}
    >
      <Text c="var(--mantine-color-anchor)" component={inline ? 'span' : undefined} size={size}>
        {children}
      </Text>
    </Link>
  );
}
