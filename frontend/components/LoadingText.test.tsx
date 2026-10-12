import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Button } from '@mantine/core';
import { renderWithMantine } from '@/test/render';
import { ButtonSpinner, LoadingText } from './LoadingText';

describe('LoadingText', () => {
  it('shows the plain words beside an aria-hidden spinner, with no ellipsis', () => {
    const { container } = renderWithMantine(<LoadingText>Searching</LoadingText>);
    expect(screen.getByText('Searching')).toBeVisible();
    expect(container.textContent).not.toContain('…');
    const loader = container.querySelector('.mantine-Loader-root');
    expect(loader).toHaveAttribute('aria-hidden', 'true');
  });

  it('passes a live-region role to the words', () => {
    renderWithMantine(
      <LoadingText role="status" aria-live="polite">
        Finding stations near you
      </LoadingText>,
    );
    expect(screen.getByRole('status')).toHaveTextContent('Finding stations near you');
  });
});

describe('ButtonSpinner', () => {
  it('leaves the button named by its label alone', () => {
    renderWithMantine(<Button leftSection={<ButtonSpinner />}>Saving</Button>);
    expect(screen.getByRole('button', { name: 'Saving' })).toBeInTheDocument();
  });
});
