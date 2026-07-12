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

// jsdom's localStorage doesn't have a clear method. Mantine's color-scheme manager
// needs to store and clear color scheme preferences. Polyfill clear() so tests work.
if (typeof window !== 'undefined' && typeof window.localStorage !== 'undefined') {
  const store: Record<string, string> = {};

  // Replace localStorage with a full implementation
  Object.defineProperty(window, 'localStorage', {
    value: {
      length: 0 as number,
      getItem(key: string) {
        return store[key] ?? null;
      },
      setItem(key: string, value: string) {
        if (!store[key]) {
          store[key] = value;
        } else {
          store[key] = value;
        }
      },
      removeItem(key: string) {
        delete store[key];
      },
      clear() {
        for (const key of Object.keys(store)) {
          delete store[key];
        }
      },
      key(index: number) {
        const keys = Object.keys(store);
        return keys[index] ?? null;
      },
    },
    writable: true,
    configurable: true,
  });

  // Make length a computed property
  Object.defineProperty(window.localStorage, 'length', {
    get() {
      return Object.keys(store).length;
    },
  });
}

