import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { UpcomingDisruptions } from './UpcomingDisruptions';
import type { UpcomingDisruption } from '@/lib/types';

const strike = (day: string, from: string, to: string): UpcomingDisruption => ({
  from,
  to,
  summary: `Industrial action to affect TransPennine Express services on ${day}`,
  incidentId: '1D3D4694',
});

const upcoming = [
  strike('Sunday 11 October', '2026-10-10T23:00:00Z', '2026-10-11T23:00:00Z'),
  strike('Sunday 18 October', '2026-10-17T23:00:00Z', '2026-10-18T23:00:00Z'),
];

describe('UpcomingDisruptions', () => {
  it('renders nothing without notes', () => {
    const { container } = renderWithMantine(<UpcomingDisruptions upcoming={[]} />);
    expect(container.querySelector('[data-upcoming]')).toBeNull();
    renderWithMantine(<UpcomingDisruptions upcoming={undefined} compact />);
    expect(screen.queryByText(/Upcoming/)).toBeNull();
  });

  it('lists every note with its day and a link to the incident', () => {
    renderWithMantine(<UpcomingDisruptions upcoming={upcoming} />);
    expect(screen.getByText('Upcoming')).toBeInTheDocument();
    expect(screen.getByText('Sun 11 Oct')).toBeInTheDocument();
    expect(screen.getByText('Sun 18 Oct')).toBeInTheDocument();
    const links = screen.getAllByRole('link');
    expect(links).toHaveLength(2);
    expect(links[0]).toHaveAttribute('href', '/incidents/1D3D4694');
  });

  it('shows only the soonest note, without a link, on a card', () => {
    renderWithMantine(<UpcomingDisruptions upcoming={upcoming} compact />);
    const note = document.querySelector('[data-upcoming]');
    expect(note?.textContent).toBe(
      'Upcoming: Sun 11 Oct, Industrial action to affect TransPennine Express services on Sunday 11 October (+1 more)',
    );
    expect(screen.queryByRole('link')).toBeNull();
  });
});
