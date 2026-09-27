import { ANTHROPIC_API_KEY_STORAGE_KEY } from './anthropicKey';
import { MCP_OAUTH_STORAGE_KEYS, MCP_OAUTH_STORAGE_PREFIX } from './mcpOAuthProvider';

/** Browser-only data this app keeps for a signed-in visitor: the chat's
 * MCP OAuth client and tokens (`lib/mcpOAuthProvider.ts`, keys prefixed
 * `ds-mcp-oauth:`) and the visitor's own Anthropic key
 * (`lib/anthropicKey.ts`). Neither is ever sent to Distant Signal, so the
 * backend cannot delete them.
 *
 * DQ5 (FE-3/LEG-11/LEG-26): cleared on logout, on "log out other
 * sessions", and on account deletion. The MCP grant is bound to the
 * identity that approved it, so leaving it behind on a shared browser let
 * the next person to log in chat as the previous user, on the previous
 * user's Anthropic key. */
export const BROWSER_ACCOUNT_KEYS: readonly string[] = [...MCP_OAUTH_STORAGE_KEYS, ANTHROPIC_API_KEY_STORAGE_KEY];

/** Removes every {@link BROWSER_ACCOUNT_KEYS} entry, plus any other
 * `ds-mcp-oauth:*` key a newer provider version may have written. Never
 * throws: storage may be blocked (private mode, disabled site data), in
 * which case nothing was stored. */
export function clearBrowserAccountData(): void {
  try {
    const keys = new Set(BROWSER_ACCOUNT_KEYS);
    if (typeof localStorage.length === 'number' && typeof localStorage.key === 'function') {
      for (let i = 0; i < localStorage.length; i++) {
        const key = localStorage.key(i);
        if (key?.startsWith(MCP_OAUTH_STORAGE_PREFIX)) keys.add(key);
      }
    }
    for (const key of keys) {
      localStorage.removeItem(key);
    }
  } catch {
    // Storage blocked: nothing stored.
  }
}
