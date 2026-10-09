import { describe, expect, it } from 'vitest';
import { trainIdentifiers, trainName } from './trainName';

describe('trainName', () => {
  it('names a train by its departure time (London) and route', () => {
    expect(trainName({ departure: '2026-10-09T07:42:00Z', origin: 'Woking', destination: 'London Waterloo' })).toBe(
      '08:42 Woking to London Waterloo',
    );
  });

  it('drops the time when unknown, and gives up without both ends', () => {
    expect(trainName({ departure: null, origin: 'Woking', destination: 'London Waterloo' })).toBe(
      'Woking to London Waterloo',
    );
    expect(trainName({ departure: '2026-10-09T07:42:00Z', origin: 'Woking', destination: null })).toBeNull();
  });
});

describe('trainIdentifiers', () => {
  it('labels the headcode and UID', () => {
    expect(trainIdentifiers({ headcode: '1S00', uid: 'W12345' })).toBe('Headcode 1S00 · UID W12345');
    expect(trainIdentifiers({ headcode: null, uid: 'W12345' })).toBe('UID W12345');
    expect(trainIdentifiers({ headcode: null, uid: null })).toBeNull();
  });
});
