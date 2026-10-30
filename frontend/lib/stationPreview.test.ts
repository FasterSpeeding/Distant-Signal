import { describe, expect, it } from 'vitest';
import { SNAPSHOT_NOTE } from './linePreview';
import { stationPreviewCard, stationPreviewImage, stationPreviewVersion } from './stationPreview';
import type { LineStatus, LineStatusReport } from './types';

const NOW = Date.parse('2026-10-30T14:05:00Z');

function report(id: string, name: string, severity: number, reason: string, computedAt = '2026-10-30T14:03:00Z') {
  const status: LineStatus = {
    statusSeverity: severity,
    statusSeverityDescription: '',
    reason,
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'available' },
    fullCoverageAvailability: { state: 'not-enabled' },
  };
  return {
    $type: 'Line',
    id,
    name,
    modeName: 'national-rail',
    operators: [],
    lineStatuses: [status],
    computedAt,
  } satisfies LineStatusReport;
}

const MINOR = report('southern-coastway-east', 'Coastway East', 9, 'Delays between Haywards Heath and Lewes');
const SEVERE = report(
  'southern-brighton-main-line',
  'Brighton Main Line',
  6,
  'Signalling fault at Lewes',
  '2026-10-30T14:04:00Z',
);

describe('stationPreviewCard', () => {
  it('bands the station by the worst line through it and lists the disrupted lines, worst first', () => {
    expect(stationPreviewCard('Lewes (LWS)', { coverage: 'covered', reports: [MINOR, SEVERE] }, NOW)).toEqual({
      band: { tone: 'severe', label: 'Severe delays', aside: 'As of Fri 30 Oct, 14:04' },
      title: 'Lewes (LWS)',
      lead: 'Signalling fault at Lewes',
      chips: { names: ['Brighton Main Line', 'Coastway East'], more: 0 },
      rows: [SNAPSHOT_NOTE],
    });
  });

  it('says there are no disruptions, as of now, when every line is fine', () => {
    expect(stationPreviewCard('Woking (WOK)', { coverage: 'covered', reports: [] }, NOW)).toEqual({
      band: { tone: 'good', label: 'No disruptions', aside: 'As of Fri 30 Oct, 14:05' },
      title: 'Woking (WOK)',
      lead: null,
      chips: { names: [], more: 0 },
      rows: [SNAPSHOT_NOTE],
    });
  });

  it('never calls an untracked station good', () => {
    const card = stationPreviewCard('Kyle of Lochalsh (KYL)', { coverage: 'none' }, NOW);
    expect(card.band).toEqual({ tone: 'informational', label: 'No line status tracked', aside: '' });
    expect(card.rows).toEqual([]);
  });

  it('leaves a good line out of the chips', () => {
    const good = report('x', 'Fine Line', 10, '');
    const card = stationPreviewCard('Lewes (LWS)', { coverage: 'covered', reports: [good, MINOR] }, NOW);
    expect(card.chips.names).toEqual(['Coastway East']);
    expect(card.band.label).toBe('Minor delays');
  });
});

describe('stationPreviewVersion', () => {
  it('changes with the disruptions and not with their times or order', () => {
    const base = stationPreviewVersion('Lewes (LWS)', { coverage: 'covered', reports: [MINOR, SEVERE] });
    expect(
      stationPreviewVersion('Lewes (LWS)', {
        coverage: 'covered',
        reports: [SEVERE, { ...MINOR, computedAt: 'later' }],
      }),
    ).toBe(base);
    expect(stationPreviewVersion('Lewes (LWS)', { coverage: 'covered', reports: [MINOR] })).not.toBe(base);
    expect(stationPreviewVersion('Lewes (LWS)', { coverage: 'covered', reports: [] })).not.toBe(base);
    expect(stationPreviewVersion('Lewes (LWS)', { coverage: 'none' })).not.toBe(base);
    expect(stationPreviewVersion('Lewes (LWS)', null)).not.toBe(base);
  });

  it('is the image URL', () => {
    expect(stationPreviewImage('LWS', 'Lewes (LWS)', null, 'Lewes (LWS) on Distant Signal')).toEqual({
      url: expect.stringMatching(/^\/stations\/LWS\/card\.png\?v=[0-9a-f]{8}$/) as unknown,
      alt: 'Lewes (LWS) on Distant Signal',
      width: 1200,
      height: 630,
      type: 'image/png',
    });
  });
});
