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

  it('cycles off -> rainbow -> trans -> off, setting document.body.dataset.pride at each step', () => {
    const { container } = renderWithMantine(<PrideToggle />);
    const button = screen.getByRole('button');

    fireEvent.click(button);
    expect(screen.getByLabelText('Pride mode: rainbow. Click to toggle.')).toBeInTheDocument();
    expect(button).toHaveAttribute('aria-pressed', 'true');
    expect(document.body.dataset.pride).toBe('rainbow');
    expect(container.querySelector('.prideSparkles')).toBeInTheDocument();

    fireEvent.click(button);
    expect(screen.getByLabelText('Pride mode: trans. Click to toggle.')).toBeInTheDocument();
    expect(button).toHaveAttribute('aria-pressed', 'true');
    expect(document.body.dataset.pride).toBe('trans');
    expect(container.querySelector('.prideSparkles')).toBeInTheDocument();

    fireEvent.click(button);
    expect(screen.getByLabelText('Pride mode: off. Click to toggle.')).toBeInTheDocument();
    expect(button).toHaveAttribute('aria-pressed', 'false');
    expect(document.body.dataset.pride).toBe('off');
    expect(container.querySelector('.prideSparkles')).not.toBeInTheDocument();
  });

  it('shows a different emoji and sparkle set for rainbow vs. trans', () => {
    renderWithMantine(<PrideToggle />);
    const button = screen.getByRole('button');

    fireEvent.click(button);
    expect(button).toHaveTextContent('🏳️‍🌈');

    fireEvent.click(button);
    expect(button).toHaveTextContent('🏳️‍⚧️');
  });

  it('persists the preference in localStorage across remounts', () => {
    const { unmount } = renderWithMantine(<PrideToggle />);
    fireEvent.click(screen.getByRole('button')); // -> rainbow
    fireEvent.click(screen.getByRole('button')); // -> trans
    expect(localStorage.getItem('pride-mode')).toBe('trans');
    unmount();

    renderWithMantine(<PrideToggle />);
    expect(screen.getByLabelText('Pride mode: trans. Click to toggle.')).toBeInTheDocument();
  });

  it('treats a pre-existing boolean-era stored value as rainbow, not off', () => {
    localStorage.setItem('pride-mode', 'true');
    renderWithMantine(<PrideToggle />);
    expect(screen.getByLabelText('Pride mode: rainbow. Click to toggle.')).toBeInTheDocument();
  });
});
