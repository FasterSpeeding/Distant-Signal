import { describe, expect, it } from 'vitest';
import {
  buildLineDiagram,
  diagramStopCount,
  spurLabel,
  type DiagramStop,
  type LineDiagram,
} from './transitLineDiagram';
import type { TransitLineBranch, TransitLineDetail, TransitStationRef } from './types';

const st = (name: string): TransitStationRef => ({ id: name.toUpperCase().replace(/\W/g, ''), name });

function branch(
  id: string,
  names: string[],
  sharePercent: number,
  direction: 'outbound' | 'inbound' = 'outbound',
): TransitLineBranch {
  return { id, direction, name: id, stations: names.map(st), sharePercent };
}

function line(branches: TransitLineBranch[], extra: string[] = []): Pick<TransitLineDetail, 'directions' | 'branches'> {
  const all = [...new Set([...branches.flatMap((b) => b.stations.map((s) => s.name)), ...extra])];
  return { directions: [{ direction: 'outbound', headsign: null, stations: all.map(st) }], branches };
}

/** The diagram as text: `•` a stop, `◉` an end, `[label]` a branch. */
function draw(diagram: LineDiagram): string[] {
  const out: string[] = [];
  const walk = (stops: readonly DiagramStop[], depth: number) => {
    for (const stop of stops) {
      out.push(`${'  '.repeat(depth)}${stop.terminus ? '◉' : '•'} ${stop.station.name}`);
      for (const spur of stop.spurs) {
        out.push(`${'  '.repeat(depth + 1)}[${spurLabel(spur)}]`);
        walk(spur.stops, depth + 2);
      }
    }
  };
  walk(diagram.stops, 0);
  for (const spur of diagram.separate) {
    out.push(`[${spurLabel(spur)}]`);
    walk(spur.stops, 1);
  }
  return out;
}

describe('buildLineDiagram', () => {
  it('draws a plain line once, from its directions when it has no branches', () => {
    const diagram = buildLineDiagram({
      directions: [
        { direction: 'outbound', headsign: 'Eccles', stations: ['Piccadilly', 'Cornbrook', 'Eccles'].map(st) },
        { direction: 'inbound', headsign: 'Piccadilly', stations: ['Eccles', 'Cornbrook', 'Piccadilly'].map(st) },
      ],
    });
    expect(draw(diagram)).toEqual(['◉ Piccadilly', '• Cornbrook', '◉ Eccles']);
    expect(diagram.from?.name).toBe('Piccadilly');
    expect(diagram.to?.name).toBe('Eccles');
    expect(diagram.others).toEqual([]);
  });

  it('adds a stop only one way round where the other direction passes it', () => {
    const diagram = buildLineDiagram({
      directions: [
        { direction: 'outbound', headsign: null, stations: ['A', 'B', 'D'].map(st) },
        { direction: 'inbound', headsign: null, stations: ['D', 'C', 'B', 'A'].map(st) },
      ],
    });
    expect(draw(diagram)).toEqual(['◉ A', '• B', '  [Via C, rejoining at D]', '    • C', '◉ D']);
  });

  it('hangs branches off the stop where they split or join, nested, busiest first', () => {
    // The Northern line's shape: two routes through the middle, a branch
    // joining from Battersea, a split to High Barnet and Mill Hill East off that.
    const diagram = buildLineDiagram(
      line([
        branch(
          'outbound-2',
          ['Morden', 'Kennington', 'Waterloo', 'Euston', 'Camden Town', 'Archway', 'High Barnet'],
          20,
        ),
        branch('outbound-1', ['Morden', 'Kennington', 'Bank', 'Euston', 'Camden Town', 'Hampstead', 'Edgware'], 50),
        branch(
          'outbound-3',
          ['Battersea', 'Kennington', 'Bank', 'Euston', 'Camden Town', 'Archway', 'Finchley Central', 'Mill Hill East'],
          10,
        ),
      ]),
    );
    expect(draw(diagram)).toEqual([
      '◉ Morden',
      '• Kennington',
      '  [Via Waterloo, rejoining at Euston]',
      '    • Waterloo',
      '  [Branch from Battersea, joining at Kennington]',
      '    ◉ Battersea',
      '• Bank',
      '• Euston',
      '• Camden Town',
      '  [Branch to High Barnet]',
      '    • Archway',
      '      [Branch to Mill Hill East]',
      '        • Finchley Central',
      '        ◉ Mill Hill East',
      '    ◉ High Barnet',
      '• Hampstead',
      '◉ Edgware',
    ]);
    // Each stop once.
    expect(diagramStopCount(diagram)).toBe(13);
  });

  it('reads an inbound branch the same way round as the outbound ones', () => {
    const diagram = buildLineDiagram(
      line([
        branch('outbound-1', ['Ealing', 'Earls Court', 'Upminster'], 60),
        branch('inbound-1', ['Upminster', 'Earls Court', 'Olympia'], 40, 'inbound'),
      ]),
    );
    expect(draw(diagram)).toEqual([
      '◉ Ealing',
      '• Earls Court',
      '  [Branch from Olympia, joining at Earls Court]',
      '    ◉ Olympia',
      '◉ Upminster',
    ]);
  });

  it('keeps a loop line passing a stop twice, and a branch that loops back', () => {
    // The Circle line's spiral, and the Central line's Hainault loop.
    const circle = buildLineDiagram(
      line([branch('outbound-1', ['Hammersmith', 'Edgware Road', 'Aldgate', 'Victoria', 'Edgware Road'], 95)]),
    );
    expect(draw(circle)).toEqual(['◉ Hammersmith', '• Edgware Road', '• Aldgate', '• Victoria', '◉ Edgware Road']);
    const central = buildLineDiagram(
      line([
        branch('outbound-1', ['Ruislip', 'Leytonstone', 'Woodford', 'Epping'], 50),
        branch('outbound-2', ['Ruislip', 'Leytonstone', 'Newbury Park', 'Hainault', 'Roding Valley', 'Woodford'], 30),
      ]),
    );
    expect(draw(central)).toEqual([
      '◉ Ruislip',
      '• Leytonstone',
      '  [Via Newbury Park, rejoining at Woodford]',
      '    • Newbury Park',
      '    • Hainault',
      '    • Roding Valley',
      '• Woodford',
      '◉ Epping',
    ]);
  });

  it('lists stations no branch draws, never as if adjacent, and runs that touch nothing apart', () => {
    const diagram = buildLineDiagram(
      line([branch('outbound-1', ['A', 'B'], 80), branch('outbound-2', ['X', 'Y'], 10)], ['Rare']),
    );
    expect(draw(diagram)).toEqual(['◉ A', '◉ B', '[Also X to Y]', '  ◉ X', '  ◉ Y']);
    expect(diagram.others.map((s) => s.name)).toEqual(['Rare']);
  });

  it('is empty for a line with no stations', () => {
    const diagram = buildLineDiagram({ directions: [], branches: [] });
    expect(diagram.stops).toEqual([]);
    expect(diagram.from).toBeNull();
  });
});
