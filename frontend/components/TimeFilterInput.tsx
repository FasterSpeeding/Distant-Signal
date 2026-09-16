'use client';

import { useRef } from 'react';
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

/** A single optional time-of-day filter: `@mantine/dates`' `TimeInput`
 * (a native `<input type="time">`) plus the two affordances Mantine's own
 * stylesheet takes away from it.
 *
 * Why the wrapper exists at all. `TimeInput` on its own is not a
 * "pick a time" control in a desktop browser, despite being a native time
 * input. `@mantine/dates/styles.css` (imported app-wide by
 * `app/globals.css`) ships, on `TimeInput`'s own `.m_468e7eda` input class:
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
 * So the two buttons are put back explicitly, in the input's right section
 * -- which is also what `@mantine/dates`' own `TimeInput` documentation
 * recommends for the picker button:
 *
 * - A clock `ActionIcon` calling `showPicker()`, the standard DOM API for
 *   opening a form control's own picker (Chrome/Edge 99+, Firefox 101+,
 *   Safari 16+). It is wrapped in a `try`/`catch` and an optional call
 *   because `showPicker` throws rather than returning a failure in the
 *   cases it refuses (no user activation, a cross-origin frame), and
 *   because a browser without a time picker to show simply may not
 *   implement it -- in either case the field is still perfectly usable by
 *   typing, so nothing is worth surfacing to the caller.
 * - A `CloseButton`, rendered only when there is something to clear, so an
 *   optional filter can always be taken back off -- matching the
 *   `clearable` `DatePickerInput` these fields sit alongside.
 *
 * Deliberately NOT `@mantine/dates`' `TimePicker` (the segmented-field +
 * dropdown control), even though it has `clearable` and `withDropdown`
 * built in. `TimePicker` only emits a value once BOTH its hour and minute
 * segments are filled, reporting a half-entered time as `''` -- which for
 * an optional filter means a field reading "09:--" would be silently
 * dropped from the search rather than applied or objected to. A single
 * native input has no such half-state, and keeps the OS picker on mobile.
 *
 * The value contract is the plain `TimeInput` one, unchanged and
 * deliberately so: `onChange` receives the raw input event, and the value
 * is a bare `"HH:MM"` (`step` is 60, so never seconds) or `''` when empty.
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
  error,
}: {
  label: string;
  name: string;
  description: string;
  value: string;
  onChange: (value: string) => void;
  error: string | null;
}) {
  const ref = useRef<HTMLInputElement>(null);

  function openPicker() {
    try {
      ref.current?.showPicker?.();
    } catch {
      // `showPicker` throws (NotAllowedError/InvalidStateError) rather than
      // no-opping when it declines. Typing into the field still works, so
      // there is nothing to report -- see this component's doc comment.
    }
  }

  return (
    <TimeInput
      ref={ref}
      label={label}
      description={description}
      value={value}
      onChange={(event) => onChange(event.currentTarget.value)}
      error={error}
      // `all`, because the default (`none`) is what lets a click on the
      // right section fall through to focusing the input -- correct for a
      // decorative section, but these are real buttons.
      rightSectionPointerEvents="all"
      // Wide enough for the clock alone, or for the clear button next to
      // it once there is a value; without this the section keeps Mantine's
      // one-icon default and the two would overlap the HH:MM text.
      rightSectionWidth={value ? 60 : 34}
      rightSection={
        <Group gap={2} wrap="nowrap">
          {value && (
            <CloseButton
              size="sm"
              aria-label={`Clear ${name}`}
              onClick={() => {
                onChange('');
                // Focus goes back to the field rather than being dropped on
                // a button that is about to unmount itself (it only renders
                // while there is a value) -- otherwise clearing with the
                // keyboard sends focus to the top of the document.
                ref.current?.focus();
              }}
            />
          )}
          <ActionIcon variant="subtle" color="gray" size="sm" aria-label={`Pick ${name}`} onClick={openPicker}>
            <ClockIcon />
          </ActionIcon>
        </Group>
      }
    />
  );
}
