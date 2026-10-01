import { List, ListItem, Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { ContactEmail, LegalPage, LegalSection, requireLegalPages } from '@/components/LegalPage';
import { TextLink } from '@/components/TextLink';
import {
  ACCOUNT_DELETE_LABEL,
  ACCOUNT_EXPORT_LABEL,
  ACCOUNT_ROUTE,
  describeRetentionDays,
  LEGAL_CONFIG,
  legalPageMetadata,
  retentionPolicy,
  type RetentionPolicy,
} from '@/lib/legal';

// ============================================================================
// DRAFT -- NOT LEGAL ADVICE -- REVIEW BEFORE PUBLISHING.
// Drafted from the 2026-09-27 UK legal compliance gap analysis (LEG-1, LEG-3,
// LEG-4, LEG-5, LEG-7, LEG-8, LEG-11, LEG-28 and "Lawful basis by data
// category"). The operator, and ideally a lawyer, must check every statement
// against how the service actually runs before setting LEGAL_PAGES_PUBLISHED.
// Retention periods below follow the api's actual retention settings, which
// the chart passes to the frontend (lib/legal.ts `retentionPolicy`): tracked
// trains, tickets and journeys after the travel date, push subscriptions
// after a long absence, and -- only once enabled (DQ7: 730 days, after this
// notice is published) -- whole inactive accounts.
// ============================================================================

// Reads the legal-pages flags per request (see lib/legal.ts).
export const dynamic = 'force-dynamic';

export function generateMetadata(): Metadata {
  return legalPageMetadata(
    'Privacy notice',
    'What personal data Distant Signal holds, why, for how long, and your rights.',
  );
}

interface DataCategory {
  title: string;
  what: string;
  why: string;
  basis: string;
  retention: string;
}

const CONTRACT = 'Contract (UK GDPR Art. 6(1)(b)): we need it to provide the service you asked for.';
const LEGITIMATE_INTERESTS = 'Legitimate interests (UK GDPR Art. 6(1)(f)): keeping the service secure and working.';

/** One entry per row of the audit's "Lawful basis by data category" table.
 * Built per request from the live retention settings. */
function dataCategories(retention: RetentionPolicy): readonly DataCategory[] {
  const travel =
    retention.pastTravelDays > 0
      ? `, or ${describeRetentionDays(retention.pastTravelDays)} after the travel date, whichever comes first`
      : '';
  return [
    {
      title: 'Your account',
      what: 'The ID our sign-in service gives you, your display name and username, the access groups needed to decide which features you can use, and when you signed up and last signed in.',
      why: 'To create and run your account.',
      basis: CONTRACT,
      retention:
        retention.inactiveAccountDays > 0
          ? `Until you delete your account. If you do not sign in for ${describeRetentionDays(retention.inactiveAccountDays)} (and have no active session), we delete your account and everything in it automatically. We do not hold your email address, so we cannot warn you first.`
          : 'Until you delete your account.',
    },
    {
      title: 'Sign-in sessions',
      what: 'A hashed session token and its expiry, and short-lived sign-in state (a one-time code verifier and security values).',
      why: 'To keep you signed in, and to protect sign-in from forgery.',
      basis: `${CONTRACT} ${LEGITIMATE_INTERESTS}`,
      retention: 'Sessions expire after 14 days and are then deleted. Sign-in state is deleted after 15 minutes.',
    },
    {
      title: 'Saved lines, stations, operators and custom lines',
      what: 'The lines, stations and operators you pin, and any custom lines you create.',
      why: 'To show you the things you chose to follow.',
      basis: CONTRACT,
      retention: 'Until you remove them or delete your account.',
    },
    {
      title: 'Tracked trains, journeys and journey templates',
      what: 'The date, origin, destination and operator of trains and journeys you track, and any names you give them.',
      why: 'To track your trains and journeys and tell you about delays.',
      basis: CONTRACT,
      retention: `Tracked trains and journeys: until you delete them or delete your account${travel}. Journey templates: until you delete them or delete your account.`,
    },
    {
      title: 'Tickets',
      what: 'Ticket details you add: operator, ticket type, origin and destination. Ticket files you upload (PDF, Apple Wallet pass or zip) are read in memory to extract these details and are never stored.',
      why: 'To attach tickets to your trains and estimate Delay Repay.',
      basis: CONTRACT,
      retention: `Until you delete them or delete your account${travel}. Uploaded files are not kept at all.`,
    },
    {
      title: 'Groups',
      what: 'Groups you create or join, your role in them, and the trains, journeys and custom lines you share. Other members see your display name and what you share. They never see your email address.',
      why: 'To let you share travel plans with people you invite.',
      basis: CONTRACT,
      retention: 'For as long as you are a member, or until the group or your account is deleted.',
    },
    {
      title: 'Share and invite links',
      what: 'Links you create to share a journey or invite people to a group. Anyone who has a share link can view what it shares.',
      why: 'To let you share with people who do not have an account.',
      basis: CONTRACT,
      retention: 'Deleted 30 days after you revoke them or they expire.',
    },
    {
      title: 'Push notifications',
      what: 'If you turn on notifications: the address your browser gives us for sending them, its encryption keys, and when it was last used.',
      why: 'To send you the notifications you asked for.',
      basis: CONTRACT,
      retention: `Until you turn notifications off, your browser’s push service tells us the address no longer works, you have more than 20 devices registered (the oldest is removed)${
        retention.stalePushSubscriptionDays > 0
          ? `, you have not signed in (and your browser has not renewed the address) for ${describeRetentionDays(retention.stalePushSubscriptionDays)}`
          : ''
      }, or you delete your account.`,
    },
    {
      title: 'Your location',
      what: 'If you use "near me", your browser sends your approximate location (rounded to about 100 metres) so we can find the nearest stations.',
      why: 'To find stations near you.',
      basis: CONTRACT,
      retention: 'Not stored. It can appear briefly in our server logs (see below).',
    },
    {
      title: 'Technical logs and IP addresses',
      what: 'Your IP address, the pages you request and when, handled by our network provider (Cloudflare), our servers and our sign-in service.',
      why: 'To keep the service secure, prevent abuse and fix faults.',
      basis: LEGITIMATE_INTERESTS,
      retention: `Server logs are rotated within days. Our sign-in service keeps its event logs for ${LEGAL_CONFIG.SSO_LOG_RETENTION}. Cloudflare keeps its logs under its own policy.`,
    },
    {
      title: 'Connections to our file-transfer server',
      what: 'Our timetable supplier sends us files over a file-transfer (SFTP) server open to the internet. For every connection to it we log the IP address, the time, any username tried and what was transferred.',
      why: 'To secure that server against break-in attempts, and to show which files we received from our supplier and when.',
      basis: LEGITIMATE_INTERESTS,
      retention:
        '7 days for connections that never try to sign in, 90 days for failed sign-ins and blocked addresses, and 400 days for successful sign-ins and file transfers (in practice, only our supplier’s).',
    },
    {
      title: 'Backups',
      what: 'Encrypted daily copies of our database, which include the data above.',
      why: 'To recover from failures.',
      basis: 'The same basis as the data they contain.',
      retention: 'Up to 14 days. Data you delete can stay in our backups for up to 14 days.',
    },
  ];
}

export default function PrivacyPage() {
  const mode = requireLegalPages();
  const { OPERATOR_NAME, ICO_REGISTRATION, MINIMUM_AGE } = LEGAL_CONFIG;
  const categories = dataCategories(retentionPolicy());
  return (
    <LegalPage draft={mode === 'preview'} title="Privacy notice">
      <LegalSection title="Who we are">
        <Text>
          Distant Signal is run by {OPERATOR_NAME}, the controller of your personal data. Contact us about privacy at{' '}
          <ContactEmail />.
        </Text>
        <Text>ICO registration: {ICO_REGISTRATION}.</Text>
      </LegalSection>

      <LegalSection title="What we collect, why, and for how long">
        <Text>
          You can look at live rail information without an account. We only hold personal data about you if you sign in,
          or in the technical logs every website produces.
        </Text>
        {categories.map((category) => (
          <Stack key={category.title} gap={4}>
            <Title order={3} size="h5">
              {category.title}
            </Title>
            <List size="sm" spacing={2}>
              <ListItem>What: {category.what}</ListItem>
              <ListItem>Why: {category.why}</ListItem>
              <ListItem>Lawful basis: {category.basis}</ListItem>
              <ListItem>How long: {category.retention}</ListItem>
            </List>
          </Stack>
        ))}
      </LegalSection>

      <LegalSection title="The AI chat">
        <Text>
          The chat is optional. If you use it, you give it your own Anthropic API key, which is stored only in your
          browser and never sent to us. Your messages, and any Distant Signal data the chat looks up to answer them, go
          directly from your browser to Anthropic under your own Anthropic account and{' '}
          <TextLink href="https://www.anthropic.com/legal" external underline="always" inline tone="inherit">
            Anthropic&apos;s terms
          </TextLink>
          . We do not receive or keep your chat. The answers are generated by AI and may be inaccurate.
        </Text>
        <Text>
          Separately, we use a self-hosted AI model to read National Rail incident messages and work out their timing
          and severity. Those messages are public operational information, not personal data, and nothing about you is
          sent to that model.
        </Text>
      </LegalSection>

      <LegalSection title="Who we share it with">
        <List size="sm" spacing={4}>
          <ListItem>
            Cloudflare, our network provider, which handles every visit to the site (including your IP address) on our
            behalf.
          </ListItem>
          <ListItem>
            Discord, if you sign in with Discord. Discord is a separate controller and its own privacy policy applies.
          </ListItem>
          <ListItem>
            Your browser&apos;s push service (such as Google, Mozilla or Apple), if you turn on notifications. It
            carries the notifications but cannot read them, because they are encrypted.
          </ListItem>
          <ListItem>Anthropic, only if you use the AI chat with your own key (see above).</ListItem>
          <ListItem>Members of groups you join, and anyone you give a share link to (see above).</ListItem>
        </List>
        <Text>
          Our sign-in service, database, backups and incident AI model all run on infrastructure we operate ourselves.
          We do not sell your data or use it for advertising.
        </Text>
      </LegalSection>

      <LegalSection title="International transfers">
        <Text>
          Cloudflare, Discord, the push services and Anthropic may process data in the United States. Cloudflare
          processes data for us under a data processing agreement that includes the UK International Data Transfer
          Addendum. Discord, the push services and Anthropic process data under their own terms with you.
        </Text>
      </LegalSection>

      <LegalSection title="Your rights">
        <Text>
          You have the right to access, correct, delete, restrict or object to our use of your personal data, and to
          receive it in a portable format.
        </Text>
        <List size="sm" spacing={4}>
          <ListItem>
            To get a copy of your data, use <strong>{ACCOUNT_EXPORT_LABEL}</strong> on your{' '}
            <TextLink href={ACCOUNT_ROUTE} underline="always" inline>
              account page
            </TextLink>
            .
          </ListItem>
          <ListItem>
            To delete your account and its data, use <strong>{ACCOUNT_DELETE_LABEL}</strong> on the same page. Deleting
            your Distant Signal account does not delete your Discord account. You can remove Distant Signal&apos;s
            access in Discord&apos;s settings.
          </ListItem>
          <ListItem>
            For anything else, or if you cannot sign in, email <ContactEmail />. We reply within one month.
          </ListItem>
        </List>
        <Text>
          If you are unhappy with how we handle your data, please tell us first. You can also complain to the
          Information Commissioner&apos;s Office (ICO) at{' '}
          <TextLink href="https://ico.org.uk/make-a-complaint/" external underline="always" inline tone="inherit">
            ico.org.uk
          </TextLink>{' '}
          or on 0303 123 1113.
        </Text>
      </LegalSection>

      <LegalSection title="Automated decisions">
        <Text>
          We do not make decisions about you by automated means that have legal or similarly significant effects.
        </Text>
      </LegalSection>

      <LegalSection title="Children">
        <Text>Distant Signal is not intended for anyone under {MINIMUM_AGE}.</Text>
      </LegalSection>

      <LegalSection title="Cookies and browser storage">
        <Text>
          We only use cookies and browser storage that the service needs or that you choose to turn on. See the{' '}
          <TextLink href="/cookies" underline="always" inline>
            cookies page
          </TextLink>
          .
        </Text>
      </LegalSection>

      <LegalSection title="Changes to this notice">
        <Text>If we change this notice, we will update the date at the top of this page.</Text>
      </LegalSection>
    </LegalPage>
  );
}
