'use client';

import { useEffect, useMemo, useRef, useState, type SubmitEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, Text, TextInput } from '@mantine/core';
import { DatePickerInput } from '@/components/LazyDateFields';
import { LoadingText } from './LoadingText';
import { ProvisionalTimetableNote } from './ProvisionalTimetableNote';
import { ServiceRow, ServiceRowList } from './ServiceRow';
import { PICKER_DATE_FORMAT } from '@/lib/dateFormat';
import { nowInLondon } from '@/lib/londonWallClock';
import { searchDateBounds, searchDateDescription } from '@/lib/searchDates';
import { searchRowDetails, searchRowSummary, searchRowTime } from '@/lib/searchRow';
import { trainIdentifiers } from '@/lib/trainName';
import { HEADCODE_PATTERN, TRAIN_URL_PARAMS, UID_PATTERN } from '@/lib/trainSearch';
import type { TrainHeadcodePage, TrainSearchDates, TrainSearchResult } from '@/lib/types';
import { paramValue, useInitialSearchParams } from '@/lib/useInitialSearchParams';
import { replaceUrlParams } from '@/lib/useUrlFilterSync';

type Lookup =
  | { state: 'idle' }
  | { state: 'loading' }
  | { state: 'error' }
  | { state: 'unpublished'; date: string }
  | { state: 'done'; headcode: string; date: string; rows: TrainSearchResult[]; provisional: boolean };

/** What the one field holds: a headcode ("1S00"), a UID ("W12345"), or
 * neither yet. */
function codeKind(raw: string): 'headcode' | 'uid' | null {
  const code = raw.trim();
  if (HEADCODE_PATTERN.test(code)) return 'headcode';
  if (UID_PATTERN.test(code)) return 'uid';
  return null;
}

/** `GET /public/trains/by-headcode` for `headcode` on `date`, as the
 * results' state, or `null` once `signal` has aborted it. */
async function fetchHeadcode(headcode: string, date: string, signal: AbortSignal): Promise<Lookup | null> {
  try {
    const response = await fetch(`/api/trains/by-headcode?${new URLSearchParams({ headcode, date }).toString()}`, {
      signal,
    });
    if (response.status === 404) return signal.aborted ? null : { state: 'unpublished', date };
    if (!response.ok) return signal.aborted ? null : { state: 'error' };
    const body = (await response.json()) as TrainHeadcodePage;
    return signal.aborted
      ? null
      : { state: 'done', headcode, date, rows: body.results, provisional: body.provisional === true };
  } catch {
    return signal.aborted ? null : { state: 'error' };
  }
}

/** The `/trains` page's one-train view: a headcode or a UID, and a date.
 *
 * A UID names one train on a date, so it opens that train's page
 * (`/train/[uid]/[date]`: its timetable, calling points and live status).
 * A headcode can be shared by several trains a day across the network, so
 * it lists them (`GET /public/trains/by-headcode`), each at its first
 * station and linked to its page. The lookup is in the URL
 * (`?headcode=1S00&date=`), so a list is a shareable link; `/trains?uid=`
 * is answered by the page itself with a redirect to the train. */
export function TrainServiceLookup({
  initialCode = '',
  initialDate = '',
  seedFromUrl = true,
  operatorNames,
  searchDates,
}: {
  /** `?headcode=` (or `?uid=`). */
  initialCode?: string | undefined;
  initialDate?: string | undefined;
  /** `false` when the view was switched to on the page. */
  seedFromUrl?: boolean | undefined;
  /** ATOC code to name, to name each row's operator. */
  operatorNames?: Readonly<Record<string, string>> | undefined;
  searchDates?: TrainSearchDates | null | undefined;
}) {
  const operatorLookup = useMemo(
    () => (operatorNames ? new Map(Object.entries(operatorNames)) : undefined),
    [operatorNames],
  );
  const router = useRouter();
  const initialSearchParams = useInitialSearchParams();
  const urlParams = seedFromUrl ? initialSearchParams : null;
  const seededCode = urlParams
    ? (paramValue(urlParams, 'headcode') ?? paramValue(urlParams, 'uid') ?? '').toUpperCase()
    : initialCode.toUpperCase();
  const seededDate = urlParams ? (paramValue(urlParams, 'date') ?? '') : initialDate;
  // What the URL (or the props) named on mount, kept stable for the
  // arrival lookup below.
  const [seed] = useState(() => ({ code: seededCode, date: seededDate }));
  const [code, setCode] = useState(seed.code);
  const [dateValue, setDateValue] = useState<string | null>(seed.date || null);
  const [lookup, setLookup] = useState<Lookup>(() =>
    codeKind(seed.code) === 'headcode' ? { state: 'loading' } : { state: 'idle' },
  );
  const [submitted, setSubmitted] = useState(false);
  const today = nowInLondon().format('YYYY-MM-DD');
  const dateBounds = useMemo(() => searchDateBounds(searchDates, today), [searchDates, today]);
  const inFlight = useRef<AbortController | null>(null);

  const kind = codeKind(code);
  const codeError = submitted && kind === null ? 'Enter a headcode like 1S00 or a UID like W12345' : null;

  /** Looks a headcode up, superseding any lookup still in flight. */
  function findHeadcode(headcode: string, date: string) {
    inFlight.current?.abort();
    const controller = new AbortController();
    inFlight.current = controller;
    setLookup({ state: 'loading' });
    void fetchHeadcode(headcode, date, controller.signal).then((result) => {
      if (result !== null) setLookup(result);
    });
  }

  function lookUp(rawCode: string, rawDate: string | null) {
    const value = rawCode.trim().toUpperCase();
    const date = rawDate ?? today;
    const found = codeKind(value);
    if (found === 'uid') {
      router.push(`/train/${encodeURIComponent(value)}/${date}`);
      return;
    }
    if (found !== 'headcode') return;
    // In the address bar for refresh, Back and sharing; replace, not push,
    // like every other list filter.
    try {
      window.history.replaceState(
        null,
        '',
        replaceUrlParams(window.location.href, TRAIN_URL_PARAMS, {
          headcode: value,
          ...(rawDate ? { date: rawDate } : {}),
        }),
      );
    } catch {
      // Safari throttles replaceState; the list still shows.
    }
    findHeadcode(value, date);
  }

  // A URL that already names a headcode looks it up on arrival (the state
  // starts as loading for it); later lookups are the form's.
  useEffect(() => {
    if (codeKind(seed.code) !== 'headcode') return;
    const controller = new AbortController();
    inFlight.current = controller;
    void fetchHeadcode(seed.code, seed.date || nowInLondon().format('YYYY-MM-DD'), controller.signal).then((result) => {
      if (result !== null) setLookup(result);
    });
    return () => controller.abort();
  }, [seed]);

  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    setSubmitted(true);
    lookUp(code, dateValue);
  }

  return (
    <Stack gap="md">
      <Stack gap="md" component="form" onSubmit={handleSubmit}>
        <TextInput
          label="Headcode or UID"
          placeholder="e.g. 1S00 or W12345"
          description="A headcode lists the trains running under it. A UID opens its train."
          value={code}
          onChange={(event) => setCode(event.currentTarget.value)}
          error={codeError}
          autoComplete="off"
          autoCapitalize="characters"
          spellCheck={false}
          maxLength={6}
          required
        />
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
        />
        <Group>
          <Button type="submit">{kind === 'uid' ? 'Open train' : 'Find'}</Button>
        </Group>
      </Stack>
      <HeadcodeResults lookup={lookup} operatorLookup={operatorLookup} />
    </Stack>
  );
}

/** The trains a headcode runs as on a date, each at its first station. */
function HeadcodeResults({
  lookup,
  operatorLookup,
}: {
  lookup: Lookup;
  operatorLookup: ReadonlyMap<string, string> | undefined;
}) {
  switch (lookup.state) {
    case 'idle':
      return (
        <Text size="sm" c="dimmed">
          Enter a headcode or UID to see that train&apos;s timetable.
        </Text>
      );
    case 'loading':
      return <LoadingText>Searching</LoadingText>;
    case 'error':
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t search for trains right now. Try again.
        </Alert>
      );
    case 'unpublished':
      return (
        <Text size="sm" c="dimmed" role="status">
          Scheduled timetable data for {lookup.date} isn&apos;t available yet.
        </Text>
      );
    case 'done':
      if (lookup.rows.length === 0) {
        return (
          <Text size="sm" c="dimmed" role="status">
            No train runs as {lookup.headcode} on that day.
          </Text>
        );
      }
      return (
        <Stack gap="xs">
          <ProvisionalTimetableNote provisional={lookup.provisional} />
          <Text size="sm" c="dimmed" role="status">
            {lookup.rows.length === 1
              ? `1 train runs as ${lookup.headcode}.`
              : `${lookup.rows.length} trains run as ${lookup.headcode}.`}{' '}
            Times are when each leaves its first station.
          </Text>
          <ServiceRowList aria-label={`Trains running as ${lookup.headcode}`}>
            {lookup.rows.map((row) => (
              <ServiceRow
                key={row.uid}
                train={searchRowSummary(row)}
                date={row.serviceDate ?? lookup.date}
                timeOverride={searchRowTime(row)}
                dayOffset={row.dayOffset}
                details={
                  [searchRowDetails(row, operatorLookup), trainIdentifiers({ headcode: null, uid: row.uid })]
                    .filter(Boolean)
                    .join(' · ') || undefined
                }
              />
            ))}
          </ServiceRowList>
        </Stack>
      );
  }
}
