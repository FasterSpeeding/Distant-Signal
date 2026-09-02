import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TextLink } from './TextLink';

// Real next/link renders a plain <a> in jsdom (no router context needed),
// so every existing DOM-attribute assertion below still holds against this
// mock -- the only thing it adds is surfacing `prefetch`, which next/link
// itself deliberately does NOT forward to the rendered element (it's a
// component-only prop consumed by the router's IntersectionObserver
// prefetch logic), so there is no other way to assert it landed on the
// `<Link>` element at all.
vi.mock('next/link', () => ({
  default: ({
    href,
    children,
    prefetch,
    ...rest
  }: {
    href: string;
    children: React.ReactNode;
    prefetch?: boolean;
    [key: string]: unknown;
  }) => (
    <a href={href} data-prefetch={String(prefetch)} {...rest}>
      {children}
    </a>
  ),
}));

describe('TextLink', () => {
  it('forwards an explicit prefetch={false} through to next/link (regression: LoginLink relies on this to stop a real, side-effecting backend request from firing on mere link visibility)', () => {
    renderWithMantine(
      <TextLink href="/api/auth/login?return_to=%2F" prefetch={false}>
        Log in
      </TextLink>,
    );
    const link = screen.getByRole('link', { name: 'Log in' });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2F');
    expect(link).toHaveAttribute('data-prefetch', 'false');
  });

  it('leaves prefetch as next/link\'s own default (undefined) when not specified', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);
    expect(screen.getByRole('link', { name: 'All Lines' })).toHaveAttribute('data-prefetch', 'undefined');
  });

  it('renders an anchor to the href carrying the link colour', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);

    const link = screen.getByRole('link', { name: 'All Lines' });
    expect(link).toHaveAttribute('href', '/lines');
    // The seven hand-rolled call sites this replaces all set this colour
    // directly; the rendered result must not change.
    expect(screen.getByText('All Lines')).toHaveStyle({ color: 'var(--mantine-color-anchor)' });
  });

  it('defaults to an underline on hover and focus only', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);
    // jsdom applies no stylesheet, so the hook the rules in globals.css
    // hang off is what's assertable here (globals.test.ts asserts the
    // rules themselves).
    expect(screen.getByRole('link', { name: 'All Lines' })).toHaveAttribute('data-text-link', 'hover');
  });

  it('can opt into an always-on underline for links sitting among body text', () => {
    renderWithMantine(
      <TextLink href="/lines/wcml" underline="always">
        Back to line
      </TextLink>,
    );

    expect(screen.getByRole('link', { name: 'Back to line' })).toHaveAttribute('data-text-link', 'always');
  });

  it('renders no target/rel by default (regression: every existing call site omits them)', () => {
    renderWithMantine(<TextLink href="/lines">All Lines</TextLink>);
    const link = screen.getByRole('link', { name: 'All Lines' });
    expect(link).not.toHaveAttribute('target');
    expect(link).not.toHaveAttribute('rel');
  });

  it('can opt into target/rel for an external link', () => {
    renderWithMantine(
      <TextLink href="https://example.com" target="_blank" rel="noopener noreferrer">
        External
      </TextLink>,
    );
    const link = screen.getByRole('link', { name: 'External' });
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
  });
});
