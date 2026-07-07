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
