import { describe, it, expect } from 'vitest';
import { NO_MATCH_OPTION_VALUE, noMatchOptionContent, withNoMatchPlaceholder } from './autocompleteNoMatch';

describe('withNoMatchPlaceholder', () => {
  it('passes real options through unchanged when active', () => {
    const options = [{ value: 'WOK', label: 'WOK' }];
    expect(withNoMatchPlaceholder(options, 'No matching stations', { active: true })).toBe(options);
  });

  it('passes real options through unchanged when inactive', () => {
    const options = [{ value: 'WOK', label: 'WOK' }];
    expect(withNoMatchPlaceholder(options, 'No matching stations', { active: false })).toBe(options);
  });

  it('swaps in a single disabled placeholder option when there are no real options and the field is active', () => {
    expect(withNoMatchPlaceholder([], 'No matching stations', { active: true })).toEqual([
      { value: NO_MATCH_OPTION_VALUE, label: 'No matching stations', disabled: true },
    ]);
  });

  // I1 (2026-09-17 whole-branch review): the placeholder used to carry no
  // `disabled` field at all, despite this module's own doc comment claiming
  // it did. Mantine's `OptionsDropdown` forwards `disabled: data.disabled`
  // straight through to `Combobox.Option`, whose click handler is only a
  // no-op `if (!disabled)` -- so an un-disabled placeholder could actually
  // be selected, writing the literal message text into the field.
  it('marks the placeholder disabled, so ComboboxOption cannot select it', () => {
    const [placeholder] = withNoMatchPlaceholder([], 'No matching stations', { active: true });
    expect(placeholder.disabled).toBe(true);
  });

  // I2: `active: false` must return the empty array as-is, never the
  // placeholder -- this is what stops the "no matches" message from
  // appearing before a search has actually run (on focus of a blank field)
  // or while one is still in flight.
  it('returns the real (possibly empty) options unchanged when inactive, even with none', () => {
    expect(withNoMatchPlaceholder([], 'No matching stations', { active: false })).toEqual([]);
  });
});

describe('noMatchOptionContent', () => {
  it('returns the given message for the placeholder sentinel value', () => {
    expect(noMatchOptionContent(NO_MATCH_OPTION_VALUE, 'No matching stations')).toBe('No matching stations');
  });

  it('returns null for any real option value', () => {
    expect(noMatchOptionContent('WOK', 'No matching stations')).toBeNull();
  });
});
