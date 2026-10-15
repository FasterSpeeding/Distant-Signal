import { beforeEach, describe, expect, it } from 'vitest';
import { BROWSER_ACCOUNT_KEYS } from './browserAccountData';
import { PRIDE_MODE_OPTIONS, PRIDE_MODE_SCRIPT, PRIDE_MODE_STORAGE_KEY, parseStoredMode } from './prideMode';

describe('parseStoredMode', () => {
  it('maps the old boolean value to rainbow and anything unknown to off', () => {
    expect(parseStoredMode('true')).toBe('rainbow');
    expect(parseStoredMode('false')).toBe('off');
    expect(parseStoredMode(null)).toBe('off');
    expect(parseStoredMode('bogus')).toBe('off');
    expect(parseStoredMode('trans')).toBe('trans');
  });
});

describe('PRIDE_MODE_SCRIPT', () => {
  // The script the root layout inlines into <body>, run here as the
  // browser runs it before the first paint.
  const run = (): void => {
    // eslint-disable-next-line @typescript-eslint/no-implied-eval -- runs the exact inline script string, as the browser does
    new Function(PRIDE_MODE_SCRIPT)();
  };

  beforeEach(() => {
    localStorage.clear();
    delete document.body.dataset.pride;
  });

  it.each(PRIDE_MODE_OPTIONS.filter((option) => option.value !== 'off').map((option) => option.value))(
    'applies a saved %s flag',
    (mode) => {
      localStorage.setItem(PRIDE_MODE_STORAGE_KEY, mode);
      run();
      expect(document.body.dataset.pride).toBe(mode);
    },
  );

  it('reads the old boolean value as the rainbow, the same as parseStoredMode', () => {
    localStorage.setItem(PRIDE_MODE_STORAGE_KEY, 'true');
    run();
    expect(document.body.dataset.pride).toBe('rainbow');
  });

  it.each([null, 'off', 'bogus', 'trans"><script>'])('sets nothing for %s', (raw) => {
    if (raw !== null) localStorage.setItem(PRIDE_MODE_STORAGE_KEY, raw);
    run();
    expect(document.body.dataset.pride).toBeUndefined();
  });

  it('never throws when storage is blocked', () => {
    const original = Object.getOwnPropertyDescriptor(window, 'localStorage');
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      get() {
        throw new Error('blocked');
      },
    });
    try {
      expect(run).not.toThrow();
      expect(document.body.dataset.pride).toBeUndefined();
    } finally {
      if (original) Object.defineProperty(window, 'localStorage', original);
    }
  });
});

describe('logging in or out', () => {
  // Display preferences are device-local and the account keeps no copy, so
  // logging in keeps them as they are, and logging out (which clears
  // BROWSER_ACCOUNT_KEYS) leaves them alone too.
  it('leaves the pride flag and the colour theme in this browser', () => {
    expect(BROWSER_ACCOUNT_KEYS).not.toContain(PRIDE_MODE_STORAGE_KEY);
    expect(BROWSER_ACCOUNT_KEYS).not.toContain('mantine-color-scheme-value');
  });
});
