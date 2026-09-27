import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  __resetLegalWarningForTests,
  DEFAULT_RETENTION_POLICY,
  describeRetentionDays,
  retentionPolicy,
  LEGAL_CONFIG,
  legalPageMetadata,
  legalPagesMode,
  legalPagesPublished,
  legalPagesVisible,
  unfilledLegalPlaceholders,
  type LegalConfig,
} from './legal';

const FILLED: LegalConfig = {
  OPERATOR_NAME: 'Example Operator',
  CONTACT_EMAIL: 'hello@example.com',
  ICO_REGISTRATION: 'ZA000000',
  MINIMUM_AGE: '18',
  SSO_LOG_RETENTION: '1 year',
  LAST_UPDATED: '1 November 2026',
};

describe('legal pages flag', () => {
  beforeEach(() => {
    __resetLegalWarningForTests();
    vi.spyOn(console, 'warn').mockImplementation(() => {});
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('is off when LEGAL_PAGES_PUBLISHED is unset (the default)', () => {
    expect(legalPagesPublished({}, FILLED)).toBe(false);
  });

  it.each(['false', '1', 'TRUE', 'yes', ''])('is off for LEGAL_PAGES_PUBLISHED=%j', (value) => {
    expect(legalPagesPublished({ LEGAL_PAGES_PUBLISHED: value }, FILLED)).toBe(false);
  });

  it('is on only for exactly "true" with every operator value filled in', () => {
    expect(legalPagesPublished({ LEGAL_PAGES_PUBLISHED: 'true' }, FILLED)).toBe(true);
  });

  it('stays off, and warns once, while any placeholder is left', () => {
    const partial = { ...FILLED, CONTACT_EMAIL: '[[CONTACT_EMAIL]]' };
    expect(legalPagesPublished({ LEGAL_PAGES_PUBLISHED: 'true' }, partial)).toBe(false);
    expect(legalPagesPublished({ LEGAL_PAGES_PUBLISHED: 'true' }, partial)).toBe(false);
    expect(console.warn).toHaveBeenCalledTimes(1);
    expect(vi.mocked(console.warn).mock.calls[0][0]).toContain('CONTACT_EMAIL');
  });

  it('ships with every operator value still a placeholder, so the pages cannot go live by accident', () => {
    expect(unfilledLegalPlaceholders(LEGAL_CONFIG)).toEqual(Object.keys(LEGAL_CONFIG));
    expect(legalPagesPublished({ LEGAL_PAGES_PUBLISHED: 'true' })).toBe(false);
  });

  it('reads process.env by default', () => {
    const before = process.env.LEGAL_PAGES_PUBLISHED;
    delete process.env.LEGAL_PAGES_PUBLISHED;
    expect(legalPagesPublished()).toBe(false);
    if (before !== undefined) process.env.LEGAL_PAGES_PUBLISHED = before;
  });

  it('previews the drafts, placeholders and all, only with LEGAL_PAGES_PREVIEW=true', () => {
    expect(legalPagesMode({ LEGAL_PAGES_PREVIEW: 'true' })).toBe('preview');
    expect(legalPagesVisible({ LEGAL_PAGES_PREVIEW: 'true' })).toBe(true);
    expect(legalPagesPublished({ LEGAL_PAGES_PREVIEW: 'true' })).toBe(false);
    expect(legalPagesMode({ LEGAL_PAGES_PREVIEW: '1' })).toBe('off');
    expect(legalPagesVisible({})).toBe(false);
  });

  it('falls back to preview, not published, when the publish flag is on but placeholders remain', () => {
    expect(legalPagesMode({ LEGAL_PAGES_PUBLISHED: 'true', LEGAL_PAGES_PREVIEW: 'true' })).toBe('preview');
    expect(legalPagesMode({ LEGAL_PAGES_PUBLISHED: 'true', LEGAL_PAGES_PREVIEW: 'true' }, FILLED)).toBe('published');
  });
});

describe('legalPageMetadata', () => {
  it('is noindex unless the pages are really published', () => {
    expect(legalPageMetadata('Terms of use', 'd', false).robots).toEqual({ index: false, follow: false });
    expect(legalPageMetadata('Terms of use', 'd', true).robots).toBeUndefined();
    expect(legalPageMetadata('Terms of use', 'd', true).title).toBe('Terms of use — Distant Signal');
  });
});

// LEG-28 / DQ7: the privacy notice follows the api's retention settings.
describe('retentionPolicy', () => {
  it('defaults to the chart defaults (inactive-account deletion off)', () => {
    expect(retentionPolicy({})).toEqual({ pastTravelDays: 548, stalePushSubscriptionDays: 365, inactiveAccountDays: 0 });
  });

  it('reads the values the chart copies from the api', () => {
    expect(
      retentionPolicy({
        RETENTION_PAST_TRAVEL_DAYS: '400',
        RETENTION_STALE_PUSH_SUBSCRIPTION_DAYS: '0',
        RETENTION_INACTIVE_ACCOUNT_DAYS: '730',
      }),
    ).toEqual({ pastTravelDays: 400, stalePushSubscriptionDays: 0, inactiveAccountDays: 730 });
  });

  it('falls back to the default for a malformed value', () => {
    expect(retentionPolicy({ RETENTION_INACTIVE_ACCOUNT_DAYS: 'soon', RETENTION_PAST_TRAVEL_DAYS: '-1' })).toEqual(
      DEFAULT_RETENTION_POLICY,
    );
  });
});

describe('describeRetentionDays', () => {
  it.each([
    [365, '12 months'],
    [548, '18 months'],
    [730, '24 months'],
    [30, '1 month'],
    [45, '45 days'],
    [1, '1 day'],
  ])('%i -> %s', (days, words) => {
    expect(describeRetentionDays(days)).toBe(words);
  });
});
