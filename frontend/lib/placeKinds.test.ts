import { describe, expect, it } from 'vitest';
import {
  labelPlaceEnds,
  placeNamesAlike,
  RAIL_PLACE,
  stopModesFromId,
  stopModesLabel,
  stopPlaceKind,
  type PlaceKind,
} from './placeKinds';

const stop = (...modes: string[]): PlaceKind => ({ kind: 'stop', modes });

describe('labelPlaceEnds (DS-239)', () => {
  it('says which end is which when the names are the same', () => {
    expect(labelPlaceEnds({ name: 'Vauxhall', kind: RAIL_PLACE }, { name: 'Vauxhall', kind: stop('tube') })).toEqual([
      'Vauxhall (National Rail)',
      'Vauxhall (Tube)',
    ]);
  });

  it('reads "London Euston" and "Euston" as alike', () => {
    expect(labelPlaceEnds({ name: 'London Euston', kind: RAIL_PLACE }, { name: 'Euston', kind: stop('tube') })).toEqual(
      ['London Euston (National Rail)', 'Euston (Tube)'],
    );
  });

  it('tags only the stop when a mixed walk has different names', () => {
    expect(
      labelPlaceEnds(
        { name: 'London Kings Cross', kind: RAIL_PLACE },
        { name: "King's Cross St. Pancras", kind: stop('tube') },
      ),
    ).toEqual(['London Kings Cross', "King's Cross St. Pancras (Tube)"]);
  });

  it('calls a DLR-only stop DLR, never the Tube', () => {
    expect(labelPlaceEnds({ name: 'Stratford', kind: stop('dlr') }, { name: 'Stratford', kind: RAIL_PLACE })).toEqual([
      'Stratford (DLR)',
      'Stratford (National Rail)',
    ]);
  });

  it('tells two TfL stations of one hub apart by mode', () => {
    expect(
      labelPlaceEnds({ name: 'Canning Town', kind: stop('dlr') }, { name: 'Canning Town', kind: stop('tube') }),
    ).toEqual(['Canning Town (DLR)', 'Canning Town (Tube)']);
  });

  it('names an Elizabeth line station', () => {
    expect(
      labelPlaceEnds({ name: 'Paddington', kind: RAIL_PLACE }, { name: 'Paddington', kind: stop('elizabeth-line') }),
    ).toEqual(['Paddington (National Rail)', 'Paddington (Elizabeth line)']);
  });

  it('names an Overground station', () => {
    expect(
      labelPlaceEnds({ name: 'Highbury', kind: stop('overground') }, { name: 'Highbury', kind: stop('tube') }),
    ).toEqual(['Highbury (Overground)', 'Highbury (Tube)']);
  });

  it('names a tram stop', () => {
    expect(labelPlaceEnds({ name: 'Wimbledon', kind: RAIL_PLACE }, { name: 'Wimbledon', kind: stop('tram') })).toEqual([
      'Wimbledon (National Rail)',
      'Wimbledon (tram)',
    ]);
  });

  it('a stop of two modes says both', () => {
    expect(labelPlaceEnds({ name: 'Bank', kind: RAIL_PLACE }, { name: 'Bank', kind: stop('tube', 'dlr') })).toEqual([
      'Bank (National Rail)',
      'Bank (Tube, DLR)',
    ]);
  });

  it('leaves a plain same-type walk as it is', () => {
    expect(
      labelPlaceEnds({ name: 'London Euston', kind: RAIL_PLACE }, { name: 'London Kings Cross', kind: RAIL_PLACE }),
    ).toEqual(['London Euston', 'London Kings Cross']);
    expect(labelPlaceEnds({ name: 'Bank', kind: stop('tube') }, { name: 'Monument', kind: stop('tube') })).toEqual([
      'Bank',
      'Monument',
    ]);
  });

  it('never tags an end of no known kind, nor one already tagged', () => {
    expect(labelPlaceEnds({ name: 'Vauxhall' }, { name: 'Vauxhall', kind: stop('tube') })).toEqual([
      'Vauxhall',
      'Vauxhall (Tube)',
    ]);
    expect(
      labelPlaceEnds({ name: 'Vauxhall (Tube)', kind: stop('tube') }, { name: 'Vauxhall', kind: RAIL_PLACE }),
    ).toEqual(['Vauxhall (Tube)', 'Vauxhall']);
  });

  it('a ride tags only names that read alike', () => {
    const options = { onlyWhenAlike: true };
    expect(
      labelPlaceEnds({ name: 'Euston', kind: stop('tube') }, { name: 'London Kings Cross', kind: RAIL_PLACE }, options),
    ).toEqual(['Euston', 'London Kings Cross']);
  });

  it('can tag every stop, as the planner names them', () => {
    expect(
      labelPlaceEnds(
        { name: 'Uxbridge', kind: stop('tube') },
        { name: 'Harrow-on-the-Hill', kind: stop('tube') },
        {
          alwaysTagStops: true,
        },
      ),
    ).toEqual(['Uxbridge (Tube)', 'Harrow-on-the-Hill (Tube)']);
  });
});

describe('placeNamesAlike', () => {
  it('ignores case, punctuation, "London" and "station"', () => {
    expect(placeNamesAlike('Vauxhall Underground Station', 'Vauxhall')).toBe(true);
    expect(placeNamesAlike("King's Cross", 'Kings Cross')).toBe(true);
    expect(placeNamesAlike('London Euston', 'Euston')).toBe(true);
    expect(placeNamesAlike('Euston Square', 'Euston')).toBe(false);
    expect(placeNamesAlike('', '')).toBe(false);
  });
});

describe('stop kinds', () => {
  it('reads the mode from a stop id', () => {
    expect(stopModesFromId('940GZZLUVXL')).toEqual(['tube']);
    expect(stopModesFromId('940GZZDLSTD')).toEqual(['dlr']);
    expect(stopModesFromId('940GZZCRWMB')).toEqual(['tram']);
    expect(stopModesFromId('9400ZZLUKSX')).toEqual(['tube']);
    expect(stopModesFromId('9400ZZMAPIC')).toEqual(['metrolink']);
    expect(stopModesFromId('VXH')).toBeUndefined();
    expect(stopModesFromId(null)).toBeUndefined();
  });

  it("prefers a stop's own modes to its id's", () => {
    expect(stopPlaceKind({ id: '940GZZLUBNK', modes: ['tube', 'dlr'] })).toEqual(stop('tube', 'dlr'));
    expect(stopPlaceKind({ id: '940GZZDLCGT', modes: [] })).toEqual(stop('dlr'));
    expect(stopPlaceKind({ id: 'nothing' })).toBeUndefined();
  });

  it('words each mode, an unknown one as given', () => {
    expect(stopModesLabel(['tube', 'dlr'])).toBe('Tube, DLR');
    expect(stopModesLabel(['elizabeth-line'])).toBe('Elizabeth line');
    expect(stopModesLabel(['cable-car'])).toBe('cable-car');
    expect(stopModesLabel([])).toBeUndefined();
  });
});
