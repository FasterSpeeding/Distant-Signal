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
});
