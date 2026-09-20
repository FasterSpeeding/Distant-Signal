import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { EtaBadge } from './EtaBadge';

describe('EtaBadge', () => {
  it('renders nothing when there is no ETA', () => {
    renderWithMantine(<EtaBadge etaNext={null} etaSource={null} />);
    // Not `toBeEmptyDOMElement()` on `container`: MantineProvider injects
    // <style> tags into the render tree, so the container is never
    // literally empty (see RepresentativeInfo.test.tsx for the same
    // workaround on an existing `return null` component). Assert no
    // component content instead.
    expect(screen.queryByText(/ETA/)).not.toBeInTheDocument();
  });

  it('renders nothing if etaSource is somehow missing despite an etaNext value', () => {
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource={null} />);
    expect(screen.queryByText(/ETA/)).not.toBeInTheDocument();
  });

  it('shows a distinct badge for a darwin-estimated ETA', () => {
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="darwin-estimated" />);
    expect(screen.getByText('Live departure board')).toBeInTheDocument();
  });

  it('shows a distinct badge for a trust-propagated ETA', () => {
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" />);
    expect(screen.getByText('Estimate (Network Rail)')).toBeInTheDocument();
  });

  it('the two sources render visibly different badge text', () => {
    const { unmount } = renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="darwin-estimated" />);
    const darwinText = screen.getByText('Live departure board').textContent;
    unmount();
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" />);
    const trustText = screen.getByText('Estimate (Network Rail)').textContent;
    expect(darwinText).not.toBe(trustText);
  });

  // Review §2.9: "propagated" is jargon, so the visible badge no longer
  // says it -- but the precise technical term must still reach anyone who
  // needs it: sighted users via the Tooltip, everyone else via this
  // always-present (but visually hidden) text.
  it('keeps the precise "propagated" technical description available to screen readers', () => {
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" />);
    expect(
      screen.getByText(
        "Estimated by Network Rail's TRUST movement feed, propagated forward from the train's last reported delay",
      ),
    ).toBeInTheDocument();
  });

  // Task 3.6.3: a stale ETA under a "may have arrived" alert must not still
  // read as a live, present-tense time.
  describe('mayHaveArrived', () => {
    it('renders "Was due at {station} {time} (no arrival report received)" instead of the present-tense badge', () => {
      renderWithMantine(
        <EtaBadge
          etaNext="2026-08-28T18:41:00Z"
          etaSource="trust-propagated"
          mayHaveArrived
          destinationCrs="WOK"
          destinationName="Woking"
        />,
      );
      expect(screen.getByText('Was due at Woking 19:41 (no arrival report received)')).toBeInTheDocument();
      expect(screen.queryByText(/^ETA /)).not.toBeInTheDocument();
      expect(screen.queryByText('Estimate (Network Rail)')).not.toBeInTheDocument();
    });

    it('falls back to the bare CRS when no destination name resolved', () => {
      renderWithMantine(
        <EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" mayHaveArrived destinationCrs="WOK" />,
      );
      expect(screen.getByText('Was due at WOK 19:41 (no arrival report received)')).toBeInTheDocument();
    });

    it('omits the station entirely when neither destination CRS nor name is known', () => {
      renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" mayHaveArrived />);
      expect(screen.getByText('Was due 19:41 (no arrival report received)')).toBeInTheDocument();
    });

    it('leaves the ordinary present-tense badge unchanged when mayHaveArrived is false', () => {
      renderWithMantine(
        <EtaBadge
          etaNext="2026-08-28T18:41:00Z"
          etaSource="trust-propagated"
          mayHaveArrived={false}
          destinationCrs="WOK"
          destinationName="Woking"
        />,
      );
      expect(screen.getByText('ETA 19:41')).toBeInTheDocument();
      expect(screen.getByText('Estimate (Network Rail)')).toBeInTheDocument();
      expect(screen.queryByText(/Was due/)).not.toBeInTheDocument();
    });
  });
});
