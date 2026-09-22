import { describe, it, expect, vi } from 'vitest';
import { useState } from 'react';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TimeFilterInput, INCOMPLETE_TIME_MESSAGE } from './TimeFilterInput';

/** Forces `input.validity.badInput`, which is how a real browser reports a
 * half-entered time ("09:--"). jsdom models neither segment state nor
 * `badInput`, so the only way to exercise that branch here is to say so
 * directly. Verified against a real Chromium: typing only an hour leaves
 * `value === ''` and `validity.badInput === true`. */
function setBadInput(input: HTMLInputElement, badInput: boolean) {
  Object.defineProperty(input, 'validity', {
    configurable: true,
    get: () => ({ badInput }),
  });
}

/** A controlled host, so each test exercises the same
 * value-in/`onChange`-out contract `TrainSearchForm` itself uses rather
 * than an uncontrolled input that would quietly diverge from it. */
function Harness({
  initial = '',
  error = null,
  onChangeSpy,
  onIncompleteSpy,
}: {
  initial?: string;
  error?: string | null;
  onChangeSpy?: (value: string) => void;
  onIncompleteSpy?: (incomplete: boolean) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <>
      <TimeFilterInput
        label="Earliest departure (optional)"
        name="earliest departure"
        description="Only trains at RDG at or after this time."
        value={value}
        onChange={(next) => {
          onChangeSpy?.(next);
          setValue(next);
        }}
        onIncompleteChange={onIncompleteSpy ?? (() => {})}
        error={error}
      />
      <output data-testid="value">{value}</output>
    </>
  );
}

function field() {
  return screen.getByLabelText('Earliest departure (optional)');
}

describe('TimeFilterInput', () => {
  it('renders a native time input stepping by whole minutes', () => {
    renderWithMantine(<Harness />);

    // `type="time"` is what supplies the segmented HH:MM entry and, on a
    // touch device, the OS wheel picker. `step="60"` is what keeps the
    // value at "HH:MM" -- a seconds segment would make it "HH:MM:SS",
    // which is not the shape this app's time filters are parsed as.
    expect(field()).toHaveAttribute('type', 'time');
    expect(field()).toHaveAttribute('step', '60');
  });

  // Task 3.6.13: a native time input's displayed segments (12h + AM/PM vs
  // 24h) follow its own effective `lang`, not the page's -- an en-US
  // browser language rendered a 12-hour "--:-- --" skeleton on this
  // otherwise all-24h UK site. `lang="en-GB"` pins it to a 24h-clock
  // locale regardless of the visitor's own browser language.
  it('pins the native input to a 24h-clock locale, independent of the browser\'s own language', () => {
    renderWithMantine(<Harness />);
    expect(field()).toHaveAttribute('lang', 'en-GB');
  });

  it('reports typed input straight through as the raw value', () => {
    const onChangeSpy = vi.fn();
    renderWithMantine(<Harness onChangeSpy={onChangeSpy} />);

    fireEvent.change(field(), { target: { value: '09:00' } });

    // A plain string, not an event -- the wrapper unwraps
    // `event.currentTarget.value` so callers never see the DOM event.
    expect(onChangeSpy).toHaveBeenCalledWith('09:00');
    expect(screen.getByTestId('value')).toHaveTextContent('09:00');
  });

  it('offers a uniquely-named button to open the platform time picker', () => {
    renderWithMantine(<Harness />);

    // Named after the field, not just "Pick a time": four of these render
    // together on /trains, and four identically-named buttons are
    // indistinguishable in a screen reader's control list.
    const picker = screen.getByRole('button', { name: 'Pick earliest departure' });

    // `showPicker` is the DOM API for opening a control's own picker.
    // jsdom doesn't implement it, which is exactly the "browser that
    // doesn't have one" case the wrapper swallows -- clicking must stay
    // harmless rather than throwing into React's event handler.
    expect(() => fireEvent.click(picker)).not.toThrow();

    const showPicker = vi.fn();
    (field() as HTMLInputElement).showPicker = showPicker;
    fireEvent.click(picker);
    expect(showPicker).toHaveBeenCalledOnce();
  });

  it('swallows a showPicker call the browser refuses', () => {
    renderWithMantine(<Harness />);
    // Browsers throw NotAllowedError/InvalidStateError from `showPicker`
    // rather than returning a failure. The field is still typeable, so
    // there is nothing to surface -- but an uncaught throw here would
    // break the click handler.
    (field() as HTMLInputElement).showPicker = () => {
      throw new DOMException('not allowed', 'NotAllowedError');
    };

    expect(() =>
      fireEvent.click(screen.getByRole('button', { name: 'Pick earliest departure' })),
    ).not.toThrow();
  });

  it('shows a clear button only once there is something to clear', () => {
    renderWithMantine(<Harness />);

    expect(screen.queryByRole('button', { name: 'Clear earliest departure' })).not.toBeInTheDocument();

    fireEvent.change(field(), { target: { value: '09:00' } });

    expect(screen.getByRole('button', { name: 'Clear earliest departure' })).toBeInTheDocument();
  });

  it('empties the field back to "" when cleared, and keeps focus on it', () => {
    renderWithMantine(<Harness initial="09:00" />);

    fireEvent.click(screen.getByRole('button', { name: 'Clear earliest departure' }));

    // `''`, not `null`/`undefined`: callers gate their optional filters on
    // an empty string, so anything else would leak a filter onto the wire.
    expect(screen.getByTestId('value')).toHaveTextContent('');
    expect(field()).toHaveValue('');
    // The clear button unmounts itself on the same click (it only renders
    // while there is a value), so focus has to be handed back deliberately
    // or a keyboard user is dropped at the top of the document.
    expect(field()).toHaveFocus();
  });

  /** The trap a `value`-only wrapper falls into: a native time input
   * reports a HALF-entered time ("09:--") as `''`, exactly like a field
   * nobody touched. On an optional filter that means the search quietly
   * runs without a filter the caller plainly meant to set. */
  describe('a half-entered time', () => {
    /** Deleting the minutes back out of a complete time: the value goes
     * "09:00" -> "" while the hour segment still reads 09. */
    function halfEraseWhileTyping() {
      setBadInput(field() as HTMLInputElement, false);
      fireEvent.change(field(), { target: { value: '09:00' } });
      setBadInput(field() as HTMLInputElement, true);
      fireEvent.change(field(), { target: { value: '' } });
    }

    it('is called out inline rather than passing as an untouched field', () => {
      renderWithMantine(<Harness />);

      halfEraseWhileTyping();

      expect(screen.getByText(INCOMPLETE_TIME_MESSAGE)).toBeInTheDocument();
      // Still `''` on the wire -- the point is that the EMPTINESS is now
      // explained rather than silently accepted.
      expect(screen.getByTestId('value')).toHaveTextContent('');
    });

    it('is reported to the owning form so it can refuse to search', () => {
      const onIncompleteSpy = vi.fn();
      renderWithMantine(<Harness onIncompleteSpy={onIncompleteSpy} />);

      halfEraseWhileTyping();
      expect(onIncompleteSpy).toHaveBeenLastCalledWith(true);

      // ...and retracted once the field is no longer half-done, or the
      // form would stay stuck refusing to search.
      setBadInput(field() as HTMLInputElement, false);
      fireEvent.change(field(), { target: { value: '09:30' } });
      expect(onIncompleteSpy).toHaveBeenLastCalledWith(false);
      expect(screen.queryByText(INCOMPLETE_TIME_MESSAGE)).not.toBeInTheDocument();
    });

    it('is noticed on blur, for a first entry that never produced a value at all', () => {
      const onIncompleteSpy = vi.fn();
      renderWithMantine(<Harness onIncompleteSpy={onIncompleteSpy} />);

      // Typing only "09" into an untouched field never changes `value` off
      // `''`, so no change event carries the news. Leaving the field is the
      // last reliable chance to catch it having been left half-done.
      setBadInput(field() as HTMLInputElement, true);
      fireEvent.blur(field());

      expect(onIncompleteSpy).toHaveBeenLastCalledWith(true);
      expect(screen.getByText(INCOMPLETE_TIME_MESSAGE)).toBeInTheDocument();
    });

    it('offers the clear button, which is otherwise gated on a value it does not have', () => {
      const onIncompleteSpy = vi.fn();
      renderWithMantine(<Harness onIncompleteSpy={onIncompleteSpy} />);

      setBadInput(field() as HTMLInputElement, true);
      fireEvent.blur(field());
      // Without this the button would be absent exactly when a touch user
      // most needs it: a wheel picker offers no way back to empty, and a
      // half-filled field reports no value to gate the button on.
      const clear = screen.getByRole('button', { name: 'Clear earliest departure' });

      setBadInput(field() as HTMLInputElement, false);
      fireEvent.click(clear);

      expect(screen.queryByText(INCOMPLETE_TIME_MESSAGE)).not.toBeInTheDocument();
      expect(onIncompleteSpy).toHaveBeenLastCalledWith(false);
      expect(field()).toHaveValue('');
      expect(screen.queryByRole('button', { name: 'Clear earliest departure' })).not.toBeInTheDocument();
    });

    it('is retracted when the field unmounts mid-entry', () => {
      const onIncompleteSpy = vi.fn();
      const { unmount } = renderWithMantine(<Harness onIncompleteSpy={onIncompleteSpy} />);

      setBadInput(field() as HTMLInputElement, true);
      fireEvent.blur(field());
      expect(onIncompleteSpy).toHaveBeenLastCalledWith(true);

      // A caller may render this field conditionally (TrainSearchForm
      // renders the two arrival filters only while Stops at is set). The
      // owner cannot see the unmount, and a REPLACEMENT field starts at
      // `false` and so reports nothing -- `onIncompleteChange` only fires
      // on a transition. Without this retraction the owner would hold
      // `true` forever and refuse to submit with nothing on screen saying
      // why.
      unmount();

      expect(onIncompleteSpy).toHaveBeenLastCalledWith(false);
    });
  });

  it('keeps the label, description and error wired to the input for assistive tech', () => {
    renderWithMantine(<Harness initial="09:00:30" error="Must be a time like 09:00" />);

    const input = field();
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByText('Must be a time like 09:00')).toBeInTheDocument();

    // Both the description and the error are announced with the field, not
    // just rendered near it.
    const describedBy = input.getAttribute('aria-describedby') ?? '';
    const describedTexts = describedBy
      .split(' ')
      .filter(Boolean)
      .map((id) => document.getElementById(id)?.textContent);
    expect(describedTexts).toContain('Must be a time like 09:00');
    expect(describedTexts).toContain(
      'Only trains at RDG at or after this time. Uses a 24-hour clock, e.g. 19:00 for 7pm.',
    );
  });

  // Chromium's native <input type="time"> picker chrome ignores `lang` for
  // its own AM/PM-vs-24h display (confirmed via a standalone repro this
  // session) -- a genuine platform limitation `lang="en-GB"` alone can't
  // patch. This always-visible hint is the chosen mitigation: it stays
  // legible even where the picker itself still shows "7:00 PM" for a typed
  // "19:00".
  it('always tells the user this field is 24-hour, appended to its own description', () => {
    renderWithMantine(<Harness initial="" />);
    expect(
      screen.getByText('Only trains at RDG at or after this time. Uses a 24-hour clock, e.g. 19:00 for 7pm.'),
    ).toBeInTheDocument();
  });

  it('gives both buttons a target big enough to hit, and keeps them out of the label', () => {
    renderWithMantine(<Harness initial="09:00" />);

    // WCAG 2.2 SC 2.5.8 wants 24x24 CSS px minimum. Mantine's `sm` is
    // 22px and its `md` is 28px, so the size prop is load-bearing here --
    // and it matters most for the clear button, which exists to fix a
    // touch-only problem in the first place.
    for (const name of ['Clear earliest departure', 'Pick earliest departure']) {
      expect(screen.getByRole('button', { name })).toHaveAttribute('data-size', 'md');
    }
    // The buttons live in the input's right section, a sibling of the
    // input -- not inside the <label>, where they would be folded into the
    // field's own accessible name.
    expect(screen.getByText('Earliest departure (optional)').querySelector('button')).toBeNull();
  });
});
