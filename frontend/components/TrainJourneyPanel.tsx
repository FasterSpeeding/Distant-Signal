import { TrainJourney } from './TrainJourney';
import { RealTimeTrainsLink } from './RealTimeTrainsLink';
import type { TrainJourneyState } from '@/lib/types';

/** `TrainJourney` plus its Real Time Trains cross-reference link, bundled
 * together because the two are driven by exactly the same `state` and have
 * always been meant to appear side by side (per `RealTimeTrainsLink`'s own
 * `trainUid`-nullability gating, which mirrors `TrainJourney.tsx`'s own).
 *
 * Extracted after a consolidation pass found `app/train/by-id/[trackingId]`
 * had drifted from `app/train/[uid]/[date]`: the public page grew this link
 * (once `RealTimeTrainsLink` landed) while by-id's own separately-maintained
 * render path didn't, even though a `schedule_matched` tracked train already
 * has a real `trainUid` the link needs -- see this component's own tests
 * and `app/train/by-id/[trackingId]/page.test.tsx`'s "consolidation gap"
 * comment. Both pages now render this ONE component instead of the two
 * calls separately, so a future addition alongside `TrainJourney` (or a
 * future change to this pairing) only has to happen once. */
export function TrainJourneyPanel({ state }: { state: TrainJourneyState }) {
  return (
    <>
      <TrainJourney state={state} />
      <RealTimeTrainsLink trainUid={state.trainUid} serviceDate={state.serviceDate} />
    </>
  );
}
