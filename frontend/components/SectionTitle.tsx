import { Title } from '@mantine/core';
import type { ReactNode } from 'react';

/** Heading level -> rendered size for everything below a page's `<h1>`.
 *
 * Two tiers, one step smaller than the tag each time (docs/style-guide.md,
 * "Heading rules"): a section heading is an `h2` drawn at `h3` size (22px),
 * a subsection an `h3` drawn at `h4` size (18px). Before this, `order={2}`
 * headings rendered at four different sizes across the app -- unsized (26px,
 * which reads as a second page title on mobile), `h3`, `h4` and `h5` -- for
 * the same outline level. */
export const SECTION_TITLE_SIZE = { 2: 'h3', 3: 'h4', 4: 'h5' } as const;

export type SectionTitleOrder = keyof typeof SECTION_TITLE_SIZE;

/** A section (`order` 2, the default) or subsection (`order` 3/4) heading at
 * the app's standard size for that level. Pick `order` for the document
 * outline -- never skip a level -- and let the size follow from it. A small
 * bold label naming a group is still a plain `<Title order={3} size="sm">`,
 * not this. */
export function SectionTitle({
  order = 2,
  id,
  children,
}: {
  order?: SectionTitleOrder;
  id?: string;
  children: ReactNode;
}) {
  return (
    <Title order={order} size={SECTION_TITLE_SIZE[order]} id={id}>
      {children}
    </Title>
  );
}
