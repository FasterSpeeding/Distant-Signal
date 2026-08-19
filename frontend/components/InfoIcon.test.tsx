import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { InfoIcon } from './InfoIcon';

describe('InfoIcon', () => {
  it('renders a decorative svg, leaving the accessible name to its trigger', () => {
    const { container } = render(<InfoIcon />);
    const svg = container.querySelector('svg');
    expect(svg).toBeInTheDocument();
    expect(svg).toHaveAttribute('aria-hidden', 'true');
    expect(svg).not.toHaveAttribute('aria-label');
  });
});
