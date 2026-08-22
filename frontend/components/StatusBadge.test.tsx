import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { StatusBadge } from './StatusBadge';

describe('StatusBadge', () => {
  it('renders the label for Good Service', () => {
    renderWithMantine(<StatusBadge severity={10} />);
    expect(screen.getByText('Good Service')).toBeInTheDocument();
  });

  it('renders the label for Severe Delays', () => {
    renderWithMantine(<StatusBadge severity={6} />);
    expect(screen.getByText('Severe Delays')).toBeInTheDocument();
  });

  it('renders a fallback label for an unrecognized severity', () => {
    renderWithMantine(<StatusBadge severity={999} />);
    expect(screen.getByText('Unknown')).toBeInTheDocument();
  });

  it('marks the badge so it can be opted out of Mantine\'s label truncation', () => {
    const { container } = renderWithMantine(<StatusBadge severity={10} />);
    expect(container.querySelector('[data-status-badge]')).not.toBeNull();
  });
});
