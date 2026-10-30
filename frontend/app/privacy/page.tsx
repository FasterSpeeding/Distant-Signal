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
import { withAbsoluteTitle } from '@/lib/pageMetadata';

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
// Server logs: Loki keeps the application logs for 7 days.
// Usage analytics (DS-190): the api's feature counters (Prometheus, 30 days,
// no identifiers, legitimate interests) and the opt-in usage counts
// (`usage_daily`, daily totals, 13 months, consent; GPC and DNT honoured).
// Keep both rows in step with crates/api/src/usage_events.rs and
// docs/personal-data-retention.md.
// ============================================================================

// Reads the legal-pages flags per request (see lib/legal.ts).
export const dynamic = 'force-dynamic';

export function generateMetadata(): Metadata {
  return withAbsoluteTitle(
    legalPageMetadata(
      'Privacy notice',
      'What personal data Distant Signal holds, why, for how long, and your rights.',
      '/privacy',
    ),
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
const IMPROVING =
  'Legitimate interests (UK GDPR Art. 6(1)(f)): understanding how the service is used, in a way that cannot identify you.';

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
      what: 'The ID our login service gives you, your display name and username, the access groups needed to decide which features you can use, and when you signed up and last logged in.',
      why: 'To create and run your account.',
      basis: CONTRACT,
      retention:
        retention.inactiveAccountDays > 0
          ? `Until you delete your account. If you do not log in for ${describeRetentionDays(retention.inactiveAccountDays)} (and have no active session), we delete your account and everything in it automatically. We do not hold your email address, so we cannot warn you first.`
          : 'Until you delete your account.',
    },
    {
      title: 'Login sessions',
      what: 'A hashed session token and its expiry, and short-lived login state (a one-time code verifier and security values).',
      why: 'To keep you logged in, and to protect logging in from forgery.',
      basis: `${CONTRACT} ${LEGITIMATE_INTERESTS}`,
      retention: 'Sessions expire after 14 days and are then deleted. Login state is deleted after 15 minutes.',
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
      retention: `Until you turn notifications off, your browser's push service tells us the address no longer works, you have more than 20 devices registered (the oldest is removed)${
        retention.stalePushSubscriptionDays > 0
          ? `, you have not logged in (and your browser has not renewed the address) for ${describeRetentionDays(retention.stalePushSubscriptionDays)}`
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
      what: 'Your IP address, the pages you request and when, handled by our network provider (Cloudflare), our servers and our login service.',
      why: 'To keep the service secure, prevent abuse and fix faults.',
      basis: LEGITIMATE_INTERESTS,
      retention: `Our server logs are kept for up to 7 days. Our login service keeps its event logs for ${LEGAL_CONFIG.SSO_LOG_RETENTION}. Cloudflare keeps its logs under its own policy.`,
    },
    {
      title: 'Feature counts',
      what: 'Our servers count how often features are used and how they turn out, such as how many trains are tracked, trips planned or share links opened. These are running totals with no account, IP address or other identifier attached, and they never name a station, line, train or journey.',
      why: 'To see which features are used and whether they work, so we can fix and improve them.',
      basis: `${IMPROVING} To object, email us (see "Your rights").`,
      retention: 'Kept in our monitoring system for 30 days.',
    },
    {
      title: 'Usage counts (only if you turn them on)',
      what: 'If you turn on "Help improve Distant Signal", your browser tells us which kinds of page you open (such as "a line page", never which line), which steps of a feature you reach (such as searching for a trip and getting results), when a search finds nothing, when a request or page fails (with only its error reference), whether you use the installed app, and when the offline page was shown. Never a specific station, line, train or journey. It uses no cookies, and goes only to us. We add each one to a daily total, split only by whether you were logged in, and store nothing that identifies you: no IP address, account, device or browser details.',
      why: 'To learn which features are used and where people run into problems, so we can improve them.',
      basis:
        'Consent (UK GDPR Art. 6(1)(a)): it is off unless you turn it on, in Preferences or on the cookies page. If your browser sends Global Privacy Control or Do Not Track, nothing is sent even if it is on.',
      retention:
        'Daily totals are kept for 13 months, then deleted. Turning it off stops any more counts straight away. Totals already counted cannot be linked to you or anyone else.',
    },
    {
      title: 'Connections to our file-transfer server',
      what: 'Our timetable supplier sends us files over a file-transfer (SFTP) server open to the internet. For every connection to it we log the IP address, the time, any username tried and what was transferred.',
      why: 'To secure that server against break-in attempts, and to show which files we received from our supplier and when.',
      basis: LEGITIMATE_INTERESTS,
      retention:
        "7 days for connections that never try to log in, 90 days for failed logins and blocked addresses, and 400 days for successful logins and file transfers (in practice, only our supplier's).",
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
          You can look at live rail information without an account. We only hold personal data about you if you log in,
          or in the technical logs every website produces. If you turn on usage counts, we keep only anonymous daily
          totals.
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

      {/* Delegated access (docs/superpowers/specs/2026-10-14-ds-mcp-delegated-
          access-design.md). Worded to ship before the feature: no flag
          exists yet. Drop "Once it is available" when Connected apps goes
          live. */}
      <LegalSection title="Connecting your AI assistant">
        <Text>
          This is optional and off unless you turn it on. Once it is available, you turn it on in Connected apps on your
          account page. It lets an AI assistant you have connected to Distant Signal, such as Claude, work with the
          journeys in your account.
        </Text>
        <List size="sm" spacing={4}>
          <ListItem>It can save journeys you plan with it, and list and read your own journeys.</ListItem>
          <ListItem>
            It can delete only journeys it saved itself that are not shared with a group or by a link.
          </ListItem>
          <ListItem>
            It can read journeys shared in a group only if a group owner or admin has turned this on for that group.
            Other members&apos; journey names are hidden from it.
          </ListItem>
          <ListItem>It cannot create share links or change your groups.</ListItem>
        </List>
        <Text>
          The journeys it saves, lists or reads (their names, dates, stations and trains) go through our assistant
          connector, which we run ourselves, to your assistant&apos;s AI provider, such as Anthropic. The provider
          handles them under your own account and its own terms.
        </Text>
        <Text>
          To disconnect, use Disconnect in Connected apps. It takes effect straight away. The connection also ends after
          90 days without use, and when you log out of our login service. Logging out of Distant Signal alone does not
          end it.
        </Text>
        <Text>
          We log each request your assistant makes (your account, the time, what it asked for and whether it worked),
          never the journey contents, and keep these logs for 7 days. We keep a list of the journeys it deleted (name,
          date, stations and train) for 30 days, shown in Connected apps so you can save them again. After a connection
          ends we keep a record of it (which app, what it could do, and when it was connected, last used and ended) for
          one year, then delete it.
        </Text>
      </LegalSection>

      <LegalSection title="Who we share it with">
        <List size="sm" spacing={4}>
          <ListItem>
            Cloudflare, our network provider, which handles every visit to the site (including your IP address) on our
            behalf.
          </ListItem>
          <ListItem>
            Discord, if you log in with Discord. Discord is a separate controller and its own privacy policy applies.
          </ListItem>
          <ListItem>
            Your browser&apos;s push service (such as Google, Mozilla or Apple), if you turn on notifications. It
            carries the notifications but cannot read them, because they are encrypted.
          </ListItem>
          <ListItem>Anthropic, only if you use the AI chat with your own key (see above).</ListItem>
          <ListItem>Your AI assistant&apos;s provider, only if you connect your assistant (see above).</ListItem>
          <ListItem>Members of groups you join, and anyone you give a share link to (see above).</ListItem>
        </List>
        <Text>
          Our login service, database, backups and incident AI model all run on infrastructure we operate ourselves. We
          do not sell your data or use it for advertising.
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
            For anything else, or if you cannot log in, email <ContactEmail />. We reply within one month.
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
