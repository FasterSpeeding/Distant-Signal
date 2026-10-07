import type { ServiceMode, ServiceModeFields } from './types';

/** What a schedule's vehicle is, for the passenger. The CIF timetable
 * carries buses and ferries alongside trains (9% of weekday schedules, a
 * quarter on Sundays), and Network Rail's live feed never reports any of
 * them -- so the backend marks them `liveTracking: false` and the UI shows
 * them as timetable-only rather than as a train forever "waiting for a
 * movement report".
 *
 * Each non-train mode has its own label (and its own icon, in
 * `components/ServiceModeIcon.tsx`): a rail replacement bus is standing in
 * for a train that isn't running, a bus service is a timetabled bus that
 * always runs, and a ferry is a ferry. */
export const SERVICE_MODE_LABELS: Record<Exclude<ServiceMode, 'train'>, string> = {
  replacementBus: 'Rail replacement bus',
  bus: 'Bus service',
  ferry: 'Ferry',
};

/** The copy shown wherever live progress would be. */
export const TIMETABLE_ONLY_MESSAGE = "Timetabled only — buses and ferries aren't tracked live";

/** `null` for a train, or when the backend predates `serviceMode`. */
export function serviceModeLabel(mode: ServiceMode | null | undefined): string | null {
  if (!mode || mode === 'train') {
    return null;
  }
  // `mode` is a wire value: a newer api can send one this bundle has no
  // label for yet.
  const labels: Partial<Record<string, string>> = SERVICE_MODE_LABELS;
  return labels[mode] ?? null;
}

/** A bus or ferry: no live position, delay or arrival will ever arrive.
 * `false` when the fields are absent (an older backend) -- a train. */
export function isTimetableOnly(fields: ServiceModeFields | null | undefined): boolean {
  if (!fields) {
    return false;
  }
  if (fields.liveTracking === false) {
    return true;
  }
  return fields.serviceMode !== undefined && fields.serviceMode !== null && fields.serviceMode !== 'train';
}

/** The noun for a service in running text ("Bus C30818", "Train 1A23"). */
export function serviceNoun(mode: ServiceMode | null | undefined): string {
  switch (mode) {
    case 'replacementBus':
    case 'bus':
      return 'Bus';
    case 'ferry':
      return 'Ferry';
    default:
      return 'Train';
  }
}
