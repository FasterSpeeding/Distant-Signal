import { describe, expect, it } from 'vitest';
import {
  transitIncidentPreviewAlt,
  transitIncidentPreviewCard,
  transitIncidentPreviewDescription,
  transitIncidentPreviewImage,
  transitIncidentPreviewVersion,
} from './transitIncidentPreview';
import { DELAY_DETAIL } from '@/test/transitFixtures';
import type { TransitSituationDetail } from './types';

// 14 Oct 2026 is in BST: UK time is UTC+1.
const NOW = Date.parse('2026-10-14T10:00:00Z');
const LOOKUPS = { networkName: 'Manchester Metrolink', now: NOW };

function detail(overrides: Partial<TransitSituationDetail> = {}): TransitSituationDetail {
  return { ...DELAY_DETAIL, ...overrides };
}

describe('transitIncidentPreviewCard', () => {
  it('bands an active disruption with its status, and names its lines and network', () => {
    const card = transitIncidentPreviewCard(detail(), LOOKUPS);
    expect(card.band).toEqual({ tone: 'mild', label: 'Minor delays', aside: 'Updated Wed 14 Oct, 10:00' });
    expect(card.title).toBe('Delays between Piccadilly and Eccles');
    expect(card.chips).toEqual({ names: ['Blue Line'], more: 0 });
    expect(card.rows).toEqual(['Manchester Metrolink', 'Since Wed 14 Oct, 07:00 · No end time given (UK time)']);
    expect(card.lead).toBeNull();
  });

  it('says planned, upcoming and ended in words', () => {
    expect(transitIncidentPreviewCard(detail({ planned: true }), LOOKUPS).band.label).toBe('Planned work in progress');
    const upcoming = transitIncidentPreviewCard(
      detail({
        state: 'upcoming',
        planned: true,
        starts: '2026-10-18T04:00:00Z',
        validity: [{ start: '2026-10-18T04:00:00Z', end: '2026-10-19T22:00:00Z' }],
      }),
      LOOKUPS,
    );
    expect(upcoming.band).toMatchObject({ tone: 'planned', label: 'Upcoming planned work' });
    expect(upcoming.rows[1]).toBe('Runs Sun 18 Oct, 05:00 to Mon 19 Oct, 23:00 (UK time)');
    const ended = transitIncidentPreviewCard(detail({ state: 'ended', endedAt: '2026-10-14T09:30:00Z' }), LOOKUPS);
    expect(ended.band).toMatchObject({ tone: 'informational', label: 'Ended' });
    expect(ended.rows[1]).toBe('Last listed Wed 14 Oct, 10:30 (UK time)');
  });

  it('names the stops of a notice, which affects no line', () => {
    const card = transitIncidentPreviewCard(detail({ affects: [], severity: 26, stops: ['9400ZZMAPIC', 'X'] }), {
      ...LOOKUPS,
      stopNames: new Map([['9400ZZMAPIC', 'Piccadilly']]),
    });
    expect(card.band).toMatchObject({ tone: 'informational', label: 'Notice in effect' });
    expect(card.lead).toBe('At Piccadilly');
    expect(card.chips.names).toEqual([]);
  });
});

describe('the rest of the preview', () => {
  it('describes what and where, not the state', () => {
    expect(transitIncidentPreviewDescription(detail(), 'Manchester Metrolink')).toBe(
      'Disruption on Blue Line, Manchester Metrolink.',
    );
    expect(
      transitIncidentPreviewDescription(
        detail({ affects: [], summary: 'Lift out of service at Piccadilly.' }),
        'Manchester Metrolink',
      ),
    ).toBe('Notice, Manchester Metrolink: Lift out of service at Piccadilly.');
  });

  it('versions the image by what it shows, not by each snapshot', () => {
    const base = transitIncidentPreviewVersion(detail());
    expect(transitIncidentPreviewVersion(detail({ updatedAt: '2026-10-14T09:05:00Z' }))).toBe(base);
    expect(transitIncidentPreviewVersion(detail({ state: 'ended' }))).not.toBe(base);
    const card = transitIncidentPreviewCard(detail(), LOOKUPS);
    const image = transitIncidentPreviewImage('metrolink', detail(), transitIncidentPreviewAlt(card, 'Distant Signal'));
    expect(image.url).toBe(`/transit/metrolink/incidents/S1/card.png?v=${base}`);
    expect(image.alt).toBe('Delays between Piccadilly and Eccles (minor delays) on Distant Signal');
  });
});
