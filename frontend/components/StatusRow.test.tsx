import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { Badge, Button } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { expectShrinkGuarded, expectNoUnguardedNowrapBadges } from '@/test/shrinkGuard';
import { StatusRow } from './StatusRow';

describe('StatusRow', () => {
  it('renders a string title as text', () => {
    renderWithMantine(<StatusRow title="West Coast Main Line" />);
    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
  });

  it('renders no subtitle when none is given', () => {
    const { container } = renderWithMantine(<StatusRow title="Bakerloo" />);
    // Only the title's own Text node should be present -- no extra Stack
    // wrapper was introduced for a subtitle that was never asked for.
    expect(container.querySelectorAll('.mantine-Text-root')).toHaveLength(1);
  });

  it('renders the subtitle under the title when given', () => {
    renderWithMantine(<StatusRow title="Bakerloo" subtitle="Shared by Alex" />);
    expect(screen.getByText('Bakerloo')).toBeInTheDocument();
    expect(screen.getByText('Shared by Alex')).toBeInTheDocument();
  });

  it('renders trailing content', () => {
    renderWithMantine(<StatusRow title="Bakerloo" trailing={<Badge>Good Service</Badge>} />);
    expect(screen.getByText('Good Service')).toBeInTheDocument();
  });

  it('renders nothing extra when trailing is nullish or false', () => {
    const { container: withNull } = renderWithMantine(<StatusRow title="Bakerloo" trailing={null} />);
    expect(withNull.querySelector('[data-status-row-trailing]')).not.toBeInTheDocument();

    const { container: withFalse } = renderWithMantine(<StatusRow title="Bakerloo" trailing={false} />);
    expect(withFalse.querySelector('[data-status-row-trailing]')).not.toBeInTheDocument();
  });

  it('renders a non-string title node as-is, without adding its own Text wrapper', () => {
    renderWithMantine(
      <StatusRow
        title={
          <a href="/lines/wcml" data-testid="custom-title-link">
            West Coast Main Line
          </a>
        }
      />,
    );
    const link = screen.getByTestId('custom-title-link');
    expect(link).toHaveTextContent('West Coast Main Line');
    // A composite title isn't auto-wrapped in the component's own
    // `fw`/`lineClamp` Text -- it renders exactly what was passed.
    expect(link.closest('.mantine-Text-root')).toBeNull();
  });

  it('marks the outer row with the data-wrap="nowrap" convention other rows in this codebase use', () => {
    const { container } = renderWithMantine(<StatusRow title="Bakerloo" trailing={<Badge>Good Service</Badge>} />);
    expect(container.querySelector('[data-status-row][data-wrap="nowrap"]')).toBeInTheDocument();
  });

  describe('the shrink guard (WCAG 2.5.3)', () => {
    it('gives a string title minWidth: 0 so it is the side that shrinks', () => {
      renderWithMantine(<StatusRow title="A very long line name that could overflow" trailing={<Badge>Bad</Badge>} />);
      const title = screen.getByText('A very long line name that could overflow');
      expect(title.style.minWidth).toBe('0');
    });

    it('gives the trailing wrapper flexShrink: 0 so a badge cannot be crushed', () => {
      const { container } = renderWithMantine(
        <StatusRow title="A very long line name that could overflow the row" trailing={<Badge>Good Service</Badge>} />,
      );
      const trailing = container.querySelector('[data-status-row-trailing]');
      expect(trailing).toHaveStyle({ flexShrink: '0' });
      expectShrinkGuarded(screen.getByText('Good Service'));
    });

    it('gives the trailing wrapper flexShrink: 0 for an action button too', () => {
      renderWithMantine(<StatusRow title="Some train" trailing={<Button>Remove</Button>} />);
      expectShrinkGuarded(screen.getByRole('button', { name: 'Remove' }));
    });

    it('guards a trailing group of several badges together', () => {
      renderWithMantine(
        <StatusRow
          title="My Commute"
          trailing={
            <>
              <Badge>Severe Delays</Badge>
              <Badge>Diverted</Badge>
            </>
          }
        />,
      );
      expectShrinkGuarded(screen.getByText('Severe Delays'));
      expectShrinkGuarded(screen.getByText('Diverted'));
    });

    it('passes the generic "no unguarded badge inside a wrap=nowrap row" sweep', () => {
      const { container } = renderWithMantine(
        <>
          <StatusRow title="Bakerloo" trailing={<Badge>Good Service</Badge>} />
          <StatusRow title="Victoria" subtitle="Shared by Alex" trailing={<Badge>Minor Delays</Badge>} />
        </>,
      );
      expect(() => expectNoUnguardedNowrapBadges(container)).not.toThrow();
    });

    it('still guards the trailing content when title and subtitle are both present', () => {
      const { container } = renderWithMantine(
        <StatusRow
          title="A shared train with a name long enough to threaten the button"
          subtitle="Shared by a member of the family group"
          trailing={<Button>Remove from group</Button>}
        />,
      );
      const trailing = container.querySelector('[data-status-row-trailing]');
      expect(trailing).toHaveStyle({ flexShrink: '0' });
      expectShrinkGuarded(screen.getByRole('button', { name: 'Remove from group' }));
    });
  });
});
