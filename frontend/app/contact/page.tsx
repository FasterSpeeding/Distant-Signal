import { List, ListItem, Text } from '@mantine/core';
import type { Metadata } from 'next';
import { ContactEmail, LegalPage, LegalSection, requireLegalPages } from '@/components/LegalPage';
import { TextLink } from '@/components/TextLink';
import { ACCOUNT_ROUTE, LEGAL_CONFIG, legalPageMetadata } from '@/lib/legal';

// ============================================================================
// DRAFT -- REVIEW BEFORE PUBLISHING. The operator contact point (LEG-2) and
// the Online Safety Act 2023 reporting and complaints route (LEG-12,
// ss. 20-21). The inbox in LEGAL_CONFIG.CONTACT_EMAIL must be monitored.
// ============================================================================

// Reads the legal-pages flags per request (see lib/legal.ts).
export const dynamic = 'force-dynamic';

export function generateMetadata(): Metadata {
  return legalPageMetadata('Contact', 'How to contact Distant Signal, report content or make a privacy request.');
}

export default function ContactPage() {
  const mode = requireLegalPages();
  return (
    <LegalPage draft={mode === 'preview'} title="Contact">
      <Text>
        Distant Signal is run by {LEGAL_CONFIG.OPERATOR_NAME}. Email <ContactEmail /> for any of the following.
      </Text>

      <LegalSection title="Report content">
        <Text>
          To report content you think is illegal or breaks our{' '}
          <TextLink href="/terms" underline="always" inline>
            terms
          </TextLink>
          , such as a group name, a shared journey or a display name, email us with where it is and what the problem is.
          The same address handles complaints about how we dealt with a report.
        </Text>
      </LegalSection>

      <LegalSection title="Privacy requests">
        <List size="sm" spacing={4}>
          <ListItem>
            You can download or delete your data yourself on your{' '}
            <TextLink href={ACCOUNT_ROUTE} underline="always" inline>
              account page
            </TextLink>
            .
          </ListItem>
          <ListItem>
            For any other request, see the{' '}
            <TextLink href="/privacy" underline="always" inline>
              privacy notice
            </TextLink>{' '}
            and email us.
          </ListItem>
        </List>
      </LegalSection>

      <LegalSection title="Accessibility and everything else">
        <Text>
          If something on the site is hard to use, or you have any other question, email us and tell us what you need.
          See also our{' '}
          <TextLink href="/accessibility" underline="always" inline>
            accessibility statement
          </TextLink>
          .
        </Text>
      </LegalSection>
    </LegalPage>
  );
}
