import '@testing-library/jest-dom/vitest';
import { vi } from 'vitest';

// jsdom doesn't implement matchMedia, but Mantine's MantineProvider calls it
// during color-scheme setup. Polyfill it so components can render in tests.
if (typeof window !== 'undefined' && !window.matchMedia) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}

// jsdom doesn't implement ResizeObserver, but Mantine's SegmentedControl uses
// it (via FloatingIndicator) to size/position the selected-segment highlight.
// Polyfill it so components can render in tests.
if (typeof window !== 'undefined' && !window.ResizeObserver) {
  class ResizeObserverStub {
    observe = vi.fn();
    unobserve = vi.fn();
    disconnect = vi.fn();
  }
  window.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
}

// jsdom's `window.localStorage` isn't a working Storage implementation in
// this project's setup (e.g. `localStorage.setItem` isn't even a
// function), but Mantine's color-scheme manager reads/writes it to
// persist the light/dark/auto preference. Polyfill just the methods it
// (and tests calling `localStorage.clear()` between cases) actually use —
// `key`/`length` are part of the Storage interface but nothing here
// exercises them, so they're deliberately omitted.
if (typeof window !== 'undefined' && typeof window.localStorage !== 'undefined') {
  const store: Record<string, string> = {};

  Object.defineProperty(window, 'localStorage', {
    value: {
      getItem(key: string) {
        return store[key] ?? null;
      },
      setItem(key: string, value: string) {
        store[key] = value;
      },
      removeItem(key: string) {
        delete store[key];
      },
      clear() {
        for (const key of Object.keys(store)) {
          delete store[key];
        }
      },
    },
    writable: true,
    configurable: true,
  });
}

