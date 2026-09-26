/** Reads the one-time plaintext `token` out of a successful
 * `POST .../invite-link` or `POST .../share-link` response.
 *
 * Share/invite tokens are stored hashed server-side (2026-09-26 review,
 * L14), so that POST response is the ONLY place the token ever appears --
 * a later page load reports an active link's expiry but never its token.
 * A malformed/unexpected body degrades to `null` (the component then shows
 * "a link is active but can't be shown") rather than throwing. */
export async function freshTokenFromResponse(response: Response): Promise<string | null> {
  try {
    const body: unknown = await response.json();
    if (body && typeof body === 'object' && 'token' in body && typeof body.token === 'string') {
      return body.token;
    }
  } catch {
    // Fall through.
  }
  return null;
}

/** Like {@link freshTokenFromResponse}, but also reads `expiresAt` -- for
 * the journey share-link POSTs (create/regenerate/extend), whose expiry the
 * UI shows immediately rather than waiting for a refresh. */
export async function readShareLinkBody(
  response: Response,
): Promise<{ token: string | null; expiresAt: string | null }> {
  try {
    const body: unknown = await response.json();
    if (body && typeof body === 'object') {
      const token = 'token' in body && typeof body.token === 'string' ? body.token : null;
      const expiresAt = 'expiresAt' in body && typeof body.expiresAt === 'string' ? body.expiresAt : null;
      return { token, expiresAt };
    }
  } catch {
    // Fall through.
  }
  return { token: null, expiresAt: null };
}
