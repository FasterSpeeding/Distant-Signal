import { describe, expect, it } from 'vitest';
import {
  linePreviewCard,
  linePreviewImage,
  linePreviewVersion,
  NO_LINE_STATUS_LABEL,
  sampleRow,
  SNAPSHOT_NOTE,
} from './linePreview';
import type { LineStatus, LineStatusReport } from './types';

const TOCS = new Map([
  ['SE', 'Southeastern'],
  ['TL', 'Thameslink'],
]);

function lineStatus(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 10,
    statusSeverityDescription: 'Good Service',
    reason: '',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'available' },
    fullCoverageAvailability: { state: 'not-enabled' },
    ...overrides,
  };
}

function lineReport(overrides: Partial<LineStatusReport> = {}): LineStatusReport {
  return {
    $type: 'Line',
    id: 'southeastern-chatham',
    name: 'Chatham Main Line',
    modeName: 'national-rail',
    operators: ['SE', 'TL'],
    lineStatuses: [lineStatus()],
    computedAt: '2026-10-30T14:03:00Z',
    ...overrides,
  };
}

const MINOR = lineStatus({
  statusSeverity: 9,
  statusSeverityDescription: 'Minor Delays',
  reason: 'Disruption through Rochester expected until 15:00',
  sampleStats: { total: 42, delayed: 5, cancelled: 1, skipped: 0, avgDelayMinutes: 3.24 },
});

describe('linePreviewCard', () => {
  it('bands a disrupted line by its worst status, in words, as of when it was computed', () => {
    const card = linePreviewCard('Chatham Main Line', lineReport({ lineStatuses: [MINOR] }), ['SE', 'TL'], TOCS);
    expect(card).toEqual({
      band: { tone: 'mild', label: 'Minor delays', aside: 'As of Fri 30 Oct, 14:03' },
      title: 'Chatham Main Line',
      lead: 'Disruption through Rochester expected until 15:00',
      chips: { names: [], more: 0 },
      rows: ['Southeastern and Thameslink', '5 of 42 sampled trains delayed, avg 3.2 min late', SNAPSHOT_NOTE],
    });
  });

  it.each([
    [10, 'good', 'Good service'],
    [9, 'mild', 'Minor delays'],
    [6, 'severe', 'Severe delays'],
    [4, 'planned', 'Planned closure'],
    [0, 'informational', 'Special service'],
    [99, 'informational', 'Unknown'],
  ])('maps severity %i to the %s pill, labelled %j', (severity, tone, label) => {
    const card = linePreviewCard(
      'X',
      lineReport({ lineStatuses: [lineStatus({ statusSeverity: severity })] }),
      [],
      TOCS,
    );
    expect(card.band).toMatchObject({ tone, label });
  });

  it('picks the worst of several statuses and counts the disruptions', () => {
    const severe = lineStatus({ statusSeverity: 6, reason: '<p>Signalling fault at Rainham &amp; Gillingham</p>' });
    const card = linePreviewCard('X', lineReport({ lineStatuses: [MINOR, severe, lineStatus()] }), [], TOCS);
    expect(card.band.label).toBe('Severe delays');
    expect(card.lead).toBe('Signalling fault at Rainham & Gillingham');
    expect(card.rows[0]).toBe('2 disruptions on this line');
  });

  it('says nothing is wrong on a good line, and still that it is a snapshot', () => {
    const card = linePreviewCard('X', lineReport(), ['ZZ'], TOCS);
    expect(card.band).toMatchObject({ tone: 'good', label: 'Good service' });
    expect(card.lead).toBeNull();
    expect(card.rows).toEqual(['ZZ', SNAPSHOT_NOTE]);
  });

  it('never calls a line with no status row yet good', () => {
    const card = linePreviewCard('New Line', null, ['SE'], TOCS);
    expect(card.band).toEqual({ tone: 'informational', label: NO_LINE_STATUS_LABEL, aside: '' });
    expect(card.rows).toEqual(['Southeastern']);
  });

  it('reads an empty report as good, as the page does', () => {
    expect(linePreviewCard('X', lineReport({ lineStatuses: [] }), [], TOCS).band.label).toBe('Good service');
  });
});

describe('sampleRow', () => {
  it('prefers full-coverage figures and leaves out an empty sample', () => {
    const full = lineStatus({
      fullCoverageStats: { total: 100, delayed: 7, cancelled: 0, skipped: 0, avgDelayMinutes: 1 },
    });
    expect(sampleRow([MINOR, full])).toBe('7 of 100 sampled trains delayed, avg 1.0 min late');
    expect(
      sampleRow([lineStatus({ sampleStats: { total: 0, delayed: 0, cancelled: 0, skipped: 0, avgDelayMinutes: 0 } })]),
    ).toBeNull();
    expect(sampleRow([lineStatus()])).toBeNull();
  });
});

describe('linePreviewVersion', () => {
  it('changes with the status and not with the time or the sampled figures', () => {
    const base = linePreviewVersion('X', lineReport({ lineStatuses: [MINOR] }));
    expect(
      linePreviewVersion(
        'X',
        lineReport({
          computedAt: '2026-10-30T14:08:00Z',
          lineStatuses: [
            { ...MINOR, sampleStats: { total: 50, delayed: 9, cancelled: 2, skipped: 1, avgDelayMinutes: 4 } },
          ],
        }),
      ),
    ).toBe(base);
    expect(linePreviewVersion('X', lineReport())).not.toBe(base);
    expect(linePreviewVersion('X', lineReport({ lineStatuses: [{ ...MINOR, reason: 'Other' }] }))).not.toBe(base);
    expect(linePreviewVersion('X', null)).not.toBe(base);
  });

  it('ignores the order the statuses come in', () => {
    const severe = lineStatus({ statusSeverity: 6, reason: 'Fault' });
    expect(linePreviewVersion('X', lineReport({ lineStatuses: [MINOR, severe] }))).toBe(
      linePreviewVersion('X', lineReport({ lineStatuses: [severe, MINOR] })),
    );
  });

  it('is the image URL, with the id encoded', () => {
    expect(linePreviewImage('a b', 'X', null, 'X on Distant Signal')).toEqual({
      url: expect.stringMatching(/^\/lines\/a%20b\/card\.png\?v=[0-9a-f]{8}$/) as unknown,
      alt: 'X on Distant Signal',
      width: 1200,
      height: 630,
      type: 'image/png',
    });
  });
});
