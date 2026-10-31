'use client';

import { useEffect, useRef, useState } from 'react';
import Form from 'next/form';
import { useRouter } from 'next/navigation';
import { Alert, Combobox, InputBase, Loader, Stack, Text, VisuallyHidden, useCombobox } from '@mantine/core';
import { useId } from '@mantine/hooks';
import { SearchIcon } from '@/components/ListFilterBar';
import { NO_MATCH_OPTION_VALUE } from '@/lib/autocompleteNoMatch';
import {
  loadFinderStops,
  rankStops,
  STOP_FINDER_LIMIT,
  stopIndexHref,
  stopLineNames,
  type FinderStop,
} from '@/lib/transitStopFinder';
import { searchKey } from '@/lib/stationSearch';
import { transitStopHref } from '@/lib/transitStops';
import { trackFeature } from '@/lib/usageAnalytics';
import classes from './TransitStopFinder.module.css';

/** The class `Autocomplete`'s own options carry, so these rows look the same. */
const OPTION_CLASS = (Combobox.classes as Record<string, string | undefined>).optionsDropdownOption ?? '';

/** The last row's value: open the A to Z filtered by the text typed. */
const ALL_MATCHES_VALUE = '__all_matches__';

type Load = { status: 'idle' } | { status: 'loading' } | { status: 'ready'; stops: FinderStop[] } | { status: 'error' };

/** "Find a stop" on a transit network's page (`/transit/[network]`): a
 * type-ahead over the network's stations that opens the one picked. The
 * page renders none of them: the list (`GET /api/transit/{network}/stops`,
 * cached for an hour) loads on first focus or keystroke, so a visit that
 * never searches never fetches it.
 *
 * A combobox (`useCombobox`, as `CodeLookupField`): arrows move through
 * the matches, Enter opens the highlighted stop (or, with none
 * highlighted, the only match or the one named exactly that), Escape
 * closes the list.
 * Each row names the stop and its lines; past `STOP_FINDER_LIMIT` matches
 * the last row is "See all *n* matches", the A to Z filtered. The field is
 * a GET form to that A to Z (`?q=`), so Enter with nothing highlighted, or
 * a browser without JavaScript, still finds the stop. A polite live region
 * says how many match. */
export function TransitStopFinder({
  network,
  networkName,
  lineNames,
}: {
  network: string;
  /** "Manchester Metrolink", for the field's description. */
  networkName: string;
  /** Public line id (`metrolink:blue`) to name ("Blue"), for each row. */
  lineNames: Readonly<Record<string, string>>;
}) {
  const router = useRouter();
  const combobox = useCombobox({
    onDropdownClose: () => {
      combobox.resetSelectedOption();
      setActiveId(null);
    },
  });
  const id = useId();
  const [query, setQuery] = useState('');
  const [load, setLoad] = useState<Load>({ status: 'idle' });
  const inputRef = useRef<HTMLInputElement>(null);
  // The highlighted row's id, for `aria-activedescendant`. Mantine keeps its
  // own only from the arrow keys, so it outlives a row that new text
  // filtered away (an id that points at nothing); this one is cleared on
  // every edit and read back from the list after each arrow key.
  const [activeId, setActiveId] = useState<string | null>(null);

  function syncActive() {
    requestAnimationFrame(() => {
      setActiveId(inputRef.current?.form?.querySelector('[data-combobox-selected]')?.id ?? null);
    });
  }

  function ensureLoaded() {
    if (load.status === 'loading' || load.status === 'ready') return;
    setLoad({ status: 'loading' });
    loadFinderStops(network).then(
      (stops) => setLoad({ status: 'ready', stops }),
      () => setLoad({ status: 'error' }),
    );
  }

  // Text typed before the page hydrated is in the field but not in state:
  // pick it up, so its matches show.
  useEffect(() => {
    const input = inputRef.current;
    if (!input?.value) return;
    setQuery(input.value);
    ensureLoaded();
    if (document.activeElement === input) combobox.openDropdown();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, on mount
  }, []);

  const searching = query.trim() !== '';
  const matches = load.status === 'ready' ? rankStops(load.stops, query) : [];
  const shown = matches.slice(0, STOP_FINDER_LIMIT);
  const more = matches.length > shown.length;
  const noMatch = searching && load.status === 'ready' && matches.length === 0;

  function open(value: string) {
    combobox.closeDropdown();
    if (value === NO_MATCH_OPTION_VALUE) return;
    if (value === ALL_MATCHES_VALUE) {
      trackFeature('stop_finder', 'all_matches_opened');
      router.push(stopIndexHref(network, query));
      return;
    }
    const stop = matches.find((s) => s.id === value);
    if (!stop) return;
    trackFeature('stop_finder', 'stop_opened');
    setQuery(stop.name);
    router.push(transitStopHref(network, stop.id));
  }

  const status = !searching
    ? ''
    : load.status === 'loading'
      ? 'Loading stops'
      : load.status === 'ready'
        ? matches.length === 0
          ? 'No stops match.'
          : `${matches.length} ${matches.length === 1 ? 'stop matches' : 'stops match'}.`
        : '';

  const hasOptions = searching && (shown.length > 0 || noMatch);

  /** The stop Enter opens with no row highlighted: the only match, or one
   * named exactly what was typed. Otherwise Enter submits the form, to the
   * A to Z filtered. */
  function enterTarget(): FinderStop | undefined {
    if (matches.length === 1) return matches[0];
    const first = matches[0];
    return first && searchKey(first.name) === searchKey(query) ? first : undefined;
  }

  return (
    <Stack gap={6}>
      <Form action={stopIndexHref(network)} prefetch={false} role="search" aria-label="Find a stop">
        {/* Not portalled: the open list stays inside the search landmark. */}
        <Combobox store={combobox} onOptionSubmit={open} withinPortal={false}>
          <Combobox.Target
            withExpandedAttribute
            autoComplete="off"
            // Overrides Mantine's (see `activeId`), `null` as Mantine drops an
            // undefined prop; not in its props' type.
            {...({ 'aria-activedescendant': (combobox.dropdownOpened && activeId) || null } as object)}
          >
            <InputBase
              id={id}
              ref={inputRef}
              name="q"
              label="Find a stop"
              description={`Any of ${networkName}'s stops, by name.`}
              placeholder="Start typing a stop name"
              value={query}
              maw={480}
              enterKeyHint="search"
              // A spinner in place of the search glyph while the list loads.
              leftSection={
                load.status === 'loading' && searching ? <Loader size={14} aria-hidden="true" /> : <SearchIcon />
              }
              onFocus={() => {
                ensureLoaded();
                if (searching) combobox.openDropdown();
              }}
              onClick={() => combobox.openDropdown()}
              onBlur={() => combobox.closeDropdown()}
              onChange={(event) => {
                ensureLoaded();
                setQuery(event.currentTarget.value);
                combobox.resetSelectedOption();
                setActiveId(null);
                combobox.openDropdown();
              }}
              onKeyDown={(event) => {
                if (event.key === 'ArrowDown' || event.key === 'ArrowUp') syncActive();
                if (event.key !== 'Enter') return;
                // Enter on a highlighted row is Mantine's: open that stop,
                // not the form's A to Z.
                if (combobox.dropdownOpened && combobox.getSelectedOptionIndex() !== -1) {
                  event.preventDefault();
                  return;
                }
                const target = enterTarget();
                if (target) {
                  event.preventDefault();
                  open(target.id);
                }
              }}
              inputContainer={(children) => (
                <>
                  {children}
                  <VisuallyHidden role="status" aria-live="polite">
                    {status}
                  </VisuallyHidden>
                </>
              )}
            />
          </Combobox.Target>
          <Combobox.Dropdown hidden={!hasOptions}>
            <Combobox.Options labelledBy={`${id}-label`}>
              {shown.map((stop) => {
                const lines = stopLineNames(stop, lineNames);
                return (
                  <Combobox.Option key={stop.id} value={stop.id} className={OPTION_CLASS}>
                    {/* The name, its lines under it. */}
                    <Stack gap={0}>
                      <Text size="sm" fw={500}>
                        {stop.name}
                      </Text>
                      {lines && (
                        <Text size="xs" className={classes.lines}>
                          {lines}
                        </Text>
                      )}
                    </Stack>
                  </Combobox.Option>
                );
              })}
              {more && (
                <Combobox.Option value={ALL_MATCHES_VALUE} className={OPTION_CLASS}>
                  <Text size="sm" fw={500} td="underline">
                    See all {matches.length} matches
                  </Text>
                </Combobox.Option>
              )}
              {noMatch && (
                <Combobox.Option value={NO_MATCH_OPTION_VALUE} disabled className={OPTION_CLASS}>
                  No stops match. Try a shorter search.
                </Combobox.Option>
              )}
            </Combobox.Options>
          </Combobox.Dropdown>
        </Combobox>
      </Form>
      {load.status === 'error' && (
        <Alert color="red" title="Couldn't load the stops" role="alert" maw={480}>
          Press Enter to search the A to Z instead.
        </Alert>
      )}
    </Stack>
  );
}
