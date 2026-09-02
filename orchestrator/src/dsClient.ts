/** The allowlist check -- this service's ONE call directly into DS's own
 * `crates/api` (embedded-chatbot-option-b plan, Task 3 Step 2). A small,
 * session-forwarding client, genuinely separate from distant-signal-mcp's
 * own `DsApiClient` (that one lives in a different repository, is
 * anonymous-only, and does something different -- see the plan's Global
 * Constraints). */

export type ChatbotAccess = 'allowed' | 'unauthenticated' | 'forbidden';

/** Forwards the caller's raw `Cookie` header to `GET /public/chatbot/access`
 * (Task 2) and translates its three possible outcomes. Fails closed: any
 * response other than 200/401 (including a network error, a 5xx, or a
 * response this client doesn't recognise) is treated as `forbidden` --
 * never spend an Anthropic call on an ambiguous allow. */
export async function checkChatbotAccess(
    dsApiBaseUrl: string,
    cookieHeader: string,
    fetchImpl: typeof fetch = fetch
): Promise<ChatbotAccess> {
    try {
        const res = await fetchImpl(`${dsApiBaseUrl}/public/chatbot/access`, {
            headers: { Cookie: cookieHeader }
        });
        if (res.status === 200) {
            return 'allowed';
        }
        if (res.status === 401) {
            return 'unauthenticated';
        }
        return 'forbidden';
    } catch {
        // A network failure reaching `api` is exactly as fail-closed as a
        // non-200/401 response -- see this function's own doc comment.
        return 'forbidden';
    }
}
