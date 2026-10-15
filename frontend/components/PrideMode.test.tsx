import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PrideModeApplier, PrideModeSelect, PrideSparkles } from './PrideMode';
import { PRIDE_MODE_OPTIONS, PRIDE_SPARKLES, type PrideMode } from '@/lib/prideMode';

const FLAGS = PRIDE_MODE_OPTIONS.map((option) => option.value).filter(
  (mode): mode is Exclude<PrideMode, 'off'> => mode !== 'off',
);

function sparkleText(container: HTMLElement): (string | null)[] {
  return Array.from(container.querySelectorAll('.prideSparkle')).map((sparkle) => sparkle.textContent);
}

describe('PrideModeSelect', () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.body.dataset.pride;
  });

  it('starts off, with an outlined empty swatch', () => {
    const { container } = renderWithMantine(<PrideModeSelect />);
    expect(screen.getByRole('combobox', { name: /Pride flag/ })).toHaveValue('off');
    expect(container.querySelector('.prideSwatch')).toHaveAttribute('data-flag', 'off');
  });

  it('lists every flag in sentence case', () => {
    renderWithMantine(<PrideModeSelect />);
    const labels = screen.getAllByRole('option').map((option) => option.textContent);
    expect(labels).toEqual([
      'Off',
      'Rainbow',
      'Trans',
      'Non-binary',
      'Bisexual',
      'Pansexual',
      'Asexual',
      'Sapphic',
      'Lesbian',
    ]);
  });

  it.each(PRIDE_MODE_OPTIONS.map((option) => option.value))(
    'applies and saves %s straight away, with its swatch',
    (mode) => {
      const { container } = renderWithMantine(<PrideModeSelect />);
      fireEvent.change(screen.getByRole('combobox', { name: /Pride flag/ }), { target: { value: mode } });
      expect(document.body.dataset.pride).toBe(mode);
      expect(localStorage.getItem('pride-mode')).toBe(mode);
      expect(container.querySelector('.prideSwatch')).toHaveAttribute('data-flag', mode);
    },
  );

  it('previews the flag beside the picker: its swatch, ring and sparkles, decorative only', () => {
    const { container } = renderWithMantine(<PrideModeSelect />);
    const preview = container.querySelector('.pridePreview');
    expect(preview).toHaveAttribute('data-flag', 'off');
    expect(preview?.closest('[aria-hidden="true"]')).not.toBeNull();
    expect(container.querySelector('.prideSparkles')).not.toBeInTheDocument();

    fireEvent.change(screen.getByRole('combobox', { name: /Pride flag/ }), { target: { value: 'nonbinary' } });
    expect(container.querySelector('.pridePreview')).toHaveAttribute('data-flag', 'nonbinary');
    expect(sparkleText(container)).toEqual(['💛', '🤍', '💜']);

    fireEvent.change(screen.getByRole('combobox', { name: /Pride flag/ }), { target: { value: 'off' } });
    expect(container.querySelector('.prideSparkles')).not.toBeInTheDocument();
  });

  it('reads the saved flag after mount', () => {
    localStorage.setItem('pride-mode', 'sapphic');
    renderWithMantine(<PrideModeSelect />);
    expect(screen.getByRole('combobox', { name: /Pride flag/ })).toHaveValue('sapphic');
  });
});

describe('PrideModeApplier', () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.body.dataset.pride;
  });

  it('applies the saved flag with no control on screen', () => {
    localStorage.setItem('pride-mode', 'trans');
    const { container } = renderWithMantine(<PrideModeApplier />);
    expect(document.body.dataset.pride).toBe('trans');
    expect(container.querySelector('select, button, input')).toBeNull();
  });
});

describe('PrideSparkles', () => {
  // Which scatter shows is CSS (`body[data-pride]` against each scatter's
  // `data-flag`, in globals.css), so it is there on the first paint; the
  // e2e spec checks what is on screen.
  it('renders each flag its own decorative trio, keyed by flag, and none for off', () => {
    const { container } = renderWithMantine(<PrideSparkles />);
    const scatters = Array.from(container.querySelectorAll<HTMLElement>('.prideSparkles'));
    expect(scatters.map((scatter) => scatter.dataset.flag)).toEqual(FLAGS);
    for (const scatter of scatters) {
      expect(scatter).toHaveAttribute('aria-hidden', 'true');
      const mode = scatter.dataset.flag as Exclude<PrideMode, 'off'>;
      expect(sparkleText(scatter)).toEqual([...PRIDE_SPARKLES[mode]]);
    }
  });

  it('gives each flag its own trio', () => {
    const trios = new Set(FLAGS.map((mode) => PRIDE_SPARKLES[mode].join('')));
    expect(trios.size).toBe(FLAGS.length);
  });
});
