import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NationalRailCredit, NATIONAL_RAIL_URL } from './NationalRailCredit';
import { visibleText } from '@/test/routeText';

describe('NationalRailCredit (LEG-23)', () => {
  it('carries both Schedule 1 strings verbatim, matching the footer line', () => {
    const { container } = renderWithMantine(<NationalRailCredit />);
    const credit = container.querySelector('[data-nre-credit]');
    expect(visibleText(credit as Element)).toMatch(
      /^Live departure data powered by NationalRail \(Train Information Services Ltd\)$/,
    );
  });

  it('links only "powered by NationalRail", in a new tab without an opener', () => {
    renderWithMantine(<NationalRailCredit />);
    const link = screen.getByRole('link', { name: 'powered by NationalRail (opens in a new tab)' });
    expect(link).toHaveAttribute('href', NATIONAL_RAIL_URL);
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(screen.getAllByRole('link')).toHaveLength(1);
    // A TextLink in the credit's own dimmed colour, underlined.
    expect(link).toHaveAttribute('data-text-link', 'always');
    expect(link).toHaveAttribute('data-text-link-tone', 'inherit');
  });

  it('uses text only, never an NRE logo', () => {
    const { container } = renderWithMantine(<NationalRailCredit />);
    // The one SVG is the link's new-tab icon, not a logo.
    expect(container.querySelector('img, svg:not([data-icon="external-link"])')).toBeNull();
  });

  it('says the credit link opens in a new tab, without changing its visible wording', () => {
    renderWithMantine(<NationalRailCredit />);
    const link = screen.getByRole('link', { name: 'powered by NationalRail (opens in a new tab)' });
    expect(visibleText(link)).toBe('powered by NationalRail');
    expect(link.querySelector('svg[data-icon="external-link"]')).toHaveAttribute('aria-hidden', 'true');
  });
});
