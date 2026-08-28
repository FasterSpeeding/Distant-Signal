import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PrideToggle } from './PrideToggle';

describe('PrideToggle', () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.body.dataset.pride;
  });

  it('starts off by default, with no stored preference, and no sparkles', () => {
    const { container } = renderWithMantine(<PrideToggle />);
    expect(screen.getByLabelText('Pride mode: off. Click to toggle.')).toBeInTheDocument();
    expect(screen.getByRole('button')).toHaveAttribute('aria-pressed', 'false');
    expect(container.querySelector('.prideSparkles')).not.toBeInTheDocument();
  });

  it('turns on when clicked, sets document.body.dataset.pride for globals.css to key off, and shows sparkles', () => {
    const { container } = renderWithMantine(<PrideToggle />);
    fireEvent.click(screen.getByRole('button'));

    expect(screen.getByLabelText('Pride mode: on. Click to toggle.')).toBeInTheDocument();
    expect(screen.getByRole('button')).toHaveAttribute('aria-pressed', 'true');
    expect(document.body.dataset.pride).toBe('true');

    const sparkles = container.querySelector('.prideSparkles');
    expect(sparkles).toBeInTheDocument();
    expect(sparkles).toHaveAttribute('aria-hidden', 'true');
  });

  it('toggles back off on a second click, and hides the sparkles again', () => {
    const { container } = renderWithMantine(<PrideToggle />);
    const button = screen.getByRole('button');

    fireEvent.click(button);
    fireEvent.click(button);

    expect(screen.getByLabelText('Pride mode: off. Click to toggle.')).toBeInTheDocument();
    expect(document.body.dataset.pride).toBe('false');
    expect(container.querySelector('.prideSparkles')).not.toBeInTheDocument();
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
