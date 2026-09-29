import { Alert, Stack, Text, Title } from '@mantine/core';
import { SectionTitle } from './SectionTitle';
import { TextLink } from './TextLink';
import { notFound } from 'next/navigation';
import type { ReactNode } from 'react';
import { LEGAL_CONFIG, legalPagesMode } from '@/lib/legal';

/** The gate every legal page calls first: 404 while the pages are off
 * (the default), otherwise the mode to render in. */
export function requireLegalPages(): 'preview' | 'published' {
  const mode = legalPagesMode();
  if (mode === 'off') notFound();
  return mode as 'preview' | 'published';
}

/** Max width of a long reading page: the legal pages and `/attribution`
 * (docs/style-guide.md "Page widths"). */
export const READING_PAGE_WIDTH = 760;

/** Shared shell for the draft legal pages (`/privacy`, `/terms`,
 * `/cookies`, `/contact`). DRAFT: see `lib/legal.ts` -- operator and legal
 * review required before `LEGAL_PAGES_PUBLISHED` is turned on. */
export function LegalPage({ title, draft, children }: { title: string; draft: boolean; children: ReactNode }) {
  return (
    <Stack p="lg" gap="md" maw={READING_PAGE_WIDTH}>
      <Title order={1}>{title}</Title>
      {draft && (
        <Alert color="orange" variant="light" title="Draft, not yet published" data-legal-draft>
          This page is a draft for review. It has not been checked by a lawyer, and values in double square brackets
          still need to be filled in.
        </Alert>
      )}
      <Text size="sm" c="dimmed">
        Last updated: {LEGAL_CONFIG.LAST_UPDATED}
      </Text>
      {children}
    </Stack>
  );
}

/** A titled section. `order={2}` so every page keeps a clean h1 > h2 > h3
 * outline (axe `heading-order`). */
export function LegalSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Stack gap="xs" component="section">
      <SectionTitle>{title}</SectionTitle>
      {children}
    </Stack>
  );
}

/** The contact email as a mailto link, with the address visible as text.
 * A `TextLink` like the `/privacy`, `/contact` links in the same
 * paragraphs, so every link in legal body text looks the same. */
export function ContactEmail() {
  return (
    <TextLink href={`mailto:${LEGAL_CONFIG.CONTACT_EMAIL}`} underline="always" inline>
      {LEGAL_CONFIG.CONTACT_EMAIL}
    </TextLink>
  );
}
