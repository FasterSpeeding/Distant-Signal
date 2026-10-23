/** How `/journeys/new` (`JourneyCreationFlow`) starts a journey: plan a
 * route, pick a train the visitor already knows, or a Tube, bus or walk
 * leg. */
export type JourneyEntryMode = 'known' | 'plan' | 'untracked';

/** `TrackTrainForm`'s own two ways to find a train, shown under "I know my
 * route". */
export type KnownRouteMode = 'pick' | 'window';

/** What a page load of `/journeys/new` opens on. */
export interface JourneyEntryStart {
  entryMode: JourneyEntryMode;
  /** Only set when the URL named one (`?mode=pick` or `?mode=window`), so
   * `TrackTrainForm` keeps its own default otherwise. */
  knownRouteMode?: KnownRouteMode;
}

/** The planner first: most visitors starting a journey know where they're
 * going, not which train to catch (product decision, 23 Oct 2026). */
export const DEFAULT_JOURNEY_ENTRY_MODE: JourneyEntryMode = 'plan';

/** Reads `?mode=` on `/journeys/new`. `known`, `plan` and `untracked` pick
 * the option of that name; `pick` and `window` are `/track`'s values, and
 * open "I know my route" on that half of its form, so a link written for
 * one page works on the other. Anything else, a repeated value's later
 * copies included, means absent: the default. (`?mode=plan` itself never
 * reaches the page: `next.config.mjs` sends it to `/plan`.) */
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
