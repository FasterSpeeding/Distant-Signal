import { describe, it, expect } from 'vitest';
import { memberLabel, MEMBER_PLACEHOLDER_INLINE } from './memberLabel';

describe('memberLabel', () => {
  it('renders a real name exactly as it is, with no suffix', () => {
    expect(memberLabel('Ada Rider', null)).toBe('Ada Rider');
    expect(memberLabel('  Ada Rider  ', null)).toBe('Ada Rider');
  });

  /** A name is a name: a tag alongside one would decorate a real person,
   * which is not what it is for. The backend never sends both, and this is
   * the frontend half of that guarantee. */
  it('never suffixes a member the backend could name', () => {
    expect(memberLabel('Ada Rider', 'a1b2c3')).toBe('Ada Rider');
  });

  it('falls back to the placeholder when there is no name and no tag', () => {
    expect(memberLabel(null, null)).toBe('A member');
    expect(memberLabel(undefined, undefined)).toBe('A member');
    expect(memberLabel('   ', null)).toBe('A member');
    expect(memberLabel(null, null, MEMBER_PLACEHOLDER_INLINE)).toBe('a member');
  });

  /** The bug: on an IdP whose username claim is the user's email by design
   * (Entra ID's UPN), the backend can name nobody, and every row read as
   * the same "A member" -- no email leaked, but no way to tell an
   * eight-person group apart either. */
  it('distinguishes two members who both fall back to the placeholder', () => {
    const one = memberLabel(null, 'a1b2c3');
    const two = memberLabel(null, 'd4e5f6');
    expect(one).toBe('A member (#a1b2c3)');
    expect(two).toBe('A member (#d4e5f6)');
    expect(one).not.toBe(two);
  });

  it('suffixes the mid-sentence placeholder too', () => {
    expect(memberLabel(null, 'a1b2c3', MEMBER_PLACEHOLDER_INLINE)).toBe('a member (#a1b2c3)');
  });

  /** A blank name with a tag is the same case as a null one -- rows
   * written before the backend normalized blanks to null still exist. */
  it('treats a blank name as no name when a tag is present', () => {
    expect(memberLabel('', 'a1b2c3')).toBe('A member (#a1b2c3)');
    expect(memberLabel('  \t ', 'a1b2c3')).toBe('A member (#a1b2c3)');
  });

  /** An older backend (or a cached response from one) sends no tag at all.
   * That must render exactly as it did before the tag existed, not as
   * "A member (#)" or "A member (#undefined)". */
  it('degrades to the bare placeholder when the tag is missing or blank', () => {
    expect(memberLabel(null, undefined)).toBe('A member');
    expect(memberLabel(null, '   ')).toBe('A member');
  });
});
