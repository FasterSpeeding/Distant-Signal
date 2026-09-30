import Link from 'next/link';
import { Text } from '@mantine/core';
import { ExternalLinkIcon } from './ExternalLinkIcon';

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
 * body text" root cause.
 *
 * External and `mailto:` hrefs (anything with a URL scheme) render a plain
 * `<a>` rather than `next/link`: there is no client route to prefetch or
 * transition to, and the styling hooks are the same either way.
 *
 * `tone="inherit"` is for credit and licence links sitting inside dimmed
 * or otherwise coloured text (the footer's data credits, `/attribution`'s
 * licence statements): the link takes its parent's colour and font instead
 * of the anchor grape, and relies on `underline="always"` alone as its cue.
 * Everywhere else, leave the default `'anchor'`. docs/style-guide.md
 * ("TextLink") records the rule.
 *
 * `external` opens the link in a new tab (`target="_blank"`, `rel="noopener
 * noreferrer"`) and says so: a trailing aria-hidden `ExternalLinkIcon` on
 * screen and "(opens in a new tab)" in the accessible name, never a
 * literal "↗" in the text. An explicit `target`/`rel` still wins. */
export function TextLink({
  href,
  children,
  underline = 'hover',
  inline = false,
  tone = 'anchor',
  size,
  lh,
  target,
  rel,
  prefetch,
  onClick,
  onKeyDown,
  title,
  ariaLabel,
  external = false,
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
  // Task 3.4.8 (AllLinesTable's Name column): a wrapped two-line link at
  // Mantine `Text`'s default line-height read as two separate stacked
  // items rather than one wrapped name at narrow widths. Optional --
  // `undefined` keeps every existing call site at Mantine's own default,
  // unchanged.
  lh?: number | string;
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
  // The full destination, for a call site whose visible text is a shortened
  // stand-in for it (station-accessibility rich text's raw-URL-as-link-text
  // fix, review §3.5.9: the link reads "nationalrail.co.uk" on screen but
  // still discloses the exact URL on hover/focus).
  title?: string;
  // An explicit accessible name, for one of N identically-worded links in
  // a list -- "History" on every card in `/operators`' grid, where the
  // distinguishing text (the operator's name) is a sibling element and so
  // is NOT part of the link's own name. Screen-reader users listing links
  // then hear N indistinguishable items (2026-09-22 UX review, I26/P3;
  // WCAG 2.4.9). Left `undefined` everywhere the visible text is already
  // unique on its page, which is nearly every call site -- an `aria-label`
  // that merely restates the visible text is noise, and one that
  // *contradicts* it is a 2.5.3 Label-in-Name failure, so the label passed
  // here must always CONTAIN the visible text.
  //
  // Two independent UX-review findings landed on this same prop: the
  // `/operators` grid's repeated "History" links (above) and `/lines/[id]`'s
  // repeated "View live status" links, one per train row. Both are the same
  // shape -- N identically-worded links whose distinguishing text is always
  // a sibling `<Text>` -- so they share one prop rather than two.
  ariaLabel?: string;
  tone?: 'anchor' | 'inherit';
  external?: boolean;
}) {
  const inheritTone = tone === 'inherit';
  const text = (
    <Text
      c={inheritTone ? 'inherit' : ANCHOR_COLOR}
      inherit={inheritTone}
      component={inline ? 'span' : undefined}
      size={size}
      lh={lh}
    >
      {children}
      {external && <ExternalLinkIcon />}
    </Text>
  );
  // The undecorated resting state comes from the stylesheet rather than
  // the `style={{ textDecoration: 'none' }}` these call sites used to
  // carry: an inline style outranks every selector, so a hover rule
  // would never have got a look in.
  const anchorProps = {
    href,
    'data-text-link': underline,
    'data-text-link-tone': inheritTone ? 'inherit' : undefined,
    target: target ?? (external ? '_blank' : undefined),
    rel: rel ?? (external ? 'noopener noreferrer' : undefined),
    onClick,
    onKeyDown,
    title,
    'aria-label': ariaLabel,
  };
  if (HAS_URL_SCHEME.test(href)) {
    return <a {...anchorProps}>{text}</a>;
  }
  return (
    <Link {...anchorProps} prefetch={prefetch}>
      {text}
    </Link>
  );
}

/** `https:`, `mailto:`, `tel:` and the like -- not a path or `#fragment`. */
const HAS_URL_SCHEME = /^[a-z][a-z\d+.-]*:/i;

/** The default link colour. `AppNavBar`'s auth Suspense fallback must match
 * it (`AppNavBar.test.tsx` compares the two in source). */
const ANCHOR_COLOR = 'var(--mantine-color-anchor)';
