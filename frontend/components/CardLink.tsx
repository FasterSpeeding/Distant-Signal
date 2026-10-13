import type { ReactNode } from 'react';
import Link from 'next/link';
import { Card, type MantineSpacing } from '@mantine/core';
import { tokens } from '@/lib/theme';

/** A whole card that is one link: a home-page line, station or train
 * card, a `/groups` row, a `/status` tile.
 *
 * The `<a>` carries `data-card-link` and the `Card` `data-card`; the rules
 * in `app/globals.css` keep the card's text its own colour with no
 * underline, change the border to the link colour on hover and keyboard
 * focus, and draw the focus ring on the card rather than the anchor. The
 * card takes `tokens.card.link` (border plus the `sm` shadow that marks a
 * clickable card).
 *
 * A heading link inside a card whose other parts hold their own controls
 * (a row with a rename button, say) can't be one big link, since an `<a>`
 * can't contain other interactive content. Put `data-card-link` on a plain
 * `<Link>` around just the heading instead: it gets the same colour and
 * underline reset and a visible focus ring.
 *
 * A plain `<Link>` around the `Card`, not `component={Link}` on the card:
 * this renders from Server Components, which can't pass `Link` as a prop
 * value to a Client Component. No `'use client'` of its own for the same
 * reason. */
export function CardLink({
  href,
  children,
  padding,
  ariaLabel,
}: {
  href: string;
  children: ReactNode;
  padding?: MantineSpacing | undefined;
  /** For a card whose visible text doesn't name the destination on its
   * own (a `/status` tile is just a count and a label). */
  ariaLabel?: string | undefined;
}) {
  return (
    <Link href={href} data-card-link aria-label={ariaLabel}>
      <Card {...tokens.card.link} {...(padding !== undefined && { padding })} data-card>
        {children}
      </Card>
    </Link>
  );
}
