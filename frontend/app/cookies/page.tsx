import { List, ListItem, Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { LegalPage, LegalSection, requireLegalPages } from '@/components/LegalPage';
import { legalPageMetadata } from '@/lib/legal';

// ============================================================================
// DRAFT -- NOT LEGAL ADVICE -- REVIEW BEFORE PUBLISHING.
// Drafted from the 2026-09-27 UK legal compliance gap analysis (LEG-11).
// Everything listed is strictly necessary or set at the user's request, so
// PECR reg. 6 needs no consent banner. If analytics are ever added, PECR
// Sch. A1 (in force 5 Feb 2026) requires clear information AND a free,
// simple way to object, so this page would then need an opt-out.
// Keep the list in sync with the code: cookie names in crates/api/src/auth.rs,
// storage keys in components/PrideToggle.tsx, components/ServiceWorkerRegister.tsx,
// lib/anthropicKey.ts and lib/mcpOAuthProvider.ts.
// ============================================================================

// Reads the legal-pages flags per request (see lib/legal.ts).
export const dynamic = 'force-dynamic';

export function generateMetadata(): Metadata {
  return legalPageMetadata('Cookies and browser storage', 'The cookies and browser storage Distant Signal uses, and why.');
}

interface StorageItem {
  name: string;
  purpose: string;
  lasts: string;
}

const COOKIES: readonly StorageItem[] = [
  {
    name: 'distant_signal_session',
    purpose: 'Keeps you signed in. Only sent over HTTPS and not readable by scripts.',
    lasts: '14 days, or until you sign out.',
  },
  {
    name: 'distant_signal_login',
    purpose: 'Protects the sign-in process from forgery while you sign in.',
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
    purpose: 'Remembers whether you turned on the pride colour theme.',
    lasts: 'Until you clear it.',
  },
  {
    name: 'lastSuccessfulLoadAt',
    purpose: 'Records when the app last loaded, so the offline page can say how old its information is. Never sent to us.',
    lasts: 'Until you clear it.',
  },
  {
    name: 'ds-anthropic-api-key',
    purpose:
      'Only if you choose to use the AI chat: your own Anthropic API key, stored in your browser at your choice and never sent to us. You can remove it in the chat settings.',
    lasts: 'Until you remove it, log out, or delete your account.',
  },
  {
    name: 'ds-mcp-oauth:…',
    purpose: 'Only if you use the AI chat: the sign-in details that let the chat look up rail data for you.',
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
        We do not use analytics, advertising or tracking cookies. Everything below is either needed for the service to
        work or stores a choice you made, so we do not ask for consent with a cookie banner.
      </Text>

      <LegalSection title="Cookies">
        <StorageList items={COOKIES} />
      </LegalSection>

      <LegalSection title="Browser storage (localStorage)">
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
