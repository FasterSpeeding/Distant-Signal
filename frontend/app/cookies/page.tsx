import { List, ListItem, Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { LegalPage, LegalSection, requireLegalPages } from '@/components/LegalPage';
import { AnalyticsOptIn } from '@/components/AnalyticsOptIn';
import { TextLink } from '@/components/TextLink';
import { legalPageMetadata } from '@/lib/legal';
import { ANALYTICS_OPT_IN_STORAGE_KEY, OFFLINE_PENDING_STORAGE_KEY } from '@/lib/usageAnalytics';
import { withAbsoluteTitle } from '@/lib/pageMetadata';

// ============================================================================
// DRAFT -- NOT LEGAL ADVICE -- REVIEW BEFORE PUBLISHING.
// Drafted from the 2026-09-27 UK legal compliance gap analysis (LEG-11).
// Everything listed is strictly necessary or set at the user's request, so
// PECR reg. 6 needs no consent banner. The one analytics feature, the usage
// counts (DS-190), is opt-in consent rather than relying on PECR Sch. A1's
// analytics exemption: off by default, turned on with the switch on this page
// or in Preferences, and turned off (the free, simple objection Sch. A1 asks
// for) with the same switch. It sets no cookie; its two browser storage keys
// are only written once the visitor opts in, and Global Privacy Control and
// Do Not Track stop it entirely (lib/usageAnalytics.ts).
// Keep the list in sync with the code: cookie names in crates/api/src/auth.rs,
// storage keys in components/PrideMode.tsx, components/ServiceWorkerRegister.tsx,
// lib/anthropicKey.ts, lib/mcpOAuthProvider.ts, lib/usageAnalytics.ts and
// public/offline.html.
// ============================================================================

// Reads the legal-pages flags per request (see lib/legal.ts).
export const dynamic = 'force-dynamic';

export function generateMetadata(): Metadata {
  return withAbsoluteTitle(
    legalPageMetadata(
      'Cookies and browser storage',
      'The cookies and browser storage Distant Signal uses, and why.',
      '/cookies',
    ),
  );
}

interface StorageItem {
  name: string;
  purpose: string;
  lasts: string;
}

const COOKIES: readonly StorageItem[] = [
  {
    name: 'distant_signal_session',
    purpose: 'Keeps you logged in. Only sent over HTTPS and not readable by scripts.',
    lasts: '14 days, or until you sign out.',
  },
  {
    name: 'distant_signal_login',
    purpose: 'Protects the login process from forgery while you log in.',
    lasts: 'A few minutes.',
  },
];

const LOCAL_STORAGE: readonly StorageItem[] = [
  {
    name: 'mantine-color-scheme-value',
    purpose: 'Remembers the light or dark theme you chose.',
    lasts: 'Until you clear it.',
  },
  {
    name: 'pride-mode',
    purpose: 'Remembers the pride flag you chose, if any.',
    lasts: 'Until you clear it.',
  },
  {
    name: 'lastSuccessfulLoadAt',
    purpose:
      'Records when the app last loaded, so the offline page can say how old its information is. Never sent to us.',
    lasts: 'Until you clear it.',
  },
  {
    name: 'ds-anthropic-api-key',
    purpose:
      'Only if you choose to use the AI chat: your own Claude API key, stored in your browser at your choice and never sent to us. You can remove it in Preferences.',
    lasts: 'Until you remove it, log out, or delete your account.',
  },
  {
    name: ANALYTICS_OPT_IN_STORAGE_KEY,
    purpose:
      'Only if you turn on usage counts while logged out: remembers that you said yes. Logged in, the choice is saved to your account instead.',
    lasts: 'Until you turn usage counts off or clear it.',
  },
  {
    name: OFFLINE_PENDING_STORAGE_KEY,
    purpose:
      'Only if you turned on usage counts: notes that the offline page was shown, so it can be counted once you are back online. Then removed.',
    lasts: 'Until the next page you open while online.',
  },
  {
    name: 'ds-mcp-oauth:*',
    purpose: 'Only if you use the AI chat: the login details that let the chat look up rail data for you.',
    lasts: 'Until you log out, delete your account, or clear it.',
  },
];

function StorageList({ items }: { items: readonly StorageItem[] }) {
  return (
    <Stack gap="sm">
      {items.map((item) => (
        <Stack key={item.name} gap={2}>
          <Title order={3} size="h5">
            <code>{item.name}</code>
          </Title>
          <List size="sm" spacing={2}>
            <ListItem>Purpose: {item.purpose}</ListItem>
            <ListItem>How long: {item.lasts}</ListItem>
          </List>
        </Stack>
      ))}
    </Stack>
  );
}

export default function CookiesPage() {
  const mode = requireLegalPages();
  return (
    <LegalPage draft={mode === 'preview'} title="Cookies and browser storage">
      <Text>
        We do not use advertising or tracking cookies, or anyone else&apos;s analytics. Everything below is either
        needed for the service to work or stores a choice you made, so we do not ask for consent with a cookie banner.
      </Text>

      <LegalSection title="Usage counts">
        <Text>
          You can choose to help us improve Distant Signal by sending counts of which kinds of pages and features you
          use. It is off unless you turn it on. It uses no cookies, goes only to us, and we keep only daily totals with
          nothing that identifies you. If your browser sends Global Privacy Control or Do Not Track, nothing is sent.
          Turning it off stops any more counts. The{' '}
          <TextLink href="/privacy" underline="always" inline>
            privacy notice
          </TextLink>{' '}
          has the details.
        </Text>
        <AnalyticsOptIn />
      </LegalSection>

      <LegalSection title="Cookies">
        <StorageList items={COOKIES} />
      </LegalSection>

      <LegalSection title="Saved in this browser">
        <StorageList items={LOCAL_STORAGE} />
      </LegalSection>

      <LegalSection title="Offline files">
        <Text>
          The app saves its own files (scripts, styles and icons) in your browser so it can load when you are offline.
          It does not save your personal data this way.
        </Text>
      </LegalSection>

      <LegalSection title="Network error reports">
        <Text>
          Our network provider, Cloudflare, asks your browser to report failed connections to it (Network Error
          Logging). This helps keep the site reachable and is not used to track you.
        </Text>
      </LegalSection>

      <LegalSection title="Removing them">
        <Text>
          You can delete cookies and site data at any time in your browser settings. If you do, you will be signed out
          and your theme choices will reset.
        </Text>
      </LegalSection>
    </LegalPage>
  );
}
