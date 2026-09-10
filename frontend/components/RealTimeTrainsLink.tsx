import { TextLink } from './TextLink';

/** Builds the URL of this train's service page on Real Time Trains
 * (https://www.realtimetrains.co.uk/), a well-known third-party UK
 * train-tracking site with more granular, signalling-level detail than
 * this app surfaces.
 *
 * Confirmed against two independent sources, not assumed:
 *   - RTT's own published JSON API spec
 *     (https://realtimetrains.github.io/api-specification/specification/main.yml)
 *     documents a service's `uniqueIdentity` as `{identity}:{departureDate}`
 *     under the `gb-nr` namespace, e.g. `gb-nr:L01525:2025-10-26` --
 *     confirming both the `gb-nr` namespace prefix and that a service is
 *     keyed by (CIF/TOPS UID, service date) together, date as `YYYY-MM-DD`.
 *   - Real service links shared on RailUK Forums' "RealTimeTrains website"
 *     thread use the website's own path form of that same identity, e.g.
 *     `.../service/gb-nr:L58365/2021-05-24/detailed` -- confirming the
 *     website (as opposed to the JSON API) separates the UID from the date
 *     with `/` rather than `:`, and appends a `/detailed` view suffix.
 * `/detailed` is RTT's full stop-by-stop view (as opposed to its plainer
 * "simple" view) -- the natural target here, since the whole point of this
 * link is to hand a visitor MORE detail than this app itself shows.
 *
 * `trainUid` is `encodeURIComponent`-escaped defensively, matching
 * `getPublicTrainByUidAndDate` (`lib/api.ts`)'s own treatment of the same
 * value when building ITS request URL -- CIF UIDs are plain alphanumeric in
 * practice, but nothing at the type level guarantees that. The `gb-nr:`
 * prefix and the `/` separators are left un-encoded: RTT's own URLs use
 * both literally, and neither is a character `encodeURIComponent` would
 * touch anyway. `serviceDate` is not encoded -- every call site already
 * validates it as `YYYY-MM-DD` before it reaches here (this page's own
 * `DATE_PATTERN` check), so it never contains a character that would need
 * it. */
export function realTimeTrainsUrl(trainUid: string, serviceDate: string): string {
  return `https://www.realtimetrains.co.uk/service/gb-nr:${encodeURIComponent(trainUid)}/${serviceDate}/detailed`;
}

/** External cross-reference link to this train's Real Time Trains service
 * page. Renders nothing until `trainUid` is genuinely known -- a train
 * that hasn't yet resolved to a real CIF UID (`resolutionStatus ===
 * 'pending'`, or the `'unresolved'` give-up state) has nothing on RTT to
 * link to. Mirrors `TrainJourney.tsx`'s own `trainUid`-nullability gating
 * rather than inventing a separate rule for this one link. */
export function RealTimeTrainsLink({
  trainUid,
  serviceDate,
}: {
  trainUid: string | null;
  serviceDate: string;
}) {
  if (!trainUid) {
    return null;
  }

  return (
    <TextLink
      href={realTimeTrainsUrl(trainUid, serviceDate)}
      underline="always"
      target="_blank"
      rel="noopener noreferrer"
    >
      View on Real Time Trains ↗
    </TextLink>
  );
}
