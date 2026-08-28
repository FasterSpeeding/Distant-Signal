import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PrideToggle } from './PrideToggle';

describe('PrideToggle', () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.body.dataset.pride;
  });

  it('starts off by default, with no stored preference', () => {
    renderWithMantine(<PrideToggle />);
    expect(screen.getByLabelText('Pride mode: off. Click to toggle.')).toBeInTheDocument();
    expect(screen.getByRole('button')).toHaveAttribute('aria-pressed', 'false');
  });

  it('turns on when clicked, and sets document.body.dataset.pride for globals.css to key off', () => {
    renderWithMantine(<PrideToggle />);
    fireEvent.click(screen.getByRole('button'));

    expect(screen.getByLabelText('Pride mode: on. Click to toggle.')).toBeInTheDocument();
    expect(screen.getByRole('button')).toHaveAttribute('aria-pressed', 'true');
    expect(document.body.dataset.pride).toBe('true');
  });

  it('toggles back off on a second click', () => {
    renderWithMantine(<PrideToggle />);
    const button = screen.getByRole('button');

    fireEvent.click(button);
    fireEvent.click(button);

    expect(screen.getByLabelText('Pride mode: off. Click to toggle.')).toBeInTheDocument();
    expect(document.body.dataset.pride).toBe('false');
  });

  it('persists the preference in localStorage across remounts', () => {
    const { unmount } = renderWithMantine(<PrideToggle />);
    fireEvent.click(screen.getByRole('button'));
    expect(localStorage.getItem('pride-mode')).toBe('true');
    unmount();

    renderWithMantine(<PrideToggle />);
    expect(screen.getByLabelText('Pride mode: on. Click to toggle.')).toBeInTheDocument();
  });
});
