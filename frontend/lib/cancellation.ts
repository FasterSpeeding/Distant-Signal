/**
 * Words for a cancelled or part-cancelled train (DS-181), shared by the
 * train page, journey legs, the journey summary and page descriptions, so
 * every place says the same thing about the same train.
 *
 * From the api's `cancellation` (`crates/api/src/data/cancellation.rs`):
 *
 * - wholly cancelled: "Cancelled";
 * - terminated short: "Terminates at Raynes Park";
 * - started short: "Starts from Clapham Junction";
 * - both: "Starts from Woking and terminates at Basingstoke";
 * - one section in the middle: "Cancelled between Milton Keynes Central
 *   and London Euston";
 * - several sections: "Part cancelled", with each section listed.
 *
 * With a reason, the one-line form adds it after a colon: "Cancelled: a
 * fault with the signalling system". A board sentence's "This train has
 * been cancelled because of" is dropped, as the headline already says it.
 */
import type { Cancellation, CancellationPlace, CancellationReason, JourneyStatus, ListCancellation } from './types';

/** The fields of a train state this module reads. */
export interface CancellationFields {
  status: JourneyStatus | null;
  cancellation?: Cancellation | null;
  cancelReasonCode?: string | null;
  cancelReason?: string | null;
}

/** The train's cancellation: the api's own, or, from an older backend (or
 * a cancelled status the api could not place on the route), the whole
 * train with the older reason fields. `null` when nothing is cancelled. */
export function trainCancellation(state: CancellationFields): Cancellation | null {
  if (state.cancellation) return state.cancellation;
  if (state.status !== 'cancelled') return null;
  const code = state.cancelReasonCode ?? null;
  const text = state.cancelReason ?? null;
  return {
    kind: 'full',
    startsFrom: null,
    terminatesAt: null,
    from: null,
    to: null,
    sections: [],
    reason: code !== null || text !== null ? { code, text, source: 'trust' } : null,
    at: null,
  };
}

/** A call's name, else its code. */
export function cancellationStopName(stop: CancellationPlace): string {
  return stop.name ?? stop.crs ?? 'an unnamed stop';
}

/** The headline: "Cancelled", "Terminates at X", "Starts from X", "Starts
 * from X and terminates at Y", "Cancelled between X and Y" or "Part
 * cancelled". */
export function cancellationHeadline(cancellation: Cancellation): string {
  if (cancellation.kind === 'full') return 'Cancelled';
  const { startsFrom, terminatesAt, sections } = cancellation;
  if (startsFrom && terminatesAt && sections.length === 2) {
    return shortRunHeadline(startsFrom, terminatesAt) ?? 'Part cancelled';
  }
  if (sections.length === 1) {
    if (terminatesAt) return shortRunHeadline(null, terminatesAt) ?? 'Part cancelled';
    if (startsFrom) return shortRunHeadline(startsFrom, null) ?? 'Part cancelled';
    const [only] = sections;
    if (only) return sectionLine(only.from, only.to);
  }
  return 'Part cancelled';
}

/** "Starts from X and terminates at Y", "Terminates at Y", "Starts from
 * X", or `null` when the train runs from end to end. */
function shortRunHeadline(startsFrom: CancellationPlace | null, terminatesAt: CancellationPlace | null): string | null {
  if (startsFrom && terminatesAt) {
    return `Starts from ${cancellationStopName(startsFrom)} and terminates at ${cancellationStopName(terminatesAt)}`;
  }
  if (terminatesAt) return `Terminates at ${cancellationStopName(terminatesAt)}`;
  if (startsFrom) return `Starts from ${cancellationStopName(startsFrom)}`;
  return null;
}

function sectionLine(from: CancellationPlace, to: CancellationPlace): string {
  return `Cancelled between ${cancellationStopName(from)} and ${cancellationStopName(to)}`;
}

/** One "Cancelled between X and Y" per section, for the detail under a
 * headline. Empty for a wholly cancelled train, whose headline says it
 * all. */
export function cancelledSectionLines(cancellation: Cancellation): string[] {
  if (cancellation.kind === 'full') return [];
  return cancellation.sections.map((section) => sectionLine(section.from, section.to));
}

const BOARD_PREFIX = /^this (?:train|service) (?:has been|is being|is|was) cancelled (?:because of|due to) /i;

/** The reason as a phrase to follow "Cancelled:": "a fault with the
 * signalling system", "failure to maintain vegetation ...". `null` when
 * there is no text (a system code, or no reason at all). */
export function cancellationReasonPhrase(reason: CancellationReason | null): string | null {
  const text = reason?.text?.trim();
  if (!text) return null;
  const phrase = text.replace(BOARD_PREFIX, '').replace(/\.$/, '');
  if (phrase === '') return null;
  // Lower-case the first letter of a sentence-case description ("Points
  // failure" to "points failure"), but not of an acronym ("TOC ...") or a
  // name.
  const [first = '', second = ''] = phrase;
  return second !== '' && second === second.toLowerCase() ? first.toLowerCase() + phrase.slice(1) : phrase;
}

/** Where the reason came from, said plainly, or `null` without one. */
export function cancellationReasonSource(reason: CancellationReason | null): string | null {
  if (cancellationReasonPhrase(reason) === null || reason === null) return null;
  return reason.source === 'trust' ? 'Reason recorded by Network Rail' : 'Reason from National Rail Enquiries';
}

/** The one-line form: "Cancelled: a fault with the signalling system",
 * "Terminates at Raynes Park", ... */
export function cancellationSummary(cancellation: Cancellation): string {
  const reason = cancellationReasonPhrase(cancellation.reason);
  const headline = cancellationHeadline(cancellation);
  return reason ? `${headline}: ${reason}` : headline;
}

/** A list row's headline (`ListCancellation`, which has no sections):
 * "Cancelled", "Terminates at X", "Starts from X", "Starts from X and
 * terminates at Y", or "Part cancelled" for a cancelled middle section. */
export function listCancellationHeadline(cancellation: ListCancellation): string {
  if (cancellation.kind === 'full') return 'Cancelled';
  return shortRunHeadline(cancellation.startsFrom, cancellation.terminatesAt) ?? 'Part cancelled';
}

/** The reason on its own line, under a list row's badge: the phrase
 * (`cancellationReasonPhrase`) with a capital first letter ("Failure to
 * maintain vegetation"). `null` without reason text. */
export function cancellationReasonLine(reason: CancellationReason | null): string | null {
  const phrase = cancellationReasonPhrase(reason);
  return phrase === null ? null : phrase.charAt(0).toUpperCase() + phrase.slice(1);
}

/** What a screen reader hears for a list row's cancellation, as whole
 * sentences: "This train terminates at York. Reason: Failure to maintain
 * vegetation." */
export function listCancellationSentence(cancellation: ListCancellation): string {
  let what: string;
  if (cancellation.kind === 'full') what = 'is cancelled';
  else {
    const headline = shortRunHeadline(cancellation.startsFrom, cancellation.terminatesAt);
    what = headline === null ? 'is part cancelled' : headline.charAt(0).toLowerCase() + headline.slice(1);
  }
  const reason = cancellationReasonLine(cancellation.reason);
  return reason === null ? `This train ${what}.` : `This train ${what}. Reason: ${reason}.`;
}

/** The fields of a live departure-board row `boardCancellationNote` reads
 * (`DepartureRow`). */
export interface BoardCancellationFields {
  isCancelled: boolean;
  cancelReason: string | null;
  callingPoints?: readonly { locationName: string | null; crs: string; isCancelled: boolean }[] | undefined;
}

/** One line for a departure-board row, from Darwin's own board data: "a
 * fault with the signalling system" under a cancelled row's "Cancelled"
 * badge, or "Terminates at Woking" / "Cancelled between Woking and
 * Basingstoke" (with the reason) for a train that still leaves but no
 * longer calls everywhere after. `null` when there is nothing to add. */
export function boardCancellationNote(row: BoardCancellationFields): string | null {
  const reason = cancellationReasonPhrase(
    row.cancelReason ? { code: null, text: row.cancelReason, source: 'ldbws' } : null,
  );
  if (row.isCancelled) return reason ? `Cancelled: ${reason}` : null;
  const points = row.callingPoints ?? [];
  const first = points.findIndex((point) => point.isCancelled);
  if (first === -1) return null;
  let last = first;
  while (points[last + 1]?.isCancelled) last += 1;
  const name = (point: { locationName: string | null; crs: string }) => point.locationName ?? point.crs;
  const before = points[first - 1];
  const after = points[last + 1];
  const several = points.slice(last + 1).some((point) => point.isCancelled);
  let headline: string;
  if (several || (after === undefined && before === undefined)) headline = 'Some stops cancelled';
  else if (after === undefined && before) headline = `Terminates at ${name(before)}`;
  else if (before && after) headline = `Cancelled between ${name(before)} and ${name(after)}`;
  else
    headline = `Does not call at ${points
      .slice(0, last + 1)
      .map(name)
      .join(', ')}`;
  return reason ? `${headline}: ${reason}` : headline;
}
