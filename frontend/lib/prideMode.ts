/** The pride flag preference: which flag, if any, `globals.css` paints
 * along the top of every page (`body[data-pride=...]`).
 *
 * Device-local and needs no account: saved in this browser under
 * {@link PRIDE_MODE_STORAGE_KEY}, like Mantine's colour scheme. Logging in
 * or out leaves it alone (it is not one of `lib/browserAccountData.ts`'s
 * keys), and the account stores no copy, so there is nothing to merge.
 *
 * A plain module rather than part of `components/PrideMode.tsx` so the
 * root layout, a Server Component, can read {@link PRIDE_MODE_SCRIPT}: a
 * value exported from a `'use client'` module reaches a Server Component
 * only as a client reference, not as the string. */

export const PRIDE_MODE_STORAGE_KEY = 'pride-mode';

export type PrideMode =
  'off' | 'rainbow' | 'trans' | 'nonbinary' | 'bisexual' | 'pansexual' | 'asexual' | 'sapphic' | 'lesbian';

/** The picker's options, in the order the old nav toggle cycled through
 * them: the umbrella rainbow and trans flags first, the gender-identity
 * flags together, then the attraction flags. */
export const PRIDE_MODE_OPTIONS: readonly { value: PrideMode; label: string }[] = [
  { value: 'off', label: 'Off' },
  { value: 'rainbow', label: 'Rainbow' },
  { value: 'trans', label: 'Trans' },
  { value: 'nonbinary', label: 'Non-binary' },
  { value: 'bisexual', label: 'Bisexual' },
  { value: 'pansexual', label: 'Pansexual' },
  { value: 'asexual', label: 'Asexual' },
  { value: 'sapphic', label: 'Sapphic' },
  { value: 'lesbian', label: 'Lesbian' },
];

const STORED_MODES: readonly string[] = PRIDE_MODE_OPTIONS.map((option) => option.value);

/** Reads the saved mode. `'true'` is the value from before the toggle had
 * more than one flag, and still means the rainbow. */
export function parseStoredMode(raw: string | null): PrideMode {
  if (raw !== null && STORED_MODES.includes(raw)) return raw as PrideMode;
  if (raw === 'true') return 'rainbow';
  return 'off';
}

/** Reads the saved mode from this browser, or `'off'` where storage is
 * blocked. Client only. */
export function readStoredPrideMode(): PrideMode {
  try {
    return parseStoredMode(localStorage.getItem(PRIDE_MODE_STORAGE_KEY));
  } catch {
    return 'off';
  }
}

/** Inline script the root layout runs as the first thing in `<body>`,
 * before the page paints: it sets `body[data-pride]` from the saved mode,
 * so a saved flag is there on the first frame instead of popping in once
 * React hydrates. The same idea as Mantine's `ColorSchemeScript` for the
 * colour theme, and the same parse rules as {@link parseStoredMode}.
 * `<body>` carries `suppressHydrationWarning` for the attribute it adds. */
export const PRIDE_MODE_SCRIPT = `try{var m=localStorage.getItem(${JSON.stringify(PRIDE_MODE_STORAGE_KEY)});if(m==="true")m="rainbow";if(${JSON.stringify(STORED_MODES)}.indexOf(m)>0)document.body.dataset.pride=m;}catch(e){}`;
