import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NationalRailCredit, NATIONAL_RAIL_URL } from './NationalRailCredit';

describe('NationalRailCredit (LEG-23)', () => {
  it('carries both Schedule 1 strings verbatim, matching the footer line', () => {
    const { container } = renderWithMantine(<NationalRailCredit />);
    const credit = container.querySelector('[data-nre-credit]');
    expect(credit).toHaveTextContent(
      /^Live departure data powered by NationalRail \(Train Information Services Ltd\)$/,
    );
  });

  it('links only "powered by NationalRail", in a new tab without an opener', () => {
    renderWithMantine(<NationalRailCredit />);
    const link = screen.getByRole('link', { name: 'powered by NationalRail' });
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
    expect(container.querySelector('img, svg')).toBeNull();
  });
});
