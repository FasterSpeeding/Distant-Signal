import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { MINOR_DELAYS, NETWORK } from '@/test/transitFixtures';
import { PinnedTramLineCard } from './PinnedTramLineCard';

describe('PinnedTramLineCard', () => {
  it('is a status card like its National Rail neighbours: colour, name link, badge, network', () => {
    const { container } = renderWithMantine(
      <PinnedTramLineCard
        pinned={{
          network: 'metrolink',
          networkName: 'Manchester Metrolink',
          line: { ...NETWORK.lines[0]!, status: MINOR_DELAYS },
        }}
      />,
    );
    // The swatch is decoration: the link is named by the line alone.
    expect(screen.getByRole('link', { name: 'Blue Line' }).getAttribute('href')).toBe('/transit/metrolink/lines/blue');
    expect(container.querySelector('[data-line-swatch]')).not.toBeNull();
    expect(container.querySelector('[data-card-title-row]')?.textContent).toContain('Minor delays');
    expect(screen.getByText('Manchester Metrolink')).toBeTruthy();
  });
});
