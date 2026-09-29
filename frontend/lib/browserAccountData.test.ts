import { describe, it, expect, beforeEach, vi } from 'vitest';
import { BROWSER_ACCOUNT_KEYS, clearBrowserAccountData } from './browserAccountData';

// DQ5 (FE-3/LEG-11/LEG-26).
describe('clearBrowserAccountData', () => {
  beforeEach(() => localStorage.clear());

  it('covers every MCP OAuth key and the Anthropic key', () => {
    expect([...BROWSER_ACCOUNT_KEYS].sort()).toEqual(
      [
        'ds-anthropic-api-key',
        'ds-mcp-oauth:client-information',
        'ds-mcp-oauth:client-saved-at',
        'ds-mcp-oauth:code-verifier',
        'ds-mcp-oauth:oauth-state',
        'ds-mcp-oauth:tokens',
      ].sort(),
    );
  });

  it('removes those keys and leaves unrelated ones', () => {
    for (const key of BROWSER_ACCOUNT_KEYS) localStorage.setItem(key, 'x');
    localStorage.setItem('ds-pride', 'on');
    clearBrowserAccountData();
    for (const key of BROWSER_ACCOUNT_KEYS) expect(localStorage.getItem(key)).toBeNull();
    expect(localStorage.getItem('ds-pride')).toBe('on');
  });

  it('also sweeps any other ds-mcp-oauth:* key when storage can enumerate keys', () => {
    const store = new Map<string, string>([
      ['ds-mcp-oauth:future-key', 'x'],
      ['ds-mcp-oauth:tokens', 'x'],
      ['keep-me', 'y'],
    ]);
    const storage = {
      get length() {
        return store.size;
      },
      key: (i: number) => [...store.keys()][i] ?? null,
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
    };
    vi.stubGlobal('localStorage', storage);
    try {
      clearBrowserAccountData();
      expect([...store.keys()]).toEqual(['keep-me']);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('never throws when storage is blocked', () => {
    vi.stubGlobal('localStorage', {
      get length(): number {
        throw new Error('SecurityError');
      },
      removeItem() {
        throw new Error('SecurityError');
      },
    });
    try {
      expect(() => clearBrowserAccountData()).not.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
