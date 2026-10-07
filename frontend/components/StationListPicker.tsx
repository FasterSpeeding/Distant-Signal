'use client';

import { useEffect, useId, useRef, useState } from 'react';
import { ActionIcon, Autocomplete, Group, Stack, Text, VisuallyHidden } from '@mantine/core';
import { useSuggestions } from '@/lib/useSuggestions';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { codeStationLabel, isTiplocCode, normalizeLocationCode } from '@/lib/stationLabel';
import type { Suggestion } from '@/lib/types';

function Chevron({ up }: { up: boolean }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points={up ? '6 15 12 9 18 15' : '6 9 12 15 18 9'} />
    </svg>
  );
}

function XIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  );
}

const CRS = /^[A-Za-z]{3}$/;

/** A typed code the picker accepts without a suggestion being chosen: a
 * CRS, or (when stops are allowed) a `tiploc:` code. */
function typedCode(value: string, allowStops: boolean): string | null {
  const trimmed = value.trim();
  if (CRS.test(trimmed)) return trimmed.toUpperCase();
  if (allowStops && isTiplocCode(trimmed) && trimmed.length > 'tiploc:'.length) return normalizeLocationCode(trimmed);
  return null;
}

/** A list of stations built one at a time with the planner's own station
 * picker (the same `Autocomplete` + `useSuggestions` +
 * `suggestionAutocompleteProps` trio as From and To), for the planner's
 * "Advanced options": the vias and the three avoid lists.
 *
 * Picking a suggestion (or typing a code and pressing Enter) adds it and
 * clears the field for the next one. Each chosen station is a row with a
 * labelled remove button and, when `ordered`, "move up"/"move down"
 * buttons: reordering is plain buttons rather than drag-and-drop so it
 * works from the keyboard and with a screen reader, and focus follows the
 * moved row. A polite live region announces every add, remove and move.
 *
 * `error` goes on the input (Mantine wires it to `aria-invalid` and
 * `aria-describedby`), so it is read with the field. */
export function StationListPicker({
  label,
  description,
  values,
  onChange,
  names,
  onNames,
  search,
  max,
  ordered = false,
  allowStops,
  error,
  itemNoun,
}: {
  label: string;
  description?: React.ReactNode;
  values: string[];
  onChange: (values: string[]) => void;
  /** Code -> name, for the rows. */
  names: Map<string, string>;
  /** Reports the name of a station picked from the suggestions. */
  onNames: (names: Map<string, string>) => void;
  search: (q: string, signal: AbortSignal) => Promise<Suggestion[]>;
  max: number;
  /** Order matters (vias): show move up/down buttons and positions. */
  ordered?: boolean;
  /** Whether a bus stop's or ferry terminal's `tiploc:` code may be added. */
  allowStops: boolean;
  error?: string | null | undefined;
  /** What one row is, for the buttons' names, e.g. "via" or "avoided station". */
  itemNoun: string;
}) {
  const [query, setQuery] = useState('');
  const [announcement, setAnnouncement] = useState('');
  const [typedError, setTypedError] = useState<string | null>(null);
  const { suggestions, loading } = useSuggestions(query, search);
  const listId = useId();
  // Mantine's `Autocomplete` writes the picked option's label (here, the
  // code) into the field right AFTER `onOptionSubmit`; this swallows that
  // write so the field is left empty for the next station.
  const justPicked = useRef<string | null>(null);
  const buttons = useRef(new Map<string, HTMLButtonElement | null>());
  const [focusKey, setFocusKey] = useState<string | null>(null);

  useEffect(() => {
    if (focusKey === null) return;
    buttons.current.get(focusKey)?.focus();
    // eslint-disable-next-line react-hooks/set-state-in-effect -- one-shot: the focus request has been served
    setFocusKey(null);
  }, [focusKey]);

  const full = values.length >= max;
  const visible = suggestions.filter((s) => allowStops || !isTiplocCode(s.code));
  const label_ = (code: string) => codeStationLabel(code, names.get(code));

  function add(code: string) {
    setTypedError(null);
    if (full) return;
    // An avoid list is a set; vias may repeat, just not twice in a row.
    if (ordered ? values[values.length - 1] === code : values.includes(code)) {
      setAnnouncement(`${label_(code)} is already in ${label}.`);
      return;
    }
    const picked = suggestions.find((s) => s.code === code);
    if (picked) onNames(new Map([[code, picked.name]]));
    onChange([...values, code]);
    setAnnouncement(`Added ${picked ? codeStationLabel(code, picked.name) : code} to ${label}.`);
  }

  function remove(index: number) {
    const code = values[index];
    if (code === undefined) return;
    onChange(values.filter((_, i) => i !== index));
    setAnnouncement(`Removed ${label_(code)} from ${label}.`);
  }

  function move(index: number, direction: -1 | 1) {
    const target = index + direction;
    const moving = values[index];
    const displaced = values[target];
    if (moving === undefined || displaced === undefined) return;
    const next = [...values];
    next[index] = displaced;
    next[target] = moving;
    onChange(next);
    // Keep focus on the same button of the moved row, or its other button
    // once it reaches an end.
    const atEnd = direction === -1 ? target === 0 : target === values.length - 1;
    setFocusKey(`${target}:${atEnd ? (direction === -1 ? 'down' : 'up') : direction === -1 ? 'up' : 'down'}`);
    setAnnouncement(`Moved ${label_(moving)} to position ${target + 1} of ${values.length}.`);
  }

  const shownError = error ?? typedError;

  return (
    <Stack gap={6}>
      <Autocomplete
        label={label}
        description={
          <>
            {description}
            {description ? ' ' : null}
            {full ? `Up to ${max}: remove one to add another.` : `Up to ${max}.`}
          </>
        }
        placeholder={full ? 'List full' : 'Add a station'}
        value={query}
        disabled={full}
        error={shownError}
        onChange={(value) => {
          if (justPicked.current !== null && value === justPicked.current) {
            justPicked.current = null;
            setQuery('');
            return;
          }
          justPicked.current = null;
          setTypedError(null);
          setQuery(value);
        }}
        onOptionSubmit={(code) => {
          justPicked.current = code;
          add(code);
        }}
        onKeyDown={(event) => {
          // A typed code with no suggestion highlighted. This runs BEFORE
          // the dropdown's own Enter handling, which picks a highlighted
          // option (`onOptionSubmit`); `aria-activedescendant` says one is.
          if (event.key !== 'Enter' || event.currentTarget.getAttribute('aria-activedescendant')) return;
          event.preventDefault();
          const code = typedCode(query, allowStops);
          if (code) {
            add(code);
            setQuery('');
          } else if (query.trim()) {
            setTypedError(
              allowStops
                ? 'Pick a station or stop from the list, or type its three-letter code.'
                : 'Pick a station from the list, or type its three-letter code.',
            );
          }
        }}
        {...suggestionAutocompleteProps(visible, {
          query,
          loading,
          noMatchMessage: allowStops ? 'No matching stations or stops' : 'No matching stations',
        })}
      />
      {values.length > 0 && (
        <Stack component="ol" gap={4} aria-label={label} id={listId} m={0} p={0} style={{ listStyle: 'none' }}>
          {values.map((code, index) => {
            const name = label_(code);
            return (
              <Group component="li" key={`${index}:${code}`} gap="xs" wrap="nowrap" justify="space-between">
                <Text size="sm" style={{ minWidth: 0, overflowWrap: 'anywhere' }}>
                  {ordered && (
                    <Text span c="dimmed" size="sm" mr={6}>
                      {index + 1}.
                    </Text>
                  )}
                  {name}
                </Text>
                <Group gap={2} wrap="nowrap">
                  {ordered && (
                    <>
                      <ActionIcon
                        variant="subtle"
                        className="iconHitArea24"
                        ref={(element) => {
                          buttons.current.set(`${index}:up`, element);
                        }}
                        disabled={index === 0}
                        onClick={() => move(index, -1)}
                        aria-label={`Move ${name} up`}
                      >
                        <Chevron up />
                      </ActionIcon>
                      <ActionIcon
                        variant="subtle"
                        className="iconHitArea24"
                        ref={(element) => {
                          buttons.current.set(`${index}:down`, element);
                        }}
                        disabled={index === values.length - 1}
                        onClick={() => move(index, 1)}
                        aria-label={`Move ${name} down`}
                      >
                        <Chevron up={false} />
                      </ActionIcon>
                    </>
                  )}
                  <ActionIcon
                    color="red"
                    variant="subtle"
                    className="iconHitArea24"
                    onClick={() => remove(index)}
                    aria-label={`Remove ${name} from ${itemNoun}s`}
                  >
                    <XIcon />
                  </ActionIcon>
                </Group>
              </Group>
            );
          })}
        </Stack>
      )}
      <VisuallyHidden role="status" aria-live="polite">
        {announcement}
      </VisuallyHidden>
    </Stack>
  );
}
