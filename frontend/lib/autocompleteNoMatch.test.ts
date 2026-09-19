import { describe, it, expect } from 'vitest';
import { NO_MATCH_OPTION_VALUE, noMatchOptionContent, withNoMatchPlaceholder } from './autocompleteNoMatch';

describe('withNoMatchPlaceholder', () => {
  it('passes real options through unchanged', () => {
    const options = [{ value: 'WOK', label: 'WOK' }];
    expect(withNoMatchPlaceholder(options, 'No matching stations')).toBe(options);
  });

  it('swaps in a single placeholder option when there are no real options', () => {
    expect(withNoMatchPlaceholder([], 'No matching stations')).toEqual([
      { value: NO_MATCH_OPTION_VALUE, label: 'No matching stations' },
    ]);
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
