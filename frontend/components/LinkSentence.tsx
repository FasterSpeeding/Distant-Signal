import { Fragment, type ReactNode } from 'react';
import { Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export interface SentenceLink {
  href: string;
  label: string;
  /** Said after the link, in brackets: "(Manchester Metrolink)". */
  note?: string | null | undefined;
}

/** A related-links panel as one dimmed sentence (docs/style-guide.md,
 * "Link sentences"): `lead`, then the links joined with commas and "and",
 * ending in a full stop. "Change for trams at Piccadilly (Manchester
 * Metrolink)." "National Rail stations here: London King's Cross and St
 * Pancras International." For links that lead off the page's subject, so
 * they sit under its main content rather than competing with it as cards.
 * Nothing when there are no links. */
export function LinkSentence({
  lead,
  links,
  children,
}: {
  /** The words before the links, with their own trailing punctuation or
   * space ("Also here: ", "Change for trams at "). */
  lead: ReactNode;
  links: readonly SentenceLink[];
  /** Anything after the sentence, inside the same paragraph. */
  children?: ReactNode;
}) {
  if (links.length === 0) return null;
  return (
    <Text size="sm" c="dimmed" data-link-sentence>
      {lead}
      {links.map((link, index) => (
        <Fragment key={`${link.href}-${index}`}>
          <TextLink href={link.href} inline underline="always">
            {link.label}
          </TextLink>
          {link.note ? ` (${link.note})` : ''}
          {index < links.length - 2 ? ', ' : index === links.length - 2 ? ' and ' : '.'}
        </Fragment>
      ))}
      {children}
    </Text>
  );
}
