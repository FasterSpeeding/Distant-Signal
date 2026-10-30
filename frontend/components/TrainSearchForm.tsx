'use client';

import { useEffect, useId, useMemo, useState, type SubmitEvent } from 'react';
import {
  Button,
  Checkbox,
  Group,
  Loader,
  MultiSelect,
  SegmentedControl,
  Stack,
  Text,
  VisuallyHidden,
} from '@mantine/core';
import { DatePickerInput } from '@/components/LazyDateFields';
import { PICKER_DATE_FORMAT } from '@/lib/dateFormat';
import { nowInLondon } from '@/lib/londonWallClock';
import { TextLink } from './TextLink';
import { TimeFilterInput } from './TimeFilterInput';
import { TrainSearchResults } from './TrainSearchResults';
import { ListFilterBar, SearchIcon, type ListNoun } from './ListFilterBar';
import { noOptionsFound } from './noOptionsFound';
import type { TrainArrivalsPage, TrainSearchDates, TrainSearchPage } from '@/lib/types';
import { searchDateBounds, searchDateDescription } from '@/lib/searchDates';
import { StationField } from './StationField';
import { useLiveSearch } from '@/lib/useLiveSearch';
import { paramValue, useInitialSearchParams } from '@/lib/useInitialSearchParams';
import { replaceUrlParams, useUrlFilterSync } from '@/lib/useUrlFilterSync';
import {
  apiSearchQuery,
  hasRows,
  operatorChoices,
  nextDateLabel,
  parseBoardParam,
  parseNextDayParam,
  parseOperatorParam,
  STATION_CODE_PATTERN,
  TIME_PATTERN,
  TRAIN_URL_PARAMS,
  upperBoundNextDay,
  type StationBoard,
  type TrainSearchResults as TrainSearchResultsState,
} from '@/lib/trainSearch';
import { trackFeature } from '@/lib/usageAnalytics';
import { TrainsAdvancedOptions } from './TrainsAdvancedOptions';
import {
  NO_STOP_FILTERS,
  countStopFilters,
  parseStopFilterParams,
  setStopFilterParams,
  stopFilterEntries,
  stopFilterErrors,
  type TrainStopFilters,
} from '@/lib/trainStopFilters';
import { getStationNames } from '@/lib/suggestions';
import { groupLabels, useStationGroups } from '@/lib/stationGroups';
import { isGroupCode } from '@/lib/stationLabel';

const TRAIN_NOUN: ListNoun = { one: 'train', other: 'trains' };

/** The "Next day" box under a time filter: the time is on the morning after
 * the searched date, still on that date's trains (`*_day_offset=1`).
 * `implied` ticks and locks it, for an upper bound earlier than its lower
 * one, which can only be after midnight. Shown only once the time is
 * filled in. */
function NextDayCheckbox({
  time,
  label,
  checked,
  implied = false,
  impliedReason,
  onChange,
}: {
  time: string;
  label: string;
  checked: boolean;
  implied?: boolean;
  impliedReason?: string;
  onChange: (checked: boolean) => void;
}) {
  if (!TIME_PATTERN.test(time.trim())) return null;
  return (
    <Checkbox
      size="xs"
      label={label}
      description={implied ? impliedReason : undefined}
      checked={checked}
      disabled={implied}
      onChange={(event) => onChange(event.currentTarget.checked)}
    />
  );
}

const CRS_PATTERN = STATION_CODE_PATTERN;

/** The `/trains` page's timetable search, for two of its views
 * (`TrainsLookup`): a station's departures or arrivals (`view="station"`,
 * with a Departures/Arrivals switch), or the trains between two stations
 * (`view="between"`). The third view, one train by headcode or UID, is
 * `TrainServiceLookup`.
 *
 * Two shapes (docs/style-guide.md, "List search and filters"): the full
 * form with a Search button until the first search, then the compact
 * sticky bar, with the station (or the two stations) in the search slot
 * and every other field behind "Filters". From then on every change
 * searches live (`useLiveSearch`), and the URL follows through
 * `history.replaceState`, in the timetable's own words (`TRAIN_URL_PARAMS`:
 * `station` or `from`/`to`, `board`, `date`, `time`/`until`...).
 *
 * The departures board and the between-stations view are the calling-point
 * search (`GET /public/trains/search`): `station` is any station a train
 * leaves from, and between two stations `stops_at` is the second, which
 * the train must reach later in its journey (see
 * docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md).
 * The arrivals board is `GET /public/trains/arrivals`: every train that
 * sets down at the station, terminating trains included.
 *
 * Filters: Date (today by default, bounded by `searchDates`, else a week
 * either side), an earliest/latest time on the board's own times (a
 * departure, or an arrival on the arrivals board; between two stations,
 * the departure from the first), each with a "Next day" box for a time
 * after the date's midnight; between two stations, an earliest/latest
 * arrival at the second; and Operator, "any of" the operators picked, a
 * code that is not on the page's list dropped (`parseOperatorParam`).
 * Under them, "Advanced options" (`TrainsAdvancedOptions`): Previously
 * calls at and Previously stops at, where the train has been before the
 * station (before the first of two), its origin counted or not, sent and
 * kept in the URL as `previously_calls_at` and `previously_stops_at`;
 * then the planner's Call at, Pass through, Avoid completely and Don't
 * stop at, applied to the part of the train ridden (after the station,
 * before it on the arrivals board, or between the two stations), as
 * `calls_at`, `via`, `avoid` and `avoid_stop`. The two "Previously" lists
 * replaced the station board's "Starts at" (`origin`): an old link's
 * `origin=X` opens as Previously calls at X (`parseStopFilterParams`),
 * and the address bar drops `origin` once the search runs.
 *
 * The rows are the timetable, each a link to the train's page. Only in the
 * ticket flow (`attachTicketId`, from the Journeys page) does a row carry
 * a "Track this train" action and the form offer the manual tracking
 * fallback: anywhere else, tracking is a secondary action on the train's
 * own page.
 *
 * Fetches through the same-origin `/api/*` proxy, like every other Client
 * Component in this app (`API_BASE_URL` is server-only). */
export function TrainSearchForm({
  view = 'station',
  initialBoard: initialBoardProp = '',
  initialStation: initialStationProp = '',
  initialStopsAt: initialStopsAtProp = '',
  initialDate: initialDateProp = '',
  initialFrom: initialFromProp = '',
  initialTo: initialToProp = '',
  initialArrivalFrom: initialArrivalFromProp = '',
  initialArrivalTo: initialArrivalToProp = '',
  initialFromDayOffset: initialFromDayOffsetProp = '',
  initialToDayOffset: initialToDayOffsetProp = '',
  initialArrivalFromDayOffset: initialArrivalFromDayOffsetProp = '',
  initialArrivalToDayOffset: initialArrivalToDayOffsetProp = '',
  initialOperator: initialOperatorProp = '',
  initialStopFilters: initialStopFiltersProp = NO_STOP_FILTERS,
  seedFromUrl = true,
  attachTicketId,
  operatorNames,
  unnamedOperatorCodes,
  searchDates,
}: {
  /** A station's board, or the trains between two stations. */
  view?: 'station' | 'between' | undefined;
  /** `?board=`: `arrivals`, else departures. Station view only. */
  initialBoard?: string | undefined;
  /** The station (`?station=`), or between two stations the first
   * (`?from=`). */
  initialStation?: string | undefined;
  /** Between two stations, the second (`?to=`). */
  initialStopsAt?: string | undefined;
  initialDate?: string | undefined;
  /** `?time=` and `?until=`: the earliest and latest time on the board. */
  initialFrom?: string | undefined;
  initialTo?: string | undefined;
  initialArrivalFrom?: string | undefined;
  initialArrivalTo?: string | undefined;
  /** `?time_day_offset=`, `?until_day_offset=` and the two arrival ones:
   * `1` ticks that time's "Next day" box. */
  initialFromDayOffset?: string | undefined;
  initialToDayOffset?: string | undefined;
  initialArrivalFromDayOffset?: string | undefined;
  initialArrivalToDayOffset?: string | undefined;
  /** `?operator=`, comma-separated ATOC codes. */
  initialOperator?: string | undefined;
  /** `?calls_at=`, `?via=`, `?avoid=`, `?avoid_stop=`,
   * `?previously_calls_at=` and `?previously_stops_at=` (and in the
   * station view a legacy `?origin=`; `parseStopFilterParams`). */
  initialStopFilters?: TrainStopFilters | undefined;
  /** `false` when the view was switched to on the page, so the address
   * bar (just cleared) seeds nothing. */
  seedFromUrl?: boolean | undefined;
  attachTicketId?: number | undefined;
  operatorNames?: Readonly<Record<string, string>> | undefined;
  unnamedOperatorCodes?: readonly string[] | undefined;
  searchDates?: TrainSearchDates | null | undefined;
}) {
  const between = view === 'between';
  // Seeded from the address bar, not the server props: see
  // `useInitialSearchParams` (after Back, the props are stale). Station
  // codes are uppercased the way the page does.
  const initialSearchParams = useInitialSearchParams();
  const urlParams = seedFromUrl ? initialSearchParams : null;
  const seed = (key: string, prop: string, code: boolean) =>
    urlParams ? ((code ? paramValue(urlParams, key)?.toUpperCase() : paramValue(urlParams, key)) ?? '') : prop;
  const initialStation = seed(between ? 'from' : 'station', initialStationProp, true);
  const initialStopsAt = between ? seed('to', initialStopsAtProp, true) : '';
  const initialDate = seed('date', initialDateProp, false);
  const initialFrom = seed('time', initialFromProp, false);
  const initialTo = seed('until', initialToProp, false);
  const initialArrivalFrom = seed('arrival_from', initialArrivalFromProp, false);
  const initialArrivalTo = seed('arrival_to', initialArrivalToProp, false);
  const initialFromDayOffset = seed('time_day_offset', initialFromDayOffsetProp, false);
  const initialToDayOffset = seed('until_day_offset', initialToDayOffsetProp, false);
  const initialArrivalFromDayOffset = seed('arrival_from_day_offset', initialArrivalFromDayOffsetProp, false);
  const initialArrivalToDayOffset = seed('arrival_to_day_offset', initialArrivalToDayOffsetProp, false);
  const initialOperator = seed('operator', initialOperatorProp, false);
  const operatorLookup = useMemo(
    () => (operatorNames ? new Map(Object.entries(operatorNames)) : undefined),
    [operatorNames],
  );
  /** The Operator field's options, by name: "SW · South Western Railway",
   * as on `/incidents`, then any code the timetable carries with no name,
   * by code. */
  const operatorOptions = useMemo(
    () => operatorChoices(operatorNames ?? {}, unnamedOperatorCodes ?? []),
    [operatorNames, unnamedOperatorCodes],
  );
  const [board, setBoard] = useState<StationBoard>(() =>
    between ? 'departures' : parseBoardParam(seed('board', initialBoardProp, false)),
  );
  const arrivals = board === 'arrivals';
  const [operators, setOperators] = useState<string[]>(() =>
    parseOperatorParam(
      initialOperator,
      new Set([...Object.keys(operatorNames ?? {}), ...(unnamedOperatorCodes ?? [])]),
    ),
  );
  const [fromNextDay, setFromNextDay] = useState(() => parseNextDayParam(initialFromDayOffset));
  const [toNextDay, setToNextDay] = useState(() => parseNextDayParam(initialToDayOffset));
  const [arrivalFromNextDay, setArrivalFromNextDay] = useState(() => parseNextDayParam(initialArrivalFromDayOffset));
  const [arrivalToNextDay, setArrivalToNextDay] = useState(() => parseNextDayParam(initialArrivalToDayOffset));
  const [stationCrs, setStationCrs] = useState(initialStation);
  const [stopsAt, setStopsAt] = useState(initialStopsAt);
  const [dateValue, setDateValue] = useState<string | null>(initialDate || null);
  const [fromTime, setFromTime] = useState(initialFrom);
  const [toTime, setToTime] = useState(initialTo);
  const [arrivalFrom, setArrivalFrom] = useState(initialArrivalFrom);
  const [arrivalTo, setArrivalTo] = useState(initialArrivalTo);
  const [stopFilters, setStopFilters] = useState<TrainStopFilters>(() =>
    urlParams ? parseStopFilterParams(urlParams, { legacyOrigin: !between }) : initialStopFiltersProp,
  );
  const [advancedOpened, setAdvancedOpened] = useState(false);
  // Code -> name for the advanced options' rows: filled as stations are
  // picked, and once on mount for codes restored from the URL.
  const [stopNames, setStopNames] = useState<Map<string, string>>(new Map());
  const addStopNames = (found: Map<string, string>) => setStopNames((current) => new Map([...current, ...found]));
  useEffect(() => {
    const restored = [
      ...stopFilters.previouslyCallsAt,
      ...stopFilters.previouslyStopsAt,
      ...stopFilters.callsAt,
      ...stopFilters.via,
      ...stopFilters.avoid,
      ...stopFilters.avoidStop,
    ].filter((code) => !isGroupCode(code));
    if (restored.length === 0) return;
    let cancelled = false;
    void getStationNames(restored).then((found) => {
      if (!cancelled) addStopNames(found);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the restored filters are read on mount only
  }, []);
  const stopGroups = useStationGroups(
    [
      ...stopFilters.previouslyCallsAt,
      ...stopFilters.previouslyStopsAt,
      ...stopFilters.callsAt,
      ...stopFilters.via,
    ].some(isGroupCode),
  );
  const stopLabels = useMemo(() => new Map([...groupLabels(stopGroups), ...stopNames]), [stopGroups, stopNames]);
  /** Which of the four time filters are currently HALF-entered -- an hour
   * segment filled in with the minutes left blank, or vice versa. A native
   * `<input type="time">` reports a half-entered time as `''`, exactly like
   * an untouched one (see `TimeFilterInput`), so without this a field
   * visibly reading "09:--" would be silently dropped from the search.
   * `canSearch` below refuses to search on it. */
  const [incompleteTimes, setIncompleteTimes] = useState({
    from: false,
    to: false,
    arrivalFrom: false,
    arrivalTo: false,
  });
  const [results, setResults] = useState<TrainSearchResultsState>(null);
  // The RAW `dateValue` submitted with the last search that came back
  // 404/unpublished, captured with the request (the `'unpublished'`
  // variant carries no payload of its own).
  const [unpublishedDate, setUnpublishedDate] = useState<string | null>(null);
  // Whether the search runs live. Off until the first Search press (or a
  // URL that already names the station or stations): before that the full
  // form is shown, and a half-filled form must not start searching under
  // the visitor. From then on the form is the compact bar and every
  // change searches.
  const [live, setLive] = useState(
    () => CRS_PATTERN.test(initialStation.trim()) && (!between || CRS_PATTERN.test(initialStopsAt.trim())),
  );
  // Separate from the search's own pending flag on purpose: a "Load more"
  // in flight must not dim the rows already on screen.
  const [loadingMore, setLoadingMore] = useState(false);

  const stationErrorId = useId();
  const boardLabelId = useId();

  const stationValid = CRS_PATTERN.test(stationCrs.trim());
  // Between two stations the second is required; on a station's board
  // there is none.
  const stopsAtValid = between ? CRS_PATTERN.test(stopsAt.trim()) : stopsAt.trim() === '';
  const fromValid = fromTime.trim() === '' || TIME_PATTERN.test(fromTime.trim());
  const toValid = toTime.trim() === '' || TIME_PATTERN.test(toTime.trim());
  const arrivalFromValid = arrivalFrom.trim() === '' || TIME_PATTERN.test(arrivalFrom.trim());
  const arrivalToValid = arrivalTo.trim() === '' || TIME_PATTERN.test(arrivalTo.trim());
  /** The arrival pair only counts between two stations, mirroring exactly
   * what `searchParams()` itself sends. */
  const timesComplete =
    !incompleteTimes.from &&
    !incompleteTimes.to &&
    (!between || (!incompleteTimes.arrivalFrom && !incompleteTimes.arrivalTo));
  const stopErrors = stopFilterErrors(stopFilters, {
    stations: between
      ? [
          [stationCrs, 'where you get on'],
          [stopsAt, 'where you get off'],
        ]
      : [[stationCrs, 'the station on this board']],
    names: stopLabels,
    arrivals,
  });
  const filtersValid =
    Object.keys(stopErrors).length === 0 &&
    stationValid &&
    stopsAtValid &&
    fromValid &&
    toValid &&
    arrivalFromValid &&
    arrivalToValid &&
    timesComplete;

  // Anchored to Europe/London (FE-4), like the server's own window, so a
  // visitor whose device isn't on UK time gets the edges the server
  // enforces rather than a 400 at either end.
  const today = nowInLondon().format('YYYY-MM-DD');
  const dateBounds = searchDateBounds(searchDates, today);

  // What the time fields' descriptions name as the station they are
  // scoped to: the entered code when it is valid ("RDG"), else the field.
  const stationDisplay = stationValid ? stationCrs.trim().toUpperCase() : between ? 'From' : 'Station';
  const stopsAtDisplay = stopsAtValid && stopsAt.trim() ? stopsAt.trim().toUpperCase() : 'To';

  // The "Next day" boxes. A latest time is after midnight when its box is
  // ticked or when it is earlier than its earliest time; Latest arrival's
  // earliest time is Earliest arrival, else Earliest departure, as the API
  // reads them.
  const nextDayLabel = `Next day, ${nextDateLabel(dateValue || today)}`;
  const fromOnNextDay = fromNextDay && fromTime.trim() !== '';
  const arrivalFromOnNextDay = arrivalFromNextDay && arrivalFrom.trim() !== '';
  const toDay = upperBoundNextDay(toTime, toNextDay, fromTime, fromOnNextDay);
  const arrivalToDay = arrivalFrom.trim()
    ? upperBoundNextDay(arrivalTo, arrivalToNextDay, arrivalFrom, arrivalFromOnNextDay)
    : upperBoundNextDay(arrivalTo, arrivalToNextDay, fromTime, fromOnNextDay);

  /** The current filters as the API's query parameters (the search's and
   * the arrivals board's share their names). Shared by the search and by
   * "Load more" so that page 2 is unambiguously a continuation of page 1's
   * query. */
  function searchParams() {
    const params = new URLSearchParams({ station: stationCrs.trim().toUpperCase() });
    if (dateValue) params.set('date', dateValue);
    if (between) {
      params.set('stops_at', stopsAt.trim().toUpperCase());
      if (arrivalFrom.trim()) {
        params.set('arrival_from', arrivalFrom.trim());
        if (arrivalFromNextDay) params.set('arrival_from_day_offset', '1');
      }
      if (arrivalTo.trim()) {
        params.set('arrival_to', arrivalTo.trim());
        if (arrivalToNextDay) params.set('arrival_to_day_offset', '1');
      }
    }
    if (fromTime.trim()) {
      params.set('from', fromTime.trim());
      if (fromNextDay) params.set('from_day_offset', '1');
    }
    if (toTime.trim()) {
      params.set('to', toTime.trim());
      // Only a box the visitor ticked: one ticked by necessity (a time
      // earlier than the earliest one) the API infers the same way.
      if (toNextDay) params.set('to_day_offset', '1');
    }
    if (operators.length > 0) params.set('operator', operators.join(','));
    for (const [param, value] of stopFilterEntries(stopFilters)) params.set(param, value);
    return params;
  }

  /** The API path for the board on screen. */
  const endpoint = arrivals ? '/api/trains/arrivals' : '/api/trains/search';
  /** One page's request URL for `query` (the filters, as `searchParams()`
   * builds them). The calling-point search with no date picked also asks
   * for yesterday's trains still running (`apiSearchQuery`); the arrivals
   * board is the date's own trains only. */
  const requestUrl = (query: string) => `${endpoint}?${arrivals ? query : apiSearchQuery(query)}`;

  const query = filtersValid ? requestUrl(searchParams().toString()) : null;

  /** The current filters in the address bar's words (`TRAIN_URL_PARAMS`). */
  function urlState(): Record<string, string> {
    const state: Record<string, string> = {};
    const station = stationCrs.trim().toUpperCase();
    if (between) {
      state.from = station;
      state.to = stopsAt.trim().toUpperCase();
    } else {
      state.station = station;
      if (arrivals) state.board = 'arrivals';
    }
    if (dateValue) state.date = dateValue;
    if (fromTime.trim()) {
      state.time = fromTime.trim();
      if (fromNextDay) state.time_day_offset = '1';
    }
    if (toTime.trim()) {
      state.until = toTime.trim();
      if (toNextDay) state.until_day_offset = '1';
    }
    if (between) {
      if (arrivalFrom.trim()) {
        state.arrival_from = arrivalFrom.trim();
        if (arrivalFromNextDay) state.arrival_from_day_offset = '1';
      }
      if (arrivalTo.trim()) {
        state.arrival_to = arrivalTo.trim();
        if (arrivalToNextDay) state.arrival_to_day_offset = '1';
      }
    }
    if (operators.length > 0) state.operator = operators.join(',');
    for (const [param, value] of stopFilterEntries(stopFilters)) state[param] = value;
    return state;
  }

  /** Page 1 for the request URL `requested`, run by `useLiveSearch`, which
   * aborts it if the filters change first. */
  const { pending } = useLiveSearch(live ? query : null, async (requested, signal) => {
    // The date that was actually searched, read from the request rather
    // than the live picker, which can move while the request is out.
    const submittedDateValue = new URL(requested, 'http://localhost').searchParams.get('date');
    // A function, not `signal.aborted` inline: it changes across the awaits.
    const superseded = () => signal.aborted;
    // Usage counts (DS-190): which view searched, then what came back.
    trackFeature('trains', between ? 'between' : arrivals ? 'station_arrivals' : 'station_departures');
    // Which advanced options it used: their kinds, never the stations.
    for (const param of setStopFilterParams(
      parseStopFilterParams(new URL(requested, 'http://localhost').searchParams),
    )) {
      trackFeature('trains_filters', param);
    }
    try {
      const response = await fetch(requested, { signal });
      if (superseded()) return;
      if (response.status === 404) {
        trackFeature('trains', 'no_results');
        setResults('unpublished');
        setUnpublishedDate(submittedDateValue);
        return;
      }
      if (!response.ok) {
        trackFeature('trains', 'failed');
        setResults('error');
        return;
      }
      const body = (await response.json()) as TrainSearchPage | TrainArrivalsPage;
      if (superseded()) return;
      trackFeature('trains', body.results.length > 0 ? 'results' : 'no_results');
      setResults({
        rows: body.results,
        nextCursor: body.nextCursor,
        date: submittedDateValue ?? '',
        loadMoreFailed: false,
        provisional: body.provisional === true,
      });
    } catch {
      if (superseded()) return;
      trackFeature('trains', 'failed');
      setResults('error');
    }
  });

  // Keeps this /trains history entry's URL on the timetable on screen, so
  // following a result to `/train/[uid]/[date]` and pressing Back restores
  // both the filters and the results (see
  // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
  // §3.1). `history.replaceState` through `useUrlFilterSync`, not
  // `router.replace`, which re-ran this page's server render on every
  // search. Only once the search is live: a form nobody has searched with
  // yet leaves the URL alone. `ticketId` and any other param are kept.
  useUrlFilterSync(live && query !== null ? urlState() : null, (current, state) =>
    state === null ? current : replaceUrlParams(current, TRAIN_URL_PARAMS, state),
  );

  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    // A problem in the advanced options shows on its field: open them.
    if (Object.keys(stopErrors).length > 0) setAdvancedOpened(true);
    if (filtersValid) setLive(true);
  }

  async function handleLoadMore() {
    if (!hasRows(results)) return;
    if (results.nextCursor === null || loadingMore) return;

    // Page 1's actual raw date (`''` meaning "none picked"), not whatever
    // the picker shows right now, so this request is unambiguously "page 2
    // of the same search".
    const pageOneDate = results.date;
    // The exact result-set object this page is a continuation of: a fresh
    // search can resolve first, and a stale page must not append its rows
    // to somebody else's search.
    const pagedFrom = results;
    setLoadingMore(true);
    try {
      const params = searchParams();
      if (pageOneDate) {
        params.set('date', pageOneDate);
      } else {
        params.delete('date');
      }
      params.set('after', results.nextCursor);
      const response = await fetch(requestUrl(params.toString()));
      if (!response.ok) {
        // The cursor is kept, not nulled: this page just failed to load, and
        // the reader gets a named error plus a working retry rather than a
        // list that quietly stops one page short of the end.
        setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
        return;
      }
      const body = (await response.json()) as TrainSearchPage | TrainArrivalsPage;
      setResults((current) =>
        current === pagedFrom
          ? {
              rows: [...current.rows, ...body.results],
              nextCursor: body.nextCursor,
              date: current.date,
              loadMoreFailed: false,
              provisional: current.provisional,
            }
          : current,
      );
    } catch {
      setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
    } finally {
      setLoadingMore(false);
    }
  }

  const codeError = 'Must be a 3-letter station code';
  const stationError = stationCrs.length > 0 && !stationValid ? codeError : null;
  const stopsAtError = between && stopsAt.length > 0 && !CRS_PATTERN.test(stopsAt.trim()) ? codeError : null;

  // Inside the filters popover the option lists and the date picker render
  // in place (`withinPortal: false`): a portalled dropdown counts as outside
  // the popover, so picking an option closed the whole panel, and it would
  // sit outside the phone sheet's focus trap.
  const inPanel = live;
  const comboboxProps = inPanel ? { comboboxProps: { withinPortal: false } } : {};

  const timeNoun = arrivals ? 'arrival' : 'departure';
  const timeVerb = arrivals ? 'reaching' : between ? 'leaving' : 'at';

  /** Every field but the station (or stations): the rest of the full form,
   * then the panel. */
  const otherFields = (
    <>
      {/* `clearButtonProps`: Mantine's `clearable` clear button ships with
          no accessible name, so axe's `button-name` fires (critical) as
          soon as a date is picked. */}
      <DatePickerInput
        valueFormat={PICKER_DATE_FORMAT}
        label="Date (optional)"
        placeholder="Today"
        description={searchDateDescription(dateBounds, today)}
        value={dateValue}
        onChange={setDateValue}
        minDate={dateBounds.minDate}
        maxDate={dateBounds.maxDate}
        clearable
        clearButtonProps={{ 'aria-label': 'Clear the date' }}
        {...(inPanel && { popoverProps: { withinPortal: false } })}
      />
      <MultiSelect
        label="Operator (optional)"
        placeholder="Any operator"
        description="Only trains run by one of these operators."
        data={operatorOptions}
        value={operators}
        onChange={setOperators}
        searchable
        clearable
        // Mantine hides the whole dropdown when it has no options and no
        // `nothingFoundMessage`, leaving the input `aria-expanded="true"`
        // over nothing.
        nothingFoundMessage={noOptionsFound('No matching operators')}
        clearButtonProps={{ 'aria-label': 'Clear operator filter', className: 'iconHitArea24' }}
        {...comboboxProps}
      />
      {/* All four time filters are `TimeFilterInput` -- a native
       * `<input type="time">` with a clock button that opens the platform
       * time picker and a clear button. Its value is already exactly the
       * `"HH:MM"` the API takes, and an emptied field is `""`. */}
      <Group grow align="flex-start">
        <Stack gap={6}>
          <TimeFilterInput
            clockHint
            label={`Earliest ${timeNoun} (optional)`}
            name={`earliest ${timeNoun}`}
            description={`Only trains ${timeVerb} ${stationDisplay} at or after this time.`}
            value={fromTime}
            onChange={setFromTime}
            onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, from: incomplete }))}
            error={fromTime.length > 0 && !fromValid ? 'Must be a time like 09:00' : null}
          />
          <NextDayCheckbox time={fromTime} label={nextDayLabel} checked={fromNextDay} onChange={setFromNextDay} />
        </Stack>
        <Stack gap={6}>
          <TimeFilterInput
            label={`Latest ${timeNoun} (optional)`}
            name={`latest ${timeNoun}`}
            description={`Only trains ${timeVerb} ${stationDisplay} at or before this time.`}
            value={toTime}
            onChange={setToTime}
            onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, to: incomplete }))}
            error={toTime.length > 0 && !toValid ? 'Must be a time like 09:00' : null}
          />
          <NextDayCheckbox
            time={toTime}
            label={nextDayLabel}
            checked={toDay.nextDay}
            implied={toDay.implied}
            impliedReason={`Earlier than the earliest ${timeNoun}, so after midnight.`}
            onChange={setToNextDay}
          />
        </Stack>
      </Group>
      {between && (
        <Group grow align="flex-start">
          <Stack gap={6}>
            <TimeFilterInput
              label="Earliest arrival (optional)"
              name="earliest arrival"
              description={`Only trains reaching ${stopsAtDisplay} at or after this time.`}
              value={arrivalFrom}
              onChange={setArrivalFrom}
              onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, arrivalFrom: incomplete }))}
              error={arrivalFrom.length > 0 && !arrivalFromValid ? 'Must be a time like 09:00' : null}
            />
            <NextDayCheckbox
              time={arrivalFrom}
              label={nextDayLabel}
              checked={arrivalFromNextDay}
              onChange={setArrivalFromNextDay}
            />
          </Stack>
          <Stack gap={6}>
            <TimeFilterInput
              label="Latest arrival (optional)"
              name="latest arrival"
              description={`Only trains reaching ${stopsAtDisplay} at or before this time.`}
              value={arrivalTo}
              onChange={setArrivalTo}
              onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, arrivalTo: incomplete }))}
              error={arrivalTo.length > 0 && !arrivalToValid ? 'Must be a time like 09:00' : null}
            />
            <NextDayCheckbox
              time={arrivalTo}
              label={nextDayLabel}
              checked={arrivalToDay.nextDay}
              implied={arrivalToDay.implied}
              impliedReason="Earlier than the earliest time above, so after midnight."
              onChange={setArrivalToNextDay}
            />
          </Stack>
        </Group>
      )}
      <TrainsAdvancedOptions
        filters={stopFilters}
        onChange={setStopFilters}
        names={stopLabels}
        onNames={addStopNames}
        errors={stopErrors}
        opened={advancedOpened}
        onOpenedChange={setAdvancedOpened}
        comboboxProps={inPanel ? { withinPortal: false } : undefined}
        scope={
          between
            ? `These look at the train's route between ${stationDisplay} and ${stopsAtDisplay}.`
            : arrivals
              ? `These look at the train's route before it reaches ${stationDisplay}.`
              : `These look at the train's route after ${stationDisplay}.`
        }
        earlierScope={`To find a particular train, these look at where it has been before ${stationDisplay}.`}
        arrivals={arrivals}
      />
    </>
  );

  const activeCount =
    (dateValue ? 1 : 0) +
    (fromTime.trim() ? 1 : 0) +
    (toTime.trim() ? 1 : 0) +
    (between && arrivalFrom.trim() ? 1 : 0) +
    (between && arrivalTo.trim() ? 1 : 0) +
    (operators.length > 0 ? 1 : 0) +
    countStopFilters(stopFilters);

  function clearPanelFilters() {
    setDateValue(null);
    setOperators([]);
    setFromTime('');
    setToTime('');
    setArrivalFrom('');
    setArrivalTo('');
    setFromNextDay(false);
    setToNextDay(false);
    setArrivalFromNextDay(false);
    setArrivalToNextDay(false);
    setIncompleteTimes({ from: false, to: false, arrivalFrom: false, arrivalTo: false });
    setStopFilters(NO_STOP_FILTERS);
  }

  // Known only once the last page is in: the api has no total.
  const matchCount = !pending && hasRows(results) && results.nextCursor === null ? results.rows.length : null;

  const resultsSection = (
    <Stack gap="xs" mih={72}>
      <TrainSearchResults
        view={view}
        board={board}
        stationValid={stationValid && stopsAtValid}
        searching={pending && results === null}
        pending={pending}
        results={results}
        unpublishedDate={unpublishedDate}
        loadingMore={loadingMore}
        onLoadMore={() => void handleLoadMore()}
        operatorLookup={operatorLookup}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );

  // Only in the ticket flow: there, tracking the train is the point, and a
  // train missing from the published timetable still has a way in.
  // `inline`/`underline="always"`: this link sits mid-sentence in body
  // text, so colour alone can't be what marks it (WCAG 1.4.1).
  const manualFallback =
    attachTicketId !== undefined ? (
      <Text size="sm" c="dimmed">
        Can&apos;t find your train?{' '}
        <TextLink href={`/track?ticketId=${attachTicketId}`} inline underline="always">
          Track it manually
        </TextLink>{' '}
        by entering its origin station and departure time.
      </Text>
    ) : null;

  // The station view's Departures/Arrivals switch, above the form or bar.
  // Switching keeps every filter and, once live, searches at once.
  const boardSwitch = between ? null : (
    <Stack gap={4}>
      <VisuallyHidden id={boardLabelId}>Board</VisuallyHidden>
      <SegmentedControl
        aria-labelledby={boardLabelId}
        value={board}
        onChange={(value) => setBoard(value === 'arrivals' ? 'arrivals' : 'departures')}
        data={[
          { label: 'Departures', value: 'departures' },
          { label: 'Arrivals', value: 'arrivals' },
        ]}
        w="fit-content"
      />
    </Stack>
  );

  if (live) {
    return (
      <Stack gap="md">
        {boardSwitch}
        <ListFilterBar
          search={
            between ? (
              <Group gap="xs" wrap="nowrap" flex="1 1 auto" miw={0}>
                <StationField
                  aria-label="From"
                  placeholder="From"
                  value={stationCrs}
                  onChange={setStationCrs}
                  error={stationError !== null}
                  aria-describedby={stationError ? stationErrorId : undefined}
                  leftSection={
                    pending ? <Loader size={14} aria-hidden="true" data-testid="list-filter-pending" /> : <SearchIcon />
                  }
                  flex="1 1 0"
                  miw={0}
                />
                <StationField
                  aria-label="To"
                  placeholder="To"
                  value={stopsAt}
                  onChange={setStopsAt}
                  error={stopsAtError !== null}
                  aria-describedby={stopsAtError ? stationErrorId : undefined}
                  flex="1 1 0"
                  miw={0}
                />
              </Group>
            ) : (
              <StationField
                aria-label="Station"
                placeholder="Station, e.g. Reading or RDG"
                value={stationCrs}
                onChange={setStationCrs}
                // Only the red outline here: the bar is one fixed-height row,
                // so the message itself goes below it.
                error={stationError !== null}
                aria-describedby={stationError ? stationErrorId : undefined}
                leftSection={
                  pending ? <Loader size={14} aria-hidden="true" data-testid="list-filter-pending" /> : <SearchIcon />
                }
                flex="1 1 auto"
                miw={0}
              />
            )
          }
          noun={TRAIN_NOUN}
          matchCount={matchCount}
          pending={pending}
          activeCount={activeCount}
          onClearFilters={clearPanelFilters}
          panelWidth={380}
          scrollablePanel
        >
          {otherFields}
        </ListFilterBar>
        {(stationError ?? stopsAtError) && (
          <Text size="sm" c="red" id={stationErrorId}>
            {stationError ?? stopsAtError}
          </Text>
        )}
        {/* No count to announce (the api has no total), but a screen reader
            still hears that a search is running. */}
        <VisuallyHidden role="status">{pending ? 'Searching' : ''}</VisuallyHidden>
        {resultsSection}
        {manualFallback}
      </Stack>
    );
  }

  return (
    <Stack gap="md">
      {boardSwitch}
      <Stack gap="md" component="form" onSubmit={handleSubmit}>
        {between ? (
          <>
            <StationField
              label="From"
              placeholder="e.g. Reading or RDG"
              value={stationCrs}
              onChange={setStationCrs}
              error={stationError}
              required
            />
            <StationField
              label="To"
              placeholder="e.g. London Paddington or PAD"
              description="A station the train calls at after the first, its destination included."
              value={stopsAt}
              onChange={setStopsAt}
              error={stopsAtError}
              required
            />
          </>
        ) : (
          <StationField
            label="Station"
            placeholder="e.g. Reading or RDG"
            value={stationCrs}
            onChange={setStationCrs}
            error={stationError}
            required
          />
        )}
        {otherFields}
        <Group>
          <Button type="submit" disabled={!filtersValid}>
            Search
          </Button>
        </Group>
        {resultsSection}
        {manualFallback}
      </Stack>
    </Stack>
  );
}
