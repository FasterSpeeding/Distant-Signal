import type { ComponentPropsWithoutRef, ReactNode } from 'react';
import Link from 'next/link';
import { Card, type MantineSpacing } from '@mantine/core';
import { tokens } from '@/lib/theme';

/** Spread onto the one element that is a clickable card's own target when
 * it isn't `CardTitleLink` or `CardTitleButton`: a Mantine `Button` that
 * acts for the whole card (the alternatives sheet's "Take this one"), or a
 * `Radio`'s input on a selectable card (`selectableCardRadioProps`). */
export const CARD_PRIMARY = { 'data-card-primary': true } as const;

/** A box the whole of which is clickable (docs/style-guide.md, "Clickable
 * cards"): a journey, train or line row, a status card, a planner option.
 *
 * The card is a plain `Card` holding one primary target, a real `<a>`
 * (`CardTitleLink`) or `<button>` (`CardTitleButton`) around the card's
 * title. That target's `::after` is stretched over the whole card
 * (`[data-card-primary]` in `app/globals.css`), so a click anywhere on the
 * card follows it, while its accessible name stays the title alone rather
 * than every word in the box. Any other control inside (Follow, a menu, a
 * remove button, a text link) is lifted above the stretched area by the
 * same stylesheet, so it works on its own and never nests in the link.
 *
 * The stylesheet also gives the card `cursor: pointer`, a hover state (the
 * link-colour border, the hover background and, in the light scheme, a
 * deeper shadow), a pressed state and the app's focus ring round the card
 * while its target has keyboard focus. Motion only under
 * `prefers-reduced-motion: no-preference`.
 *
 * No `'use client'`: a Server Component can render it with a link, and a
 * Client Component with a button. */
export function ClickableCard({
  children,
  padding,
  fill = false,
  selected,
  className,
  ...rest
}: {
  children: ReactNode;
  padding?: MantineSpacing | number | undefined;
  /** Stretch to the grid cell's height, so cards in one row line up
   * (`StatusCard`, `StatusTile`). */
  fill?: boolean;
  /** A selectable card's state (`ItineraryOption`): draws the selected
   * border. Leave unset on a card that navigates. */
  selected?: boolean | undefined;
  className?: string | undefined;
} & Record<`data-${string}`, string | number | boolean | undefined>) {
  const classes = [fill ? 'ds-column' : undefined, className].filter((name) => name !== undefined);
  return (
    <Card
      {...tokens.card.link}
      {...(padding !== undefined && { padding })}
      {...(fill && { h: '100%' })}
      {...(classes.length > 0 && { className: classes.join(' ') })}
      data-clickable-card
      data-selected={selected === true ? true : undefined}
      {...rest}
    >
      {children}
    </Card>
  );
}

/** A clickable card's target when it navigates: an `<a>` (Next `Link`)
 * around the card's title, stretched over the card. `ariaLabel` only for a
 * card whose title doesn't name where it goes on its own (a `/status`
 * tile is a count and a label). */
export function CardTitleLink({
  href,
  children,
  ariaLabel,
  className,
}: {
  href: string;
  children: ReactNode;
  ariaLabel?: string | undefined;
  className?: string | undefined;
}) {
  return (
    <Link href={href} data-card-primary aria-label={ariaLabel} className={className}>
      {children}
    </Link>
  );
}

/** A clickable card's target when it acts in place (picks a departure): a
 * `<button type="button">` around the card's title, stretched over the
 * card. It renders as text, not as a button, since the whole card is the
 * control. */
export function CardTitleButton({ children, ...props }: Omit<ComponentPropsWithoutRef<'button'>, 'type'>) {
  return (
    <button {...props} type="button" data-card-primary data-card-title-button>
      {children}
    </button>
  );
}

/** Props for the `Radio` of a selectable card: its input is the card's
 * target and its label is stretched over the card, so a click anywhere on
 * the card selects it. */
export const selectableCardRadioProps = {
  ...CARD_PRIMARY,
  classNames: { label: 'ds-card-stretch' },
} as const;

/** The same box as a `ClickableCard`, flat and not clickable
 * (`tokens.card.static`): for a card of the same list that has nowhere to
 * go or can't be picked right now (a shared train with no uid, a cancelled
 * departure, a planner route with no train), so it doesn't promise a
 * click. */
export function StaticCard({
  children,
  padding,
  ...rest
}: {
  children: ReactNode;
  padding?: MantineSpacing | number | undefined;
} & Record<`data-${string}`, string | number | boolean | undefined>) {
  return (
    <Card {...tokens.card.static} {...(padding !== undefined && { padding })} {...rest}>
      {children}
    </Card>
  );
}
