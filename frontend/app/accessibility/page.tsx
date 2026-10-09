import { List, ListItem, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { ContactEmail, LegalPage, LegalSection, requireLegalPages } from '@/components/LegalPage';
import { TextLink } from '@/components/TextLink';
import { LEGAL_CONFIG, legalPageMetadata } from '@/lib/legal';
import { withAbsoluteTitle } from '@/lib/pageMetadata';

// ============================================================================
// DRAFT -- REVIEW BEFORE PUBLISHING. The accessibility statement (LEG-15),
// behind the same LEGAL_PAGES_PUBLISHED gate as the other legal pages. The
// Public Sector Bodies Accessibility Regulations don't apply to a private
// service, but the Equality Act 2010 duty to make reasonable adjustments
// does, and a statement plus a contact route is the usual way to meet it.
// Every claim below must stay true: the testing section describes
// e2e/accessibility.spec.ts and app/globals.test.ts, and the limitations
// section describes that spec's WAIVED rules.
// ============================================================================

// Reads the legal-pages flags per request (see lib/legal.ts).
export const dynamic = 'force-dynamic';

export function generateMetadata(): Metadata {
  return withAbsoluteTitle(
    legalPageMetadata(
      'Accessibility statement',
      'How accessible Distant Signal is, known limitations, and how to ask for help.',
    ),
  );
}

export default function AccessibilityPage() {
  const mode = requireLegalPages();
  return (
    <LegalPage draft={mode === 'preview'} title="Accessibility statement">
      <Text>
        Distant Signal is run by {LEGAL_CONFIG.OPERATOR_NAME}. We want everyone to be able to use it, including people
        who use a screen reader, a keyboard only, magnification or high-contrast settings.
      </Text>

      <LegalSection title="Our target">
        <Text>
          We aim to meet the Web Content Accessibility Guidelines (WCAG) 2.2 at level AA. We have not had the site
          audited by an independent expert, so we don&apos;t claim full conformance.
        </Text>
      </LegalSection>

      <LegalSection title="How we test">
        <List size="sm" spacing={4}>
          <ListItem>
            Every change is checked by an automated accessibility scan (axe) of the site&apos;s pages, in both light and
            dark colour schemes, before it is released.
          </ListItem>
          <ListItem>Our colour palette is checked automatically against the WCAG AA contrast ratios.</ListItem>
          <ListItem>Keyboard use of drop-down lists, menus and dialogs is covered by automated tests.</ListItem>
        </List>
        <Text>
          Automated checks can&apos;t find every problem. We have not yet tested the site with assistive technology
          users.
        </Text>
      </LegalSection>

      <LegalSection title="Known limitations">
        <List size="sm" spacing={4}>
          <ListItem>
            Open drop-down lists and the account menu are placed outside the page&apos;s main landmark regions. You can
            still reach and use them with a keyboard or screen reader from the control that opens them.
          </ListItem>
          <ListItem>
            The delay and punctuality charts on line history pages don&apos;t yet have a text or table equivalent. Email
            us and we will send you the figures.
          </ListItem>
          <ListItem>
            Information from rail operators and National Rail (such as incident descriptions) is shown as they wrote it,
            and may not always be in plain English.
          </ListItem>
        </List>
      </LegalSection>

      <LegalSection title="Tell us about a problem">
        <Text>
          If something is hard to use, or you need information in a different format, email <ContactEmail />. Tell us
          the page and what went wrong, and we will reply within 10 working days. You can also use the{' '}
          <TextLink href="/contact" underline="always" inline>
            contact page
          </TextLink>
          .
        </Text>
      </LegalSection>
    </LegalPage>
  );
}
