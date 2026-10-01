import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { renderWithMantine } from '@/test/render';
import CookiesPage from '@/app/cookies/page';
import PrivacyPage from '@/app/privacy/page';
import TermsPage from '@/app/terms/page';
import ContactPage from '@/app/contact/page';

vi.mock('next/navigation', () => ({
  notFound: () => {
    throw new Error('NEXT_NOT_FOUND');
  },
}));

/** Each heading's level, in document order. */
function headingLevels(): number[] {
  return screen.getAllByRole('heading').map((heading) => Number(heading.tagName.slice(1)));
}

// Style-guide review: `/cookies` and `/privacy` render their per-item
// headings as `order={3} size="h5"`, which looked like a possible skipped
// level. They sit inside `LegalSection`'s h2s, so the outline is
// h1 > h2 > h3 with no skip -- pinned here for every legal page, since the
// visual size (h5) and the outline level (h3) are set independently.
describe.each<[string, () => ReactElement]>([
  ['/cookies', () => <CookiesPage />],
  ['/privacy', () => <PrivacyPage />],
  ['/terms', () => <TermsPage />],
  ['/contact', () => <ContactPage />],
])('%s heading outline', (_path, page) => {
  beforeEach(() => {
    vi.stubEnv('LEGAL_PAGES_PREVIEW', 'true');
  });
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('has exactly one h1, first, and never skips a level', () => {
    renderWithMantine(page());
    const levels = headingLevels();
    expect(levels[0]).toBe(1);
    expect(levels.filter((level) => level === 1)).toHaveLength(1);
    levels.forEach((level, i) => {
      if (i > 0)
        expect(level, `heading ${i} (h${level}) after h${levels[i - 1]}`).toBeLessThanOrEqual(levels[i - 1]! + 1);
    });
  });
});

describe('/cookies per-item headings', () => {
  beforeEach(() => {
    vi.stubEnv('LEGAL_PAGES_PREVIEW', 'true');
  });
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('are h3s under an h2 section, not skipped-to h5s', () => {
    renderWithMantine(<CookiesPage />);
    const item = screen.getByRole('heading', { name: 'ds-anthropic-api-key' });
    expect(item.tagName).toBe('H3');
    const section = screen.getByRole('heading', { name: 'Browser storage (localStorage)' });
    expect(section.tagName).toBe('H2');
  });
});
