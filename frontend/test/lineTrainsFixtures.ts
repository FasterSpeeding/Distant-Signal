/** Shared fixtures for the line page's "Trains on this line" tests. */
import type { LineCatalogueStation, LineTrainSummary } from '@/lib/types';

export const STATIONS: LineCatalogueStation[] = [
  { crs: 'WAT', name: 'London Waterloo', role: 'terminus' },
  { crs: 'CLJ', name: 'Clapham Junction', role: 'junction' },
  { crs: 'WOK', name: 'Woking', role: 'junction' },
  { crs: 'BSK', name: 'Basingstoke', role: 'junction' },
  { crs: 'WIN', name: 'Winchester', role: 'major' },
  { crs: 'SOA', name: 'Southampton Airport Parkway', role: 'minor' },
  { crs: 'SOU', name: 'Southampton Central', role: 'major' },
  { crs: 'WEY', name: 'Weymouth', role: 'terminus' },
];

export function train(overrides: Partial<LineTrainSummary> & { uid: string }): LineTrainSummary {
  return {
    operator: 'SW',
    serviceMode: 'train',
    scope: 'line',
    direction: 'down',
    lineDue: { time: '08:00', dayOffset: 0 },
    origin: { crs: 'WAT', name: 'London Waterloo' },
    destination: { crs: 'WEY', name: 'Weymouth' },
    onLineStops: [
      { crs: 'WAT', time: '08:00', dayOffset: 0 },
      { crs: 'WOK', time: '08:25', dayOffset: 0 },
      { crs: 'WEY', time: '10:30', dayOffset: 0 },
    ],
    live: null,
    ...overrides,
  };
}
