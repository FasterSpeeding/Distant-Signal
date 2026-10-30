/** The opt-in usage events the web app may send (DS-190), mirroring the
 * api's allowlist in `crates/api/src/usage_events.rs`. The api refuses
 * anything not listed there, and its `the_frontend_mirrors_the_allowlist`
 * test checks that every value it accepts is named here in single quotes,
 * so keep the two in step.
 *
 * Events name page types (route templates such as `/lines/[id]`) and
 * enumerated feature steps, never a specific station, line, train or
 * journey. */

/** Every page of the app as Next's route template, plus `/offline` (the
 * service worker's offline page) and `/_not-found`. `usageEvents.test.ts`
 * checks this against `app/**\/page.tsx`. */
export const ROUTE_TEMPLATES = [
  '/',
  '/_not-found',
  '/accessibility',
  '/account',
  '/account/connected-apps',
  '/account/connected-apps/[client]',
  '/account/deleted',
  '/account/notifications',
  '/account/preferences',
  '/attribution',
  '/chat',
  '/chat/callback',
  '/connect-claude',
  '/connect-claude/authorize',
  '/contact',
  '/cookies',
  '/groups',
  '/groups/[id]',
  '/groups/join/[token]',
  '/groups/new',
  '/incidents',
  '/incidents/[id]',
  '/journeys',
  '/journeys/[id]',
  '/journeys/shared/[token]',
  '/journeys/templates',
  '/journeys/templates/[id]',
  '/lines',
  '/lines/[id]',
  '/lines/[id]/edit',
  '/lines/[id]/history',
  '/lines/[id]/timetable',
  '/lines/new',
  '/network/history',
  '/offline',
  '/operators',
  '/operators/[code]/history',
  '/plan',
  '/privacy',
  '/stations',
  '/stations/[crs]',
  '/status',
  '/terms',
  '/tfl/stops/[id]',
  '/track',
  '/track/mine/add-ticket',
  '/train/[uid]/[date]',
  '/train/by-id/[trackingId]',
  '/trains',
  '/transit',
  '/transit/[network]',
  '/transit/[network]/lines/[line]',
  '/transit/[network]/lines/[line]/timetable',
  '/transit/[network]/stops/[stop]',
] as const;

export type RouteTemplate = (typeof ROUTE_TEMPLATES)[number];

/** The templates that are not pages a pathname can match. */
const PSEUDO_TEMPLATES: ReadonlySet<string> = new Set(['/offline', '/_not-found']);

export const NOT_FOUND_TEMPLATE: RouteTemplate = '/_not-found';
export const OFFLINE_TEMPLATE: RouteTemplate = '/offline';

/** The browser-visible feature steps, by feature. */
export const FEATURE_STEPS = {
  track_train: ['submitted', 'succeeded', 'failed'],
  plan: ['searched', 'results', 'no_route', 'failed', 'saved_journey'],
  share_link: ['created', 'copied'],
  notifications: ['prompted', 'granted', 'denied', 'subscribed', 'failed'],
  group_join: ['joined', 'failed'],
  chat: ['opened', 'message_sent'],
  connected_app: ['enabled', 'revoked'],
  /** `/trains`: a search in one of its views (the step names the view),
   * then what came back. A UID opens its train's page instead. */
  trains: ['station_departures', 'station_arrivals', 'between', 'headcode', 'uid', 'results', 'no_results', 'failed'],
  /** The planner's "Compare options": a search with it on, an option other
   * than the fastest chosen, and that saved as a journey. */
  compare_options: ['searched', 'alternative_selected', 'alternative_saved'],
  /** Home's "Shared with you": shown, and a journey in it opened. */
  shared_journeys: ['shown', 'opened'],
  /** A journey leg's collapsed "Earlier stops"/"Later stops" opened. */
  leg_stops: ['earlier_expanded', 'later_expanded'],
  /** `/plan`'s way in to a new journey, each time one is shown. */
  journey_start: ['plan', 'known', 'untracked'],
} as const;

export const FEATURES = [
  'track_train',
  'plan',
  'share_link',
  'notifications',
  'group_join',
  'chat',
  'connected_app',
  'trains',
  'compare_options',
  'shared_journeys',
  'leg_stops',
  'journey_start',
] as const;

export type Feature = keyof typeof FEATURE_STEPS;
export type FeatureStep<F extends Feature> = (typeof FEATURE_STEPS)[F][number];

/** Empty or no-result states (`empty_state`'s `kind`). */
export const EMPTY_STATE_KINDS = [
  'trains_station_board_none',
  'trains_between_none',
  'trains_headcode_none',
  'station_search_no_results',
  'departures_none',
  'journeys_none',
  'groups_none',
  'line_trains_none',
] as const;
export type EmptyStateKind = (typeof EMPTY_STATE_KINDS)[number];

/** `api_error`'s status classes. */
export const API_ERROR_STATUSES = ['4xx', '5xx', 'network'] as const;
export type ApiErrorStatus = (typeof API_ERROR_STATUSES)[number];

/** `display_mode`'s values. */
export const DISPLAY_MODES = ['standalone', 'browser'] as const;
export type DisplayMode = (typeof DISPLAY_MODES)[number];

export const EVENT_NAMES = [
  'page_view',
  'display_mode',
  'feature',
  'empty_state',
  'api_error',
  'error_boundary',
  'offline_shown',
] as const;
export type EventName = (typeof EVENT_NAMES)[number];

/** One event as `POST /api/events` takes it. */
export interface UsageEventBody {
  name: EventName;
  route_template: RouteTemplate;
  props?: Record<string, string>;
}

/** A Next error digest as the api accepts it: 1 to 64 of `[A-Za-z0-9@_-]`. */
export function isDigest(value: string): boolean {
  return /^[A-Za-z0-9@_-]{1,64}$/.test(value);
}

interface CompiledTemplate {
  template: RouteTemplate;
  segments: readonly string[];
}

const COMPILED: readonly CompiledTemplate[] = ROUTE_TEMPLATES.filter((t) => !PSEUDO_TEMPLATES.has(t)).map(
  (template) => ({
    template,
    segments: template === '/' ? [] : template.slice(1).split('/'),
  }),
);

function isDynamic(segment: string): boolean {
  return segment.startsWith('[') && segment.endsWith(']');
}

/** Whether `a` is more specific than `b`: at the first segment where one
 * is static and the other dynamic, the static one wins (`/groups/new`
 * over `/groups/[id]`), as Next's own router decides. */
function moreSpecific(a: CompiledTemplate, b: CompiledTemplate): boolean {
  for (let i = 0; i < a.segments.length; i++) {
    const aDynamic = isDynamic(a.segments[i] ?? '');
    const bDynamic = isDynamic(b.segments[i] ?? '');
    if (aDynamic !== bDynamic) return !aDynamic;
  }
  return false;
}

/** The route template for a pathname (`/lines/northern` is `/lines/[id]`),
 * or `/_not-found` when no page matches. Never returns the pathname. */
export function routeTemplateFor(pathname: string): RouteTemplate {
  const path = pathname.split(/[?#]/)[0] ?? '';
  const segments = path.split('/').filter((segment) => segment.length > 0);
  let best: CompiledTemplate | null = null;
  for (const candidate of COMPILED) {
    if (candidate.segments.length !== segments.length) continue;
    const matches = candidate.segments.every((segment, i) => isDynamic(segment) || segment === segments[i]);
    if (!matches) continue;
    if (best === null || moreSpecific(candidate, best)) best = candidate;
  }
  return best?.template ?? NOT_FOUND_TEMPLATE;
}

/** The status class of a failed call: `status` undefined is a network
 * failure. `null` for a success. */
export function apiErrorStatusFor(status: number | undefined): ApiErrorStatus | null {
  if (status === undefined) return 'network';
  if (status >= 500) return '5xx';
  if (status >= 400) return '4xx';
  return null;
}
