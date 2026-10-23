import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ButtonLink } from './ButtonLink';

describe('ButtonLink', () => {
  it('renders one <a> drawn as a button, not a <button> inside a link', () => {
    renderWithMantine(
      <ButtonLink href="/groups/new" variant="default" size="xs">
        Create a group
      </ButtonLink>,
    );
    const link = screen.getByRole('link', { name: 'Create a group' });
    expect(link).toHaveAttribute('href', '/groups/new');
    expect(link).toHaveClass('mantine-Button-root');
    expect(link).toHaveAttribute('data-variant', 'default');
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});
