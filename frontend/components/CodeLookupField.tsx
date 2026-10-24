'use client';

import { useEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent, type ReactNode } from 'react';
import {
  CloseButton,
  Combobox,
  InputBase,
  ScrollArea,
  VisuallyHidden,
  useCombobox,
  type ComboboxProps,
  type InputBaseProps,
} from '@mantine/core';
import { useId } from '@mantine/hooks';
import type { CodeLabelCache } from '@/lib/codeLabels';
import { NO_MATCH_OPTION_VALUE } from '@/lib/autocompleteNoMatch';
import { useSuggestions } from '@/lib/useSuggestions';
import type { Suggestion } from '@/lib/types';

type Search = (q: string, signal: AbortSignal) => Promise<Suggestion[]>;

export interface CodeLookupFieldProps extends Omit<
  InputBaseProps,
  'value' | 'onChange' | 'rightSection' | 'inputContainer' | 'component'
> {
  /** The code the field holds (`EUS`), or while nothing is picked, the
   * text typed so far. */
  value: string;
  /** Called with the picked code, `''` once cleared, or while nothing is
   * picked, the typed text (so a parent's own checks see what was typed,
   * as before the field showed names). */
  onChange: (value: string) => void;
  search: Search;
  /** Where the field's code -> label pairs live (`lib/codeLabels.ts`). */
  labels: CodeLabelCache;
  /** A dropdown row's text (`EUS · London Euston`). */
  optionLabel: (suggestion: Suggestion) => ReactNode;
  /** The inert row shown when a search finds nothing. */
  noMatchMessage: string;
  /** What a typed code stands for, upper-cased (blur or Enter picks it
   * when no suggestion's code or name matches exactly); `null` when the
   * text is no code. Omit to accept only listed suggestions. */
  parseTypedCode?: ((text: string) => string | null) | undefined;
  /** The label `value` has, when the page already knows it (a server
   * page's own lookup), so the field never shows the bare code. */
  knownLabel?: string | undefined;
  /** The field's name in "Clear From" and "From set to London Euston
   * (EUS)"; defaults to `label` (without a trailing bracket) or
   * `aria-label`. */
  fieldName?: string | undefined;
  /** Called with the suggestion a pick chose, beside `onChange`. */
  onSuggestionPick?: ((suggestion: Suggestion) => void) | undefined;
  /** Called with each new list of live suggestions. */
  onSuggestionsChange?: ((suggestions: Suggestion[]) => void) | undefined;
  comboboxProps?: ComboboxProps | undefined;
  placeholder?: string | undefined;
  'aria-label'?: string | undefined;
  name?: string | undefined;
  autoFocus?: boolean | undefined;
  'data-autofocus'?: boolean | undefined;
  onBlur?: (() => void) | undefined;
  id?: string | undefined;
  readOnly?: boolean | undefined;
}

/** The class `Autocomplete`'s own options carry, so these rows look the same. */
const OPTION_CLASS = (Combobox.classes as Record<string, string | undefined>).optionsDropdownOption ?? '';

type Announcement = { kind: 'set'; code: string } | { kind: 'cleared' } | null;

function sameText(a: string, b: string): boolean {
  return a.trim().toUpperCase() === b.trim().toUpperCase();
}

function defaultFieldName(label: ReactNode, ariaLabel: string | undefined): string {
  if (typeof label === 'string') return label.replace(/\s*\([^)]*\)\s*$/, '');
  return ariaLabel ?? 'Field';
}

/** A lookup field that holds a code but shows its name: pick "London
 * Euston" and it reads "London Euston (EUS)" while `onChange` gets `EUS`.
 * The station flavour is `StationField`, the operator one `OperatorField`.
 *
 * - While a code is picked the text is its label and nothing is searched;
 *   any edit drops the pick and searches the new text. Focus selects the
 *   whole label, so typing replaces it.
 * - A typed code (or a suggestion's exact name) followed by blur or Enter
 *   is picked, so typing `EUS` still works. A code with no known name
 *   shows bare.
 * - A code set from outside (a deep link, a prefill) shows its label from
 *   the cache, looked up once if unknown.
 * - A clear button appears while something is picked; clearing returns
 *   focus to the input. A polite live region says what was picked.
 *
 * Built on `useCombobox` rather than `Autocomplete`, which writes an
 * option's label into the field and leaves no way to tell a pick from a
 * typed label. The no-match row is the same inert option
 * `lib/autocompleteNoMatch.ts` describes. */
export function CodeLookupField({
  value,
  onChange,
  search,
  labels,
  optionLabel,
  noMatchMessage,
  parseTypedCode,
  knownLabel,
  fieldName,
  onSuggestionPick,
  onSuggestionsChange,
  comboboxProps,
  onBlur,
  label,
  'aria-label': ariaLabel,
  id: idProp,
  ...inputProps
}: CodeLookupFieldProps) {
  const combobox = useCombobox();
  const disabled = inputProps.disabled === true;
  const readOnly = inputProps.readOnly === true;
  const inputRef = useRef<HTMLInputElement>(null);
  const id = useId(idProp);
  /** Whether a `value` set from outside reads as a code (shown as picked)
   * rather than text still being typed (a reordered list's draft). */
  function isCodeValue(next: string): boolean {
    return !parseTypedCode || parseTypedCode(next) !== null || labels.get(next) !== undefined;
  }
  const [selected, setSelected] = useState<string | null>(() => {
    if (!value.trim()) return null;
    if (knownLabel) labels.set(value, knownLabel);
    return isCodeValue(value) ? value.trim() : null;
  });
  const [draft, setDraft] = useState(() => (value.trim() && selected === null ? value : ''));
  const [seen, setSeen] = useState(value);
  const [announcement, setAnnouncement] = useState<Announcement>(null);
  // Left with typed text before its search came back: an exact match in
  // the results, when they come, is still picked.
  const [awaitingMatch, setAwaitingMatch] = useState(false);

  // A new `value` from the parent that isn't what this field last sent
  // (bar the parent's own case or whitespace tidying) is a code set from
  // outside: show it as picked.
  if (value !== seen) {
    setSeen(value);
    if (!sameText(value, seen)) {
      const code = value.trim() && isCodeValue(value);
      setSelected(code ? value.trim() : null);
      setDraft(code ? '' : value);
      setAnnouncement(null);
    } else if (selected !== null && value.trim()) {
      setSelected(value.trim());
    }
  }

  useSyncExternalStore(labels.subscribe, labels.version, labels.version);
  const selectedLabel = selected === null ? null : (labels.get(selected) ?? selected);
  const text = selectedLabel ?? draft;

  useEffect(() => {
    if (selected !== null) labels.resolve(selected);
  }, [labels, selected]);

  const { suggestions, loading } = useSuggestions(selected === null ? draft : '', search);
  useEffect(() => {
    labels.remember(suggestions);
    onSuggestionsChange?.(suggestions);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reports each new list once, whatever the callback's identity
  }, [labels, suggestions]);

  function emit(next: string) {
    setSeen(next);
    onChange(next);
  }

  function pick(code: string, suggestion?: Suggestion) {
    if (suggestion) {
      labels.remember([suggestion]);
      onSuggestionPick?.(suggestion);
    }
    setSelected(code);
    setDraft('');
    setAnnouncement({ kind: 'set', code });
    setAwaitingMatch(false);
    emit(code);
  }

  /** The suggestion whose code, name or label is exactly `typed`. */
  function exactMatch(typed: string): Suggestion | undefined {
    const lower = typed.toLowerCase();
    return (
      suggestions.find((s) => s.code.toLowerCase() === lower) ??
      suggestions.find((s) => s.name.toLowerCase() === lower || labels.get(s.code)?.toLowerCase() === lower)
    );
  }

  /** Blur or Enter with typed text: an exact code or name match, else a
   * typed code, is a pick; anything else stays typed text (and is matched
   * against the results of a search still running). */
  function commitTyped() {
    if (selected !== null) return;
    const typed = draft.trim();
    if (!typed) return;
    const match = exactMatch(typed);
    if (match) {
      pick(match.code, match);
      return;
    }
    const code = parseTypedCode?.(typed);
    if (code) pick(code);
    else setAwaitingMatch(true);
  }

  useEffect(() => {
    if (!awaitingMatch || selected !== null) return;
    const match = exactMatch(draft.trim());
    // eslint-disable-next-line react-hooks/set-state-in-effect -- picks the exact match a search finished after the field was left
    if (match) pick(match.code, match);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs as each search's results arrive
  }, [suggestions]);

  function handleTextChange(next: string) {
    setSelected(null);
    setDraft(next);
    setAnnouncement(null);
    setAwaitingMatch(false);
    emit(next);
    combobox.openDropdown();
    combobox.updateSelectedOptionIndex();
  }

  function handleClear() {
    setSelected(null);
    setDraft('');
    setAnnouncement({ kind: 'cleared' });
    emit('');
    inputRef.current?.focus();
  }

  function handleKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key !== 'Enter') return;
    // An option highlighted in the open dropdown is Mantine's to pick.
    if (combobox.dropdownOpened && combobox.getSelectedOptionIndex() !== -1) return;
    commitTyped();
  }

  const name = fieldName ?? defaultFieldName(label, ariaLabel);
  const announced =
    announcement === null
      ? ''
      : announcement.kind === 'cleared'
        ? `${name} cleared`
        : `${name} set to ${labels.get(announcement.code) ?? announcement.code}`;

  const showNoMatch = selected === null && draft.trim().length > 0 && !loading && suggestions.length === 0;
  const options = suggestions.map((suggestion) => (
    <Combobox.Option key={suggestion.code} value={suggestion.code} className={OPTION_CLASS}>
      {optionLabel(suggestion)}
    </Combobox.Option>
  ));
  const hasOptions = options.length > 0 || showNoMatch;

  return (
    <Combobox
      store={combobox}
      readOnly={readOnly}
      onOptionSubmit={(code) => {
        if (code === NO_MATCH_OPTION_VALUE) return;
        pick(
          code,
          suggestions.find((s) => s.code === code),
        );
        combobox.closeDropdown();
      }}
      {...comboboxProps}
    >
      <Combobox.Target withExpandedAttribute autoComplete="off">
        <InputBase
          {...inputProps}
          id={id}
          ref={inputRef}
          label={label}
          aria-label={ariaLabel}
          value={text}
          onChange={(event) => handleTextChange(event.currentTarget.value)}
          onFocus={(event) => {
            event.currentTarget.select();
            combobox.openDropdown();
          }}
          onClick={() => combobox.openDropdown()}
          onBlur={() => {
            combobox.closeDropdown();
            commitTyped();
            onBlur?.();
          }}
          onKeyDown={handleKeyDown}
          rightSectionPointerEvents="all"
          rightSection={
            selected !== null && !disabled && !readOnly ? (
              <CloseButton
                size="sm"
                aria-label={`Clear ${name}`}
                onMouseDown={(event) => event.preventDefault()}
                onClick={handleClear}
              />
            ) : undefined
          }
          inputContainer={(children) => (
            <>
              {children}
              <VisuallyHidden role="status" aria-live="polite">
                {announced}
              </VisuallyHidden>
            </>
          )}
        />
      </Combobox.Target>
      <Combobox.Dropdown hidden={disabled || readOnly || !hasOptions} data-composed>
        <Combobox.Options {...(label ? { labelledBy: `${id}-label` } : ariaLabel ? { 'aria-label': ariaLabel } : {})}>
          <ScrollArea.Autosize mah={220} type="scroll" scrollbarSize="var(--combobox-padding)" offsetScrollbars="y">
            {options}
            {showNoMatch && (
              <Combobox.Option value={NO_MATCH_OPTION_VALUE} disabled className={OPTION_CLASS}>
                {noMatchMessage}
              </Combobox.Option>
            )}
          </ScrollArea.Autosize>
        </Combobox.Options>
      </Combobox.Dropdown>
    </Combobox>
  );
}
