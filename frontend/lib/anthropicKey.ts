// A per-viewer, browser-local credential -- localStorage, same precedent
// as ThemeToggle.tsx/PrideToggle.tsx and BrowserMcpOAuthProvider (Task 7).
// NEVER sent to any Distant Signal server -- this module's only consumers
// are Task 10's direct-to-Anthropic client construction and
// AnthropicKeySettings' own UI below. See the client-side-tokens design
// doc's Decision 6 for why localStorage over sessionStorage/IndexedDB.
/** Exported for `lib/browserAccountData.ts`, which clears it on logout. */
export const ANTHROPIC_API_KEY_STORAGE_KEY = 'ds-anthropic-api-key';
const STORAGE_KEY = ANTHROPIC_API_KEY_STORAGE_KEY;

export function getAnthropicApiKey(): string | null {
  return localStorage.getItem(STORAGE_KEY);
}

export function setAnthropicApiKey(key: string): void {
  localStorage.setItem(STORAGE_KEY, key);
}

export function clearAnthropicApiKey(): void {
  localStorage.removeItem(STORAGE_KEY);
}
