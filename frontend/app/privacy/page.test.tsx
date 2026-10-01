import { describe, it, expect, afterEach, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { visibleText } from '@/test/routeText';
import PrivacyPage from './page';

vi.mock('next/navigation', () => ({
  notFound: () => {
    throw new Error('NEXT_NOT_FOUND');
  },
}));

function retentionText(title: string): string {
  const heading = screen.getByRole('heading', { name: title });
  return heading.parentElement?.textContent ?? '';
}

// LEG-28 / DQ7: the notice states the api's actual retention settings.
describe('PrivacyPage retention copy', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  function render(env: Record<string, string> = {}) {
    vi.stubEnv('LEGAL_PAGES_PREVIEW', 'true');
    for (const [k, v] of Object.entries(env)) vi.stubEnv(k, v);
    renderWithMantine(<PrivacyPage />);
  }

  it('mentions the 12-month push-subscription prune', () => {
    render();
    expect(retentionText('Push notifications')).toMatch(/have not signed in .* for 12 months/);
  });

  it('states the 18-month travel-date limit for tracked trains, journeys and tickets', () => {
    render();
    expect(retentionText('Tracked trains, journeys and journey templates')).toMatch(/18 months after the travel date/);
    expect(retentionText('Tickets')).toMatch(/18 months after the travel date/);
  });

  it('says nothing about inactive-account deletion while it is off (the default)', () => {
    render();
    expect(retentionText('Your account')).not.toMatch(/do not sign in/);
  });

  it('states the 730-day inactive-account deletion once it is enabled', () => {
    render({ RETENTION_INACTIVE_ACCOUNT_DAYS: '730' });
    expect(retentionText('Your account')).toMatch(/If you do not sign in for 24 months .* we delete your account/);
  });

  // docs/personal-data-retention.md: the SFTP server's 7/90/400-day log tiers.
  it('states the file-transfer server log tiers', () => {
    render();
    const text = retentionText('Connections to our file-transfer server');
    expect(text).toMatch(/7 days for connections that never try to sign in/);
    expect(text).toMatch(/90 days for failed sign-ins and blocked addresses/);
    expect(text).toMatch(/400 days for successful sign-ins and file transfers/);
  });

  it('drops a limit that is switched off', () => {
    render({ RETENTION_STALE_PUSH_SUBSCRIPTION_DAYS: '0', RETENTION_PAST_TRAVEL_DAYS: '0' });
    expect(retentionText('Push notifications')).not.toMatch(/have not signed in/);
    expect(retentionText('Tickets')).not.toMatch(/travel date/);
  });
});

describe('PrivacyPage external links', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it.each([
    ["Anthropic's terms", 'https://www.anthropic.com/legal'],
    ['ico.org.uk', 'https://ico.org.uk/make-a-complaint/'],
  ])('%s opens a new tab and says so, keeping its visible text and inherited colour', (text, href) => {
    vi.stubEnv('LEGAL_PAGES_PREVIEW', 'true');
    renderWithMantine(<PrivacyPage />);
    const link = screen.getByRole('link', { name: `${text} (opens in a new tab)` });
    expect(link).toHaveAttribute('href', href);
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(link).toHaveAttribute('data-text-link-tone', 'inherit');
    expect(link).toHaveAttribute('data-text-link', 'always');
    expect(visibleText(link)).toBe(text);
    expect(link.querySelector('svg[data-icon="external-link"]')).toHaveAttribute('aria-hidden', 'true');
  });
});
