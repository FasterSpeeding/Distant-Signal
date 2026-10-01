import { isIP } from 'node:net';

/** The header the api's per-IP rate limiter keys on
 * (`crates/api/src/rate_limit.rs`'s `REAL_IP_HEADER`). */
export const REAL_IP_HEADER = 'X-Real-IP';

/** The client IP to send the api as `X-Real-IP` (FE-8), or null, read from
 * an incoming browser request's headers.
 *
 * Only `CF-Connecting-IP` is trusted: in production the frontend is reached
 * only through the Cloudflare tunnel, which sets it. There is no other
 * trustworthy source -- Next 16 exposes no socket peer address to route
 * handlers or Server Components (`request.ip` is gone), and the
 * `X-Forwarded-For` it passes in is the client's own whenever the client
 * sent one. The client's own `X-Real-IP`/`X-Forwarded-For` are never
 * relayed. With no trustworthy value the header is omitted and the api falls
 * back to its own peer address. A value that isn't a literal IP is ignored.
 *
 * Used by both paths that reach the api on a browser's behalf: the `/api/*`
 * proxy (`app/api/[...path]/route.ts`) and every server-render fetch
 * (`lib/api.ts`'s `apiFetch`). */
export function clientIpFromHeaders(headers: Pick<Headers, 'get'>): string | null {
  const cf = headers.get('cf-connecting-ip')?.trim();
  if (cf && isIP(cf) !== 0) return cf;
  return null;
}
