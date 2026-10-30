import { JOURNEY_FROM_TFL_STOP_PARAM } from './tflStops';

/** How `/plan` (`JourneyCreationFlow`) starts a journey: plan a route,
 * pick a train the visitor already knows, or a Tube, bus or walk leg.
 * `/journeys/new`, which offered the same three, redirects to `/plan`
 * with its query (DS-194), so its `?mode=` links keep working. */
export type JourneyEntryMode = 'known' | 'plan' | 'untracked';

/** `TrackTrainForm`'s own two ways to find a train, shown under "I know my
 * route". */
export type KnownRouteMode = 'pick' | 'window';

/** What a page load of `/plan` opens on. */
export interface JourneyEntryStart {
  entryMode: JourneyEntryMode;
  /** Only set when the URL named one (`?mode=pick` or `?mode=window`), so
   * `TrackTrainForm` keeps its own default otherwise. */
  knownRouteMode?: KnownRouteMode;
}

/** The planner first: most visitors starting a journey know where they're
 * going, not which train to catch (product decision, 23 Oct 2026). */
export const DEFAULT_JOURNEY_ENTRY_MODE: JourneyEntryMode = 'plan';

/** Reads `?mode=` on `/plan`. `known`, `plan` and `untracked` pick the
 * option of that name; `pick` and `window` are `/track`'s values, and open
 * "I know my route" on that half of its form, so a link written for one
 * page works on the other. Anything else, a repeated value's later copies
 * included, means absent: the default. */
export function parseJourneyEntryMode(mode: string | string[] | undefined): JourneyEntryStart {
  const value = Array.isArray(mode) ? mode[0] : mode;
  switch (value) {
    case 'known':
    case 'plan':
    case 'untracked':
      return { entryMode: value };
    case 'pick':
    case 'window':
      return { entryMode: 'known', knownRouteMode: value };
    default:
      return { entryMode: DEFAULT_JOURNEY_ENTRY_MODE };
  }
}

/** The query string (with its `?`, or empty) after switching to `mode` on
 * `/plan`, from the current one: `?mode=` names the option, except for the
 * default, which needs none. Everything else is kept (the planner's own
 * search, so switching back restores it), except a TfL stop, which only
 * means something to "Tube, bus or walk" and would otherwise reopen it. */
export function entryModeSearch(current: string, mode: JourneyEntryMode): string {
  const params = new URLSearchParams(current);
  if (mode === DEFAULT_JOURNEY_ENTRY_MODE) params.delete('mode');
  else params.set('mode', mode);
  if (mode !== 'untracked') params.delete(JOURNEY_FROM_TFL_STOP_PARAM);
  const search = params.toString();
  return search ? `?${search}` : '';
}
