import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
import { notFound } from 'next/navigation';
import { renderWithMantine } from '@/test/render';
import * as legal from '@/lib/legal';
import PrivacyPage, { dynamic as privacyDynamic, generateMetadata as PrivacyGenerateMetadata } from './privacy/page';
import TermsPage, { dynamic as termsDynamic } from './terms/page';
import CookiesPage, { dynamic as cookiesDynamic } from './cookies/page';
import ContactPage, { dynamic as contactDynamic } from './contact/page';
import AccessibilityPage, { dynamic as accessibilityDynamic } from './accessibility/page';
import AttributionPage, { dynamic as attributionDynamic } from './attribution/page';

// Real Next throws from notFound(); mirror that so an unpublished page never
// renders past the gate.
vi.mock('next/navigation', () => ({
  notFound: vi.fn(() => {
    throw new Error('NEXT_NOT_FOUND');
  }),
}));

vi.mock('@/lib/legal', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/legal')>();
  return { ...actual, legalPagesMode: vi.fn(() => 'off'), legalPagesPublished: vi.fn(() => false) };
});

const PAGES = [
  ['/privacy', PrivacyPage, privacyDynamic, 'Privacy notice'],
  ['/terms', TermsPage, termsDynamic, 'Terms of use'],
  ['/cookies', CookiesPage, cookiesDynamic, 'Cookies and browser storage'],
  ['/contact', ContactPage, contactDynamic, 'Contact'],
  ['/accessibility', AccessibilityPage, accessibilityDynamic, 'Accessibility statement'],
] as const;

afterEach(() => {
  vi.mocked(legal.legalPagesMode).mockReturnValue('off');
  vi.mocked(legal.legalPagesPublished).mockReturnValue(false);
  vi.mocked(notFound).mockClear();
});

describe('draft legal pages', () => {
  it.each(PAGES)('%s 404s while the legal pages are unpublished', (_path, Page) => {
    expect(() => Page()).toThrow('NEXT_NOT_FOUND');
    expect(notFound).toHaveBeenCalled();
  });

  it.each(PAGES)('%s renders per request, so the runtime flag is read', (_path, _Page, dynamic) => {
    expect(dynamic).toBe('force-dynamic');
  });

  it.each(PAGES)('%s renders its heading once published', (_path, Page, _dynamic, heading) => {
    vi.mocked(legal.legalPagesMode).mockReturnValue('published');
    renderWithMantine(Page());
    expect(screen.getByRole('heading', { level: 1, name: heading })).toBeInTheDocument();
    expect(notFound).not.toHaveBeenCalled();
  });

  it.each(PAGES)('%s shows a draft banner in preview mode, and none once published', (_path, Page) => {
    vi.mocked(legal.legalPagesMode).mockReturnValue('preview');
    const preview = renderWithMantine(Page());
    expect(preview.container.querySelector('[data-legal-draft]')).toHaveTextContent('Draft, not yet published');
    preview.unmount();
    vi.mocked(legal.legalPagesMode).mockReturnValue('published');
    const live = renderWithMantine(Page());
    expect(live.container.querySelector('[data-legal-draft]')).toBeNull();
  });

  it('marks the pages noindex while they are not really published', () => {
    expect(PrivacyGenerateMetadata().robots).toEqual({ index: false, follow: false });
  });

  it('points privacy rights at the account export and deletion features', () => {
    vi.mocked(legal.legalPagesMode).mockReturnValue('published');
    renderWithMantine(PrivacyPage());
    expect(screen.getByText(legal.ACCOUNT_EXPORT_LABEL)).toBeInTheDocument();
    expect(screen.getByText(legal.ACCOUNT_DELETE_LABEL)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'account page' })).toHaveAttribute('href', legal.ACCOUNT_ROUTE);
    expect(screen.getByText(/0303 123 1113/)).toBeInTheDocument();
  });

  it('lists the Anthropic key as stored locally at the user’s choice', () => {
    vi.mocked(legal.legalPagesMode).mockReturnValue('published');
    renderWithMantine(CookiesPage());
    expect(screen.getByText('ds-anthropic-api-key')).toBeInTheDocument();
    expect(screen.getByText(/stored in your browser at your choice and never sent to us/)).toBeInTheDocument();
    expect(screen.getByText('distant_signal_session')).toBeInTheDocument();
  });

  it('gives a content reporting route in the terms (Online Safety Act)', () => {
    vi.mocked(legal.legalPagesMode).mockReturnValue('published');
    renderWithMantine(TermsPage());
    expect(screen.getByRole('heading', { level: 2, name: 'Reporting content and complaints' })).toBeInTheDocument();
    expect(screen.getAllByRole('link', { name: legal.LEGAL_CONFIG.CONTACT_EMAIL }).length).toBeGreaterThan(0);
  });
});

describe('/attribution', () => {
  it('renders per request, so its footer reads the runtime legal-pages flag', () => {
    expect(attributionDynamic).toBe('force-dynamic');
  });

  it('is always public, whatever the legal pages flag says', () => {
    renderWithMantine(AttributionPage());
    expect(screen.getByRole('heading', { level: 1, name: 'Data sources and licences' })).toBeInTheDocument();
    expect(notFound).not.toHaveBeenCalled();
  });
});
