import { describe, it, expect, afterEach, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AccessibilityPage, { generateMetadata } from './page';

vi.mock('next/navigation', () => ({
  notFound: () => {
    throw new Error('NEXT_NOT_FOUND');
  },
}));

// LEG-15: the accessibility statement, behind the legal-pages gate.
describe('AccessibilityPage', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('404s while the legal pages are off (the default)', () => {
    expect(() => AccessibilityPage()).toThrow('NEXT_NOT_FOUND');
  });

  it('404s when published is requested but placeholders remain', () => {
    vi.stubEnv('LEGAL_PAGES_PUBLISHED', 'true');
    expect(() => AccessibilityPage()).toThrow('NEXT_NOT_FOUND');
  });

  it('renders the WCAG 2.2 AA target, the testing, the known limitations and the contact route in preview', () => {
    vi.stubEnv('LEGAL_PAGES_PREVIEW', 'true');
    renderWithMantine(<AccessibilityPage />);
    expect(screen.getByRole('heading', { level: 1, name: 'Accessibility statement' })).toBeInTheDocument();
    expect(screen.getByText(/WCAG\) 2\.2 at level AA/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Known limitations' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: '[[CONTACT_EMAIL]]' })).toHaveAttribute('href', 'mailto:[[CONTACT_EMAIL]]');
    expect(document.querySelector('[data-legal-draft]')).not.toBeNull();
  });

  it('is noindex until published', () => {
    expect(generateMetadata().robots).toEqual({ index: false, follow: false });
  });
});
