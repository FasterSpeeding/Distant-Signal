import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { StatusBadge } from './StatusBadge';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

describe('StatusBadge', () => {
  it('renders the label for Good Service', () => {
    renderWithProvider(<StatusBadge severity={10} />);
    expect(screen.getByText('Good Service')).toBeInTheDocument();
  });

  it('renders the label for Severe Delays', () => {
    renderWithProvider(<StatusBadge severity={6} />);
    expect(screen.getByText('Severe Delays')).toBeInTheDocument();
  });

  it('renders a fallback label for an unrecognized severity', () => {
    renderWithProvider(<StatusBadge severity={999} />);
    expect(screen.getByText('Unknown')).toBeInTheDocument();
  });
});
