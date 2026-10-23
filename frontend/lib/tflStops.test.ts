import { describe, expect, it } from 'vitest';
import {
  arrivalDue,
  arrivalDueSpoken,
  journeyFromTflStopHref,
  TFL_STOP_ID,
  tflLineHref,
  tflModeBadge,
  tflStopHref,
} from './tflStops';

describe('tflStops', () => {
  it('accepts NaPTAN IDs only', () => {
    expect(TFL_STOP_ID.test('940GZZLUUXB')).toBe(true);
    expect(TFL_STOP_ID.test('940gzzluuxb')).toBe(false);
    expect(TFL_STOP_ID.test('../metrics')).toBe(false);
    expect(TFL_STOP_ID.test('ABC')).toBe(false);
  });

  it('builds the stop, line and new-journey links', () => {
    expect(tflStopHref('940GZZLUUXB')).toBe('/tfl/stops/940GZZLUUXB');
    expect(tflLineHref('piccadilly')).toBe('/lines/tfl-piccadilly');
    expect(journeyFromTflStopHref('940GZZLUUXB')).toBe('/journeys/new?fromTflStop=940GZZLUUXB');
  });

  it('names modes as badges say them', () => {
    expect(tflModeBadge('tube')).toBe('Underground');
    expect(tflModeBadge('dlr')).toBe('DLR');
    expect(tflModeBadge('tram')).toBe('Tram');
    expect(tflModeBadge('cable-car')).toBe('cable-car');
  });

  it('says how long until a train arrives in whole minutes, "Due" inside one', () => {
    expect(arrivalDue(0)).toBe('Due');
    expect(arrivalDue(59)).toBe('Due');
    expect(arrivalDue(60)).toBe('1 min');
    expect(arrivalDue(149)).toBe('2 min');
    expect(arrivalDue(-5)).toBe('Due');
    expect(arrivalDueSpoken(30)).toBe('due now');
    expect(arrivalDueSpoken(61)).toBe('in 1 minute');
    expect(arrivalDueSpoken(600)).toBe('in 10 minutes');
  });
});
