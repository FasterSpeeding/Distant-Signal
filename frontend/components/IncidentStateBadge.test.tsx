import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { IncidentStateBadge } from './IncidentStateBadge';

describe('IncidentStateBadge', () => {
  it('shows Active for an incident still listed and not cleared', () => {
    renderWithMantine(<IncidentStateBadge isCleared={false} sourceRemovedAt={null} />);
    const badge = screen.getByText('Active');
    expect(badge.closest('[data-incident-state="active"]')).not.toBeNull();
  });

  it('shows Active against an api that predates sourceRemovedAt', () => {
    renderWithMantine(<IncidentStateBadge isCleared={false} />);
    expect(screen.getByText('Active')).toBeInTheDocument();
  });

  it('shows Cleared when RDM cleared it', () => {
    renderWithMantine(<IncidentStateBadge isCleared={true} sourceRemovedAt={null} />);
    const badge = screen.getByText('Cleared');
    expect(badge.closest('[data-incident-state="cleared"]')).not.toBeNull();
  });

  it('shows Ended, with when the source last listed it, for an unlisted, uncleared incident', () => {
    renderWithMantine(<IncidentStateBadge isCleared={false} sourceRemovedAt="2026-10-05T22:55:00+00:00" />);
    const badge = screen.getByText(/^Ended/).closest('[data-incident-state="ended"]');
    expect(badge).not.toBeNull();
    expect(badge).toHaveAttribute('title', 'No longer listed by the source since 5 Oct 2026, 23:55');
    // Screen readers get the same words, not just the colour or a title.
    expect(badge).toHaveTextContent('Ended: No longer listed by the source since 5 Oct 2026, 23:55');
    expect(screen.queryByText('Active')).not.toBeInTheDocument();
    expect(screen.queryByText('Cleared')).not.toBeInTheDocument();
  });

  // 2026-10-20: 2D8C8B610B654F93BEC3A7B377A9B2A0, listed and uncleared, was
  // "Active" for a closure that starts on Sunday 25 October.
  it('shows Upcoming, with when it starts, for planned work not in effect yet', () => {
    renderWithMantine(
      <IncidentStateBadge
        isCleared={false}
        sourceRemovedAt={null}
        state="upcoming"
        startsAt="2026-10-24T23:00:00+00:00"
        validityEndedAt={null}
      />,
    );
    const badge = screen.getByText(/^Upcoming/).closest('[data-incident-state="upcoming"]');
    expect(badge).not.toBeNull();
    expect(badge).toHaveAttribute('data-state-badge', 'planned');
    expect(badge).toHaveAttribute('title', 'Not in effect yet: starts 25 Oct 2026, 00:00');
    expect(badge).toHaveTextContent('Upcoming: Not in effect yet: starts 25 Oct 2026, 00:00');
    expect(screen.queryByText('Active')).not.toBeInTheDocument();
  });

  it('shows Ended, with when its validity period ended, for a listed incident whose windows have all finished', () => {
    renderWithMantine(
      <IncidentStateBadge
        isCleared={false}
        sourceRemovedAt={null}
        state="ended"
        startsAt={null}
        validityEndedAt="2026-10-18T22:59:00+00:00"
      />,
    );
    const badge = screen.getByText(/^Ended/).closest('[data-incident-state="ended"]');
    expect(badge).toHaveAttribute('title', 'Its validity period ended 18 Oct 2026, 23:59');
    expect(badge).toHaveTextContent('Ended: Its validity period ended 18 Oct 2026, 23:59');
  });

  it("follows the api's state over the flags", () => {
    renderWithMantine(<IncidentStateBadge isCleared={false} sourceRemovedAt={null} state="active" />);
    expect(screen.getByText('Active').closest('[data-incident-state="active"]')).not.toBeNull();
  });

  it('places an incident by its windows against an api that predates state', () => {
    renderWithMantine(
      <IncidentStateBadge
        isCleared={false}
        validityPeriods={[{ fromDate: '2999-01-01T00:00:00Z', toDate: null, isNow: true }]}
      />,
    );
    expect(screen.getByText('Upcoming').closest('[data-incident-state="upcoming"]')).not.toBeNull();
  });
});
