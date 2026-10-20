import { describe, expect, it } from 'vitest';
import { deletionSummary, preselectedScopes, scopeSentence } from './connectedApps';

describe('connected apps helpers', () => {
  it('words each scope as a sentence', () => {
    expect(scopeSentence('journeys:write')).toBe('Save journeys to your account');
    expect(scopeSentence('journeys:other')).toBe('journeys:other');
  });

  it('preselects only scopes the app may have, else all of them', () => {
    const allowed = ['journeys:read', 'journeys:write', 'journeys:delete'];
    expect(preselectedScopes('journeys:read journeys:share', allowed)).toEqual(['journeys:read']);
    expect(preselectedScopes('journeys:write,journeys:read', allowed)).toEqual(['journeys:read', 'journeys:write']);
    expect(preselectedScopes(undefined, allowed)).toEqual(allowed);
    expect(preselectedScopes('nonsense', allowed)).toEqual(allowed);
  });

  it('summarises a deleted journey for re-saving by hand', () => {
    expect(
      deletionSummary({
        id: 1,
        client: 'ds-mcp',
        journeyId: 42,
        deletedAt: '2026-10-14T10:00:00Z',
        summary: {
          customName: 'Weekend away',
          legs: [{ serviceDate: '2026-10-20', originCrs: 'EUS', destinationCrs: 'MKC', trainUid: 'C11052' }],
        },
      }),
    ).toEqual({ title: 'Weekend away', legs: ['EUS to MKC, on 20 Oct 2026, train C11052'] });
    expect(
      deletionSummary({ id: 2, client: 'ds-mcp', journeyId: 7, deletedAt: '2026-10-14T10:00:00Z', summary: {} }),
    ).toEqual({ title: 'Journey 7', legs: [] });
    // No custom name: the journey's whole route and first day, as every
    // other journey surface names it, not "Journey 9".
    expect(
      deletionSummary({
        id: 3,
        client: 'ds-mcp',
        journeyId: 9,
        deletedAt: '2026-10-14T10:00:00Z',
        summary: {
          customName: null,
          legs: [
            { serviceDate: '2026-10-20', originCrs: 'EUS', destinationCrs: 'MKC', trainUid: null },
            { serviceDate: '2026-10-20', originCrs: 'MKC', destinationCrs: 'BHM', trainUid: null },
          ],
        },
      }).title,
    ).toBe('EUS to BHM, 20 Oct 2026');
  });
});
