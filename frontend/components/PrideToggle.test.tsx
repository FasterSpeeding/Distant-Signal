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

  it('cycles off -> rainbow -> trans -> nonbinary -> bisexual -> pansexual -> asexual -> sapphic -> lesbian -> off, setting document.body.dataset.pride at each step', () => {
    const { container } = renderWithMantine(<PrideToggle />);
    const button = screen.getByRole('button');

    const cycle: Array<[string, string]> = [
      ['rainbow', '🏳️‍🌈'],
      ['trans', '🏳️‍⚧️'],
      ['nonbinary', '🏳️'],
      ['bisexual', '🏳️'],
      ['pansexual', '🏳️'],
      ['asexual', '🏳️'],
      ['sapphic', '🏳️'],
      ['lesbian', '🏳️'],
    ];

    for (const [mode, emoji] of cycle) {
      fireEvent.click(button);
      expect(screen.getByLabelText(`Pride mode: ${mode}. Click to toggle.`)).toBeInTheDocument();
      expect(button).toHaveAttribute('aria-pressed', 'true');
      expect(document.body.dataset.pride).toBe(mode);
      expect(button).toHaveTextContent(emoji);
      expect(container.querySelector('.prideSparkles')).toBeInTheDocument();
    }

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

  it('shows the plain white flag glyph, with a distinct sparkle set, for each of the newer flags without a dedicated emoji sequence', () => {
    const { container } = renderWithMantine(<PrideToggle />);
    const button = screen.getByRole('button');

    // off -> rainbow -> trans -> nonbinary
    fireEvent.click(button);
    fireEvent.click(button);
    fireEvent.click(button);
    expect(screen.getByLabelText('Pride mode: nonbinary. Click to toggle.')).toBeInTheDocument();
    expect(button).toHaveTextContent('🏳️');
    const nonbinarySparkles = container.querySelectorAll('.prideSparkle');
    expect(nonbinarySparkles).toHaveLength(3);
    expect(Array.from(nonbinarySparkles).map((s) => s.textContent)).toEqual(['💛', '🤍', '💜']);

    // -> bisexual
    fireEvent.click(button);
    expect(screen.getByLabelText('Pride mode: bisexual. Click to toggle.')).toBeInTheDocument();
    expect(button).toHaveTextContent('🏳️');
    const bisexualSparkles = container.querySelectorAll('.prideSparkle');
    expect(Array.from(bisexualSparkles).map((s) => s.textContent)).toEqual(['💗', '💜', '💙']);

    // -> pansexual
    fireEvent.click(button);
    expect(screen.getByLabelText('Pride mode: pansexual. Click to toggle.')).toBeInTheDocument();
    expect(button).toHaveTextContent('🏳️');
    const pansexualSparkles = container.querySelectorAll('.prideSparkle');
    expect(Array.from(pansexualSparkles).map((s) => s.textContent)).toEqual(['💗', '💛', '💙']);
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

  it('persists a newer flag mode (sapphic) in localStorage across remounts', () => {
    const { unmount } = renderWithMantine(<PrideToggle />);
    const button = screen.getByRole('button');
    // off -> rainbow -> trans -> nonbinary -> bisexual -> pansexual -> asexual -> sapphic
    for (let i = 0; i < 7; i++) fireEvent.click(button);
    expect(localStorage.getItem('pride-mode')).toBe('sapphic');
    unmount();

    renderWithMantine(<PrideToggle />);
    expect(screen.getByLabelText('Pride mode: sapphic. Click to toggle.')).toBeInTheDocument();
  });

  it('treats a pre-existing boolean-era stored value as rainbow, not off', () => {
    localStorage.setItem('pride-mode', 'true');
    renderWithMantine(<PrideToggle />);
    expect(screen.getByLabelText('Pride mode: rainbow. Click to toggle.')).toBeInTheDocument();
  });
});
