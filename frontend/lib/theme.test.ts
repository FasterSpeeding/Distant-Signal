import { describe, it, expect } from 'vitest';
import { theme } from './theme';

// Per the grape-theme spec's Testing section: colour is largely not
// unit-testable and shouldn't be asserted shade by shade (that's a job for
// the visual/contrast verification pass, not this suite). What IS worth
// locking down here is that the theme object the whole app depends on
// actually declares grape as primary — the one fact every other file in
// this change (layout.tsx, the converted `c="blue"` call sites, and every
// test file's `MantineProvider`) assumes is true.
describe('theme', () => {
  it('sets grape as the primary colour', () => {
    expect(theme.primaryColor).toBe('grape');
  });
});
