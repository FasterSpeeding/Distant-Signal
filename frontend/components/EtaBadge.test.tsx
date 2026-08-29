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
    expect(screen.getByText('Network Rail propagated')).toBeInTheDocument();
  });

  it('the two sources render visibly different badge text', () => {
    const { unmount } = renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="darwin-estimated" />);
    const darwinText = screen.getByText('Live departure board').textContent;
    unmount();
    renderWithMantine(<EtaBadge etaNext="2026-08-28T18:41:00Z" etaSource="trust-propagated" />);
    const trustText = screen.getByText('Network Rail propagated').textContent;
    expect(darwinText).not.toBe(trustText);
  });
});
