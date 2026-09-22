'use client';

import { useEffect, useRef, useState } from 'react';
import { ActionIcon, CloseButton, Group } from '@mantine/core';
import { TimeInput } from '@mantine/dates';

/** Decorative clock face. `@tabler/icons-react` isn't a project dependency
 * (see `InfoIcon.tsx`'s own note), so this is an inline SVG in the same
 * house style -- 16px, `currentColor`, `aria-hidden`, with the accessible
 * name living on the `ActionIcon` that wraps it. */
function ClockIcon() {
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
      <circle cx="12" cy="12" r="9" />
      <polyline points="12 7 12 12 15.5 14" />
    </svg>
  );
}

/** What a half-entered time says. Deliberately names the way out as well
 * as the problem: the field is optional, so "clear it" is as valid a
 * resolution as finishing the time, and on a touch device the clear button
 * beside this message is the only one of the two that's one tap away. */
export const INCOMPLETE_TIME_MESSAGE = 'Enter a complete time, or clear this field';

/** A single optional time-of-day filter: `@mantine/dates`' `TimeInput`
 * (a native `<input type="time">`) plus the affordances a native time
 * input doesn't give you here -- two that Mantine's own stylesheet takes
 * away, and one the platform never had.
 *
 * Why the wrapper exists at all. `TimeInput` on its own is not a
 * "pick a time" control in a desktop browser, despite being a native time
 * input. `@mantine/dates/styles.css` (imported app-wide by
 * `app/globals.css`, and confirmed present in the deployed CSS bundle)
 * ships, on `TimeInput`'s own `.m_468e7eda` input class:
 *
 *     appearance: none;
 *     ::-webkit-calendar-picker-indicator { display: none }
 *     ::-webkit-clear-button              { display: none }
 *
 * -- so on every Blink/WebKit browser the clock button that opens the
 * platform time picker, and the button that empties the field, are both
 * suppressed, leaving a bare segmented HH:MM field you can only type or
 * arrow through. (Tapping the field still opens the OS wheel on iOS/Android,
 * so the picker was only ever missing on desktop; the *clearing* gap is
 * worse on mobile, where a wheel picker offers no way back to empty at all
 * and there is no keyboard to Backspace with.)
 *
 * So the two buttons are put back explicitly, in the input's right section:
 *
 * - A clock `ActionIcon` calling `showPicker()`, the standard DOM API for
 *   opening a form control's own picker (Chrome/Edge 99+, Firefox 101+,
 *   Safari 16+). It is wrapped in a `try`/`catch` and an optional call
 *   because `showPicker` throws rather than returning a failure in the
 *   cases it refuses (no user activation, a cross-origin frame), and
 *   because a browser without a time picker to show simply may not
 *   implement it -- in either case the field is still perfectly usable by
 *   typing, so nothing is worth surfacing to the caller.
 * - A `CloseButton`, so an optional filter can always be taken back off --
 *   matching the `clearable` `DatePickerInput` these fields sit alongside.
 *   Both are `size="md"` rather than the `sm` that visually suits a
 *   36px-tall input, to clear WCAG 2.2 SC 2.5.8's 24x24 minimum target:
 *   Mantine's `sm` is 22px, under it, and its `md` is 28px (`--ai-size-md`
 *   / `--cb-size-md` are `1.75rem`), measured at 28x28 in a real browser.
 *   The clear button in particular
 *   exists to fix a touch-only problem, so it is the last control on this
 *   form that should be hard to hit.
 *
 * The third gap, and the reason this component holds state at all: a
 * native time input reports a HALF-ENTERED time as `''`. Type "09" into
 * the hour segment and leave the minutes blank and `value` is the empty
 * string -- per spec the value IDL attribute is `''` whenever the contents
 * aren't a valid time string -- indistinguishable, to a `value`-only
 * caller, from a field nobody touched. For an OPTIONAL filter that is a
 * real trap: the field visibly reads "09:--" and the search would quietly
 * run without it. The control does set `validity.badInput` in that state,
 * so this component reads that after every edit and (a) shows
 * `INCOMPLETE_TIME_MESSAGE` inline, in the form's own error style rather
 * than leaving it to the browser's native submit bubble, (b) renders the
 * clear button, which is otherwise gated on `value` and so would be absent
 * exactly when a touch user most needs it, and (c) reports upward through
 * `onIncompleteChange`, so the owning form can refuse to search on a
 * filter the caller plainly meant to set. Anything that empties or
 * completes the field clears all three.
 *
 * Deliberately NOT `@mantine/dates`' `TimePicker` (the segmented-field +
 * dropdown control), even though it has `clearable` and `withDropdown`
 * built in: it renders its own text fields rather than a native time
 * input, so it gives up the OS wheel picker on mobile -- where a native
 * `type="time"` is at its best and where most of this app's traffic is.
 * (Its own half-entered state behaves the same as the native one above;
 * that is NOT a point of difference between them, and an earlier version
 * of this comment wrongly claimed it was.)
 *
 * The value contract is the plain `TimeInput` one, minus the event
 * wrapper: `onChange` receives a bare `"HH:MM"` (`step` is 60, so never
 * seconds) or `''` when the field is empty or incomplete.
 *
 * `name` is the field's own short name, used to build unique accessible
 * names for the two buttons -- four of these render on one form, so
 * "Clear" and "Pick a time" alone would be four indistinguishable pairs in
 * a screen reader's control list. */
export function TimeFilterInput({
  label,
  name,
  description,
  value,
  onChange,
  onIncompleteChange,
  error,
}: {
  label: string;
  name: string;
  description: string;
  value: string;
  onChange: (value: string) => void;
  onIncompleteChange: (incomplete: boolean) => void;
  error: string | null;
}) {
  const ref = useRef<HTMLInputElement>(null);
  const [incomplete, setIncomplete] = useState(false);

  /** Records whether `input` is currently mid-entry, and tells the owner.
   * `validity` is optional-chained because a browser that degrades
   * `type="time"` to a plain text box has no `badInput` concept to report
   * -- there, an incomplete time is just text, which the caller's own
   * format check still catches.
   *
   * Only ever fires `onIncompleteChange` on a TRANSITION, which is what
   * makes the unmount retraction below necessary rather than merely tidy:
   * a remounted field starts at `incomplete = false` and so reports
   * nothing at all, and could not talk an owner out of a `true` it was
   * still holding from the field's previous life. */
  function syncIncomplete(input: HTMLInputElement) {
    const next = input.validity?.badInput ?? false;
    if (next === incomplete) return;
    setIncomplete(next);
    onIncompleteChange(next);
  }

  // Retract on unmount. Any caller may render this field conditionally --
  // `TrainSearchForm` renders the two arrival filters only while Stops at
  // is set -- and a field that vanishes mid-entry would otherwise leave
  // its owner holding `true` forever: the owner cannot see the unmount,
  // and the replacement field (see `syncIncomplete` above) never reports
  // the `false` that would clear it. Left unhandled that is a form stuck
  // refusing to submit with no error text anywhere to explain why.
  //
  // Through a ref, so the cleanup is bound to unmount only. Every call
  // site passes a fresh inline closure each render, so depending on the
  // callback itself would re-run this on every render -- retracting, and
  // then immediately contradicting, a `true` the field is still reporting.
  const reportIncomplete = useRef(onIncompleteChange);
  reportIncomplete.current = onIncompleteChange;
  useEffect(
    () => () => {
      reportIncomplete.current(false);
    },
    [],
  );

  function openPicker() {
    try {
      ref.current?.showPicker?.();
    } catch {
      // `showPicker` throws (NotAllowedError/InvalidStateError) rather than
      // no-opping when it declines. Typing into the field still works, so
      // there is nothing to report -- see this component's doc comment.
    }
  }

  function clear() {
    onChange('');
    if (ref.current) {
      // The DOM value has to be emptied directly as well as through
      // `onChange`. A half-entered field is already reporting `value` as
      // `''`, so React sees no change to its controlled `value` prop, does
      // not touch the input, and the stale "09:--" segments would survive
      // a press of a button whose whole job is to empty them.
      ref.current.value = '';
      syncIncomplete(ref.current);
      // Focus goes back to the field rather than being dropped on a button
      // that is about to unmount itself -- otherwise clearing with the
      // keyboard sends focus to the top of the document.
      ref.current.focus();
    }
  }

  const showClear = value !== '' || incomplete;

  return (
    <TimeInput
      ref={ref}
      // Task 3.6.13: a native `<input type="time">`'s displayed segments
      // (12h + AM/PM vs 24h) follow the INPUT's own effective locale, not
      // this app's UI copy or `<html lang>` -- Chromium and Firefox both
      // resolve it from the nearest `lang` attribute, defaulting to the
      // browser's own UI language when none is set. A visitor running an
      // en-US-language browser (common even on a UK device/OS) got a
      // 12-hour "--:-- --" skeleton on this one control, on an otherwise
      // all-24h site (`lib/dateFormat.ts`'s `formatTime`, every other
      // displayed time). `lang="en-GB"` pins this field's OWN rendering to
      // a 24h-clock locale unconditionally, independent of whatever the
      // browser's UI language is -- it does not affect the surrounding
      // page's language for assistive tech, only this one native control's
      // internal segment rendering (the same override technique this input
      // type has no dedicated 12h/24h prop for -- confirmed against
      // `@mantine/dates`' own `TimeInputProps`, which exposes no such
      // option, only forwards ordinary `<input>` props like this one).
      lang="en-GB"
      label={label}
      // Confirmed by a standalone repro this session: `lang="en-GB"` above
      // pins the INPUT's own segment rendering (09/59 vs AM/PM) on
      // Firefox/Safari, but Chromium's native `<input type="time">` picker
      // chrome ignores `lang` for its own display and shows a 12-hour
      // AM/PM face regardless -- a genuine platform limitation with no
      // patch through `lang`, confirmed against Chromium's own source
      // (LocaleConvertedFromLang is deliberately not consulted for the
      // picker UI). Replacing the native input with a masked text field
      // would fix this but throws away the OS wheel picker this component's
      // own doc comment explains is the whole reason it stays native
      // (most of this app's traffic is mobile, where a native `type="time"`
      // is at its best). The least-destructive fix that still resolves the
      // confusion -- someone types "19:00", the input (or its picker) shows
      // "7:00 PM", and they can't tell if that's the site or their own
      // typo -- is a small, always-visible hint confirming the field reads
      // and stores 24-hour time, appended to the caller's own field-specific
      // description rather than duplicated at every TimeFilterInput call
      // site.
      description={`${description} Uses a 24-hour clock, e.g. 19:00 for 7pm.`}
      value={value}
      onChange={(event) => {
        syncIncomplete(event.currentTarget);
        onChange(event.currentTarget.value);
      }}
      // Also on blur, not only on change: a browser is free to stop firing
      // `input` events once the value has settled at `''`, so leaving the
      // field is the last reliable chance to notice it was left half-done.
      onBlur={(event) => syncIncomplete(event.currentTarget)}
      // The caller's own error wins when it has one -- it is about the
      // value that arrived, which is strictly more specific than "this
      // isn't finished". In practice the two can't collide anyway: an
      // incomplete field reports `value` as `''`, and these filters only
      // validate a non-empty value.
      error={error ?? (incomplete ? INCOMPLETE_TIME_MESSAGE : null)}
      // `all`, because the default (`none`) is what lets a click on the
      // right section fall through to focusing the input -- correct for a
      // decorative section, but these are real buttons.
      rightSectionPointerEvents="all"
      // Wide enough for the clock alone, or for the clear button next to
      // it; without this the section keeps Mantine's one-icon default and
      // the two would overlap the HH:MM text.
      rightSectionWidth={showClear ? 74 : 40}
      rightSection={
        <Group gap={2} wrap="nowrap">
          {showClear && <CloseButton size="md" aria-label={`Clear ${name}`} onClick={clear} />}
          <ActionIcon variant="subtle" color="gray" size="md" aria-label={`Pick ${name}`} onClick={openPicker}>
            <ClockIcon />
          </ActionIcon>
        </Group>
      }
    />
  );
}
