import { describe, it, expect } from 'vitest';

// FE-13: vitest.setup.ts installs its localStorage polyfill regardless of
// Node version. Node 25+ has its own global `localStorage` (undefined
// without `--localstorage-file`) that used to shadow jsdom's and skip it.
describe('vitest.setup localStorage polyfill', () => {
  it('is a working Storage on both window and the bare global, and the same object', () => {
    expect(typeof localStorage.setItem).toBe('function');
    expect(window.localStorage).toBe(globalThis.localStorage);
    localStorage.setItem('fe13', 'value');
    expect(window.localStorage.getItem('fe13')).toBe('value');
    localStorage.removeItem('fe13');
    expect(localStorage.getItem('fe13')).toBeNull();
    localStorage.setItem('a', '1');
    localStorage.clear();
    expect(localStorage.getItem('a')).toBeNull();
  });
});
