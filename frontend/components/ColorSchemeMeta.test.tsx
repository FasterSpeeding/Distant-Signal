import { describe, it, expect, afterEach, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ThemeToggle } from './ThemeToggle';
import { ColorSchemeMeta } from './ColorSchemeMeta';

function colorSchemeMetaTags() {
  return document.head.querySelectorAll('meta[name="color-scheme"]');
}

describe('ColorSchemeMeta', () => {
  beforeEach(() => {
    // Same reason ThemeToggle.test.tsx clears this in its own beforeEach:
    // Mantine's stored preference (localStorage['mantine-color-scheme-value'])
    // persists across tests within this file otherwise, so a value one test
    // seeds (e.g. 'dark') would silently leak into and desync the next.
    localStorage.clear();
  });

  afterEach(() => {
    // This component's whole job is to mutate document.head outside
    // React's own render tree — @testing-library/react's automatic
    // cleanup unmounts the component but never removes a tag it appended
    // itself, so each test must undo that by hand or the next test starts
    // from a dirty document.head.
    colorSchemeMetaTags().forEach((tag) => tag.remove());
  });

  it('creates the meta tag with content="light" once mounted, matching the default resolved scheme', () => {
    renderWithMantine(<ColorSchemeMeta />);
    const tags = colorSchemeMetaTags();
    expect(tags).toHaveLength(1);
    expect(tags[0]).toHaveAttribute('content', 'light');
  });

  it('sets content="dark" when the stored preference resolves to dark', () => {
    // Same persistence key ThemeToggle.test.tsx's own SSR test seeds
    // (mantine-color-scheme-value) — Mantine reads it directly on mount,
    // no provider prop needed to set this up.
    localStorage.setItem('mantine-color-scheme-value', 'dark');
    renderWithMantine(<ColorSchemeMeta />);
    const tags = colorSchemeMetaTags();
    expect(tags).toHaveLength(1);
    expect(tags[0]).toHaveAttribute('content', 'dark');
  });

  it('updates content as the resolved scheme changes, without ever duplicating the tag', () => {
    // Mounted alongside ThemeToggle, same as production (both sit inside
    // MantineProvider in app/layout.tsx) — clicking ThemeToggle's button
    // is what actually changes the resolved scheme; ColorSchemeMeta has no
    // UI of its own to drive this directly.
    renderWithMantine(
      <>
        <ColorSchemeMeta />
        <ThemeToggle />
      </>,
      { defaultColorScheme: 'auto' },
    );
    // matchMedia is polyfilled (vitest.setup.ts) to report no dark
    // preference, so 'auto' resolves to light first.
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'light');

    const button = screen.getByRole('button');
    fireEvent.click(button); // auto -> light
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'light');

    fireEvent.click(button); // light -> dark
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'dark');

    fireEvent.click(button); // dark -> auto
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'light');
  });

  it("reuses an already-present tag (as Next's SSR-rendered viewport tag would be) instead of creating a duplicate", () => {
    const existing = document.createElement('meta');
    existing.setAttribute('name', 'color-scheme');
    existing.setAttribute('content', 'light');
    document.head.appendChild(existing);

    localStorage.setItem('mantine-color-scheme-value', 'dark');
    renderWithMantine(<ColorSchemeMeta />);

    const tags = colorSchemeMetaTags();
    expect(tags).toHaveLength(1);
    expect(tags[0]).toBe(existing);
    expect(tags[0]).toHaveAttribute('content', 'dark');
  });
});
