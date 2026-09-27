import '@testing-library/jest-dom/vitest';
import { afterAll, vi } from 'vitest';
import { installTimerLeakGuard } from './test/timerLeakGuard';

// Note on theme parity: this file runs once before test *modules* load, so
// it can't inject props into a component tree — there's no JSX here to
// wrap. Each test file wraps its subject in its own local `MantineProvider`
// (usually via a `renderWithProvider` helper); that's where
// `theme={theme}` (from `lib/theme.ts`, the same object `app/layout.tsx`
// passes in production) actually needs to be threaded through, and is.

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

// jsdom doesn't implement `scrollIntoView`, but Mantine's Combobox
// (`useCombobox`, backing `Select`/`Autocomplete`) calls it internally, on a
// timer, to keep the active/highlighted option visible -- previously never
// triggered by an existing test, but `TrackDestinationModal`'s `Select`
// (the shared-groups "Personal or a group?" picker) exercises it whenever a
// test picks a group option, leaving an uncaught
// `items[index]?.scrollIntoView is not a function` once that timer fires.
// Same "polyfill the missing jsdom API" pattern as ResizeObserver/
// matchMedia above.
if (typeof window !== 'undefined' && !window.HTMLElement.prototype.scrollIntoView) {
  window.HTMLElement.prototype.scrollIntoView = vi.fn();
}

// jsdom doesn't implement `Element.prototype.scrollTo` either (only
// `window.scrollTo`, as a no-op). `JourneyProgress` calls it on its own
// horizontal scroll box to center the "you are here" marker -- deliberately
// instead of `scrollIntoView`, which would also scroll the page. Same
// "polyfill the missing jsdom API" pattern as the block above; tests that
// assert on the call replace this stub with their own `vi.fn()`.
if (typeof window !== 'undefined' && !window.Element.prototype.scrollTo) {
  window.Element.prototype.scrollTo = vi.fn();
}

// jsdom's `window.localStorage` isn't a working Storage implementation in
// this project's setup (e.g. `localStorage.setItem` isn't even a
// function), but Mantine's color-scheme manager reads/writes it to
// persist the light/dark/auto preference. Polyfill just the methods it
// (and tests calling `localStorage.clear()` between cases) actually use —
// `key`/`length` are part of the Storage interface but nothing here
// exercises them, so they're deliberately omitted.
//
// FE-13: installed UNCONDITIONALLY, on both `window` and `globalThis`. This
// used to be guarded by `typeof window.localStorage !== 'undefined'`. Node
// 25+ ships its own global `localStorage`, which is `undefined` unless
// `--localstorage-file` is given, and it shadows jsdom's -- so the guard was
// false, the polyfill was skipped, and every test touching storage threw.
if (typeof window !== 'undefined') {
  const store: Record<string, string> = {};
  const storage = {
    getItem(key: string) {
      return store[key] ?? null;
    },
    setItem(key: string, value: string) {
      store[key] = String(value);
    },
    removeItem(key: string) {
      delete store[key];
    },
    clear() {
      for (const key of Object.keys(store)) {
        delete store[key];
      }
    },
  };
  for (const target of new Set<object>([window, globalThis])) {
    Object.defineProperty(target, 'localStorage', {
      value: storage,
      writable: true,
      configurable: true,
    });
  }
}

// Cancel any Mantine transition timer a test file leaked BEFORE Vitest
// tears the jsdom environment down (which deletes `window` from the
// global scope).
//
// Mantine's `useTransition` (Modal, Popover, Menu, Collapse, ...) runs each
// transition as rAF -> `flushSync(setStatus)` -> rAF -> `setTimeout(setStatus,
// duration)`. If that `flushSync` re-render unmounts the transitioning
// component, the unmount cleanup cancels only the rAF that is already
// running, so the second rAF still fires and schedules a real `setTimeout`
// nothing ever clears. When it fires, React's `dispatchSetState` reads
// `window.event` -- harmless while the environment is alive, but an
// uncaught `ReferenceError: window is not defined` (an "Unhandled Error"
// that fails `npm test`) if the file's environment has already been torn
// down. The leak predates Vitest 4, but only under Vitest 4 does the
// worker stay alive past teardown long enough for such a timer to fire
// there.
//
// FE-9: this used to SLEEP 50 ms + 300 ms after every file, waiting for the
// leaked timers to fire -- ~0.35 s per file, and timing-dependent under
// load. Instead, every real timer and animation frame is tracked while it
// is pending, and whatever is still pending once the file's tests (and its
// own afterAll hooks, which run before this one) are done is cancelled:
// animation frames first, so none can schedule a new timer, then timers.
// Fake timers are unaffected: `vi.useFakeTimers()` replaces these globals
// and `vi.useRealTimers()` puts these tracking wrappers back.
const timerLeakGuard = installTimerLeakGuard();

afterAll(() => {
  if (vi.isFakeTimers()) return;
  timerLeakGuard.cancelPending();
});
