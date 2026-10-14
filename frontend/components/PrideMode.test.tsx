import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PRIDE_MODE_OPTIONS, PrideModeApplier, PrideModeSelect, parseStoredMode } from './PrideMode';

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

  it('reads the saved flag after mount', () => {
    localStorage.setItem('pride-mode', 'sapphic');
    renderWithMantine(<PrideModeSelect />);
    expect(screen.getByRole('combobox', { name: /Pride flag/ })).toHaveValue('sapphic');
  });
});

describe('parseStoredMode', () => {
  it('maps the old boolean value to rainbow and anything unknown to off', () => {
    expect(parseStoredMode('true')).toBe('rainbow');
    expect(parseStoredMode('false')).toBe('off');
    expect(parseStoredMode(null)).toBe('off');
    expect(parseStoredMode('bogus')).toBe('off');
    expect(parseStoredMode('trans')).toBe('trans');
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
