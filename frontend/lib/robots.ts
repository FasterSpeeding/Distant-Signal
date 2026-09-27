import type { MetadataRoute } from 'next';

/** Path prefixes no crawler should ever fetch. Real route paths from
 * `app/`, listed explicitly rather than inferred (robots.txt paths are
 * case-sensitive, so these must match the frontend's lowercase routes, not
 * the backend's `/Journeys/...`-style API paths, which `/api/` covers).
 *
 * robots.txt is advisory and public: it keeps well-behaved crawlers away
 * from these paths, but it is not access control. Every private or
 * token-bearing page below is already gated server-side. */
export const DISALLOWED_PATHS: readonly string[] = [
  // Backend proxy (`app/api/[...path]/route.ts`): JSON, auth
  // (`/api/auth/login`, `/api/auth/logout`, ...) and every mutation.
  '/api/',
  // `/chat` is login-gated, and `/chat/callback` is the MCP OAuth callback.
  '/chat',
  // MCP OAuth authorize/consent bridge (`app/connect-claude/authorize/route.ts`).
  // `/connect-claude` itself is a public instructions page and stays crawlable.
  '/connect-claude/authorize',
  // Every groups page is per-user. This also covers `/groups/join/<token>`
  // invite links, which carry a secret token and must never be crawled.
  '/groups',
  // `/journeys/<id>` and `/journeys/templates` are per-user, and
  // `/journeys/shared/<token>` share links carry a secret token.
  // `/journeys/new` is re-allowed below.
  '/journeys/',
  // "My Trains & Tickets" (per-user), and the old `/track/tickets` path
  // that `next.config.mjs` redirects to it.
  '/track/mine',
  '/track/tickets',
  // `trackingId` is one user's own subscription id, not a public train.
  // The public train page is `/train/<uid>/<date>`.
  '/train/by-id/',
  // Creating or editing a user's own custom line.
  '/lines/new',
  '/lines/*/edit',
  // Any URL with a query string: search results (`/trains?...`), filters
  // (`/incidents?...`), history ranges (`/lines/<id>/history?...`), and
  // `/track?ticketId=...` prefills. Each variant is an uncached,
  // backend-heavy render, and none of them is a canonical page. The bare
  // path of every public page stays crawlable. `*` and `$` are part of
  // RFC 9309 and Google and Bing honor them. Crawlers that don't support
  // wildcards ignore this line.
  '/*?',
];

/** Paths re-allowed inside a disallowed prefix above. Google and Bing
 * apply the most specific (longest) matching rule, so this beats
 * `Disallow: /journeys/`. `$` anchors it, so `/journeys/new?...` is still
 * caught by the query-string rule. */
export const ALLOWED_PATHS: readonly string[] = ['/', '/journeys/new$'];

/** Crawlers that collect content to train AI models. **Not blocked by
 * default.** To opt out, set `BLOCK_AI_TRAINING_CRAWLERS` to `true`: each
 * of these user agents then gets `Disallow: /`. `Google-Extended` and
 * `Applebot-Extended` are control tokens rather than separate crawlers.
 * Blocking them opts out of AI training without affecting normal Google or
 * Apple search indexing.
 *
 * Deliberately not listed: the AI assistants' user-initiated fetchers
 * (`ChatGPT-User`, `Claude-User`, `Perplexity-User`). They fetch a page
 * because a person asked about it, which is closer to a browser than a
 * training crawler. */
export const AI_TRAINING_CRAWLERS: readonly string[] = [
  'GPTBot',
  'ClaudeBot',
  'CCBot',
  'Google-Extended',
  'Applebot-Extended',
  'Bytespider',
  'meta-externalagent',
  'Amazonbot',
  'PerplexityBot',
];

/** Owner toggle. See `AI_TRAINING_CRAWLERS`. */
export const BLOCK_AI_TRAINING_CRAWLERS = false;

/** Path of the app's sitemap, relative to the site origin. `null` because
 * the app doesn't have one yet. If an `app/sitemap.ts` is added, set this
 * to `'/sitemap.xml'` and robots.txt will advertise it. */
export const SITEMAP_PATH: string | null = null;

export interface RobotsOptions {
  /** Configured public origin (`https://host`, no trailing slash), or
   * `undefined` when none is configured. With no origin, the `Host` and
   * `Sitemap` lines are omitted: both must be absolute, and guessing an
   * origin from the request is worse than leaving them out. */
  origin: string | undefined;
  sitemapPath?: string | null;
  blockAiTrainingCrawlers?: boolean;
}

/** Pure builder behind `app/robots.ts`, kept here so it can be unit-tested
 * without a Next request context. */
export function buildRobots({
  origin,
  sitemapPath = SITEMAP_PATH,
  blockAiTrainingCrawlers = BLOCK_AI_TRAINING_CRAWLERS,
}: RobotsOptions): MetadataRoute.Robots {
  const rules: MetadataRoute.Robots['rules'] = [
    // Crawl-delay is deliberately not set. Googlebot ignores it, and the
    // expensive URLs (query-string variants and `/api/`) are disallowed
    // outright instead.
    { userAgent: '*', allow: [...ALLOWED_PATHS], disallow: [...DISALLOWED_PATHS] },
  ];
  if (blockAiTrainingCrawlers) {
    rules.push({ userAgent: [...AI_TRAINING_CRAWLERS], disallow: '/' });
  }
  return {
    rules,
    ...(origin ? { host: origin } : {}),
    ...(origin && sitemapPath ? { sitemap: `${origin}${sitemapPath}` } : {}),
  };
}
