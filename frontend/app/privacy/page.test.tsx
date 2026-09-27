import { describe, it, expect, afterEach, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import PrivacyPage from './page';

vi.mock('next/navigation', () => ({ notFound: () => { throw new Error('NEXT_NOT_FOUND'); } }));

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

  it('drops a limit that is switched off', () => {
    render({ RETENTION_STALE_PUSH_SUBSCRIPTION_DAYS: '0', RETENTION_PAST_TRAVEL_DAYS: '0' });
    expect(retentionText('Push notifications')).not.toMatch(/have not signed in/);
    expect(retentionText('Tickets')).not.toMatch(/travel date/);
  });
});
