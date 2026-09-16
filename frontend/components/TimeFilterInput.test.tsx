import { describe, it, expect, vi } from 'vitest';
import { useState } from 'react';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TimeFilterInput } from './TimeFilterInput';

/** A controlled host, so each test exercises the same
 * value-in/`onChange`-out contract `TrainSearchForm` itself uses rather
 * than an uncontrolled input that would quietly diverge from it. */
function Harness({
  initial = '',
  error = null,
  onChangeSpy,
}: {
  initial?: string;
  error?: string | null;
  onChangeSpy?: (value: string) => void;
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
    expect(describedTexts).toContain('Only trains at RDG at or after this time.');
  });
});
