import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { StatusBadge } from './StatusBadge';

describe('StatusBadge', () => {
  it('renders the label for Good Service', () => {
    renderWithMantine(<StatusBadge severity={10} />);
    expect(screen.getByText('Good service')).toBeInTheDocument();
  });

  it('renders the label for Severe Delays', () => {
    renderWithMantine(<StatusBadge severity={6} />);
    expect(screen.getByText('Severe delays')).toBeInTheDocument();
  });

  it('renders a fallback label for an unrecognized severity', () => {
    renderWithMantine(<StatusBadge severity={999} />);
    expect(screen.getByText('Unknown')).toBeInTheDocument();
  });

  it("is sentence case, not Mantine's uppercase", () => {
    const { container } = renderWithMantine(<StatusBadge severity={9} />);
    expect(screen.getByText('Minor delays')).toBeInTheDocument();
    expect(container.querySelector('.mantine-Badge-root')).toHaveStyle({ textTransform: 'none' });
  });

  // The colours themselves are measured in app/globals.test.ts ("status
  // pill contrast"); this pins that the badge paints with those tokens, not
  // Mantine's filled shade and autoContrast's black.
  it.each([
    [10, 'good'],
    [0, 'informational'],
    [4, 'planned'],
    [9, 'mild'],
    [6, 'severe'],
    [999, 'informational'],
  ] as const)('paints severity %i with the %s fill and label tokens', (severity, group) => {
    const { container } = renderWithMantine(<StatusBadge severity={severity} />);
    const root = container.querySelector<HTMLElement>('.mantine-Badge-root')!;
    expect(root.dataset.severityGroup).toBe(group);
    expect(root.style.getPropertyValue('--badge-bg')).toBe(`var(--ds-status-badge-${group}-fill)`);
    expect(root.style.color).toBe(`var(--ds-status-badge-${group}-text)`);
  });

  it("is 12px, a step up from Mantine's 11px badge text", () => {
    const { container } = renderWithMantine(<StatusBadge severity={6} />);
    expect(container.querySelector('.mantine-Badge-root')).toHaveStyle({
      fontSize: 'var(--mantine-font-size-xs)',
    });
  });

  it("marks the badge so it can be opted out of Mantine's label truncation", () => {
    const { container } = renderWithMantine(<StatusBadge severity={10} />);
    expect(container.querySelector('[data-status-badge]')).not.toBeNull();
  });
});
