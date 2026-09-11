import { RenameTrainButton } from './RenameTrainButton';
import { DeleteTrainButton } from './DeleteTrainButton';
import { AddToGroupButton } from './AddToGroupButton';
import { trackedTrainDisplayName } from '@/lib/trackingName';

/** The Rename/Delete button pair shown wherever a visitor is looking at a
 * tracked train they own -- shared here rather than reimplemented on both
 * `/train/by-id/[trackingId]` (which starts from ownership already, by
 * construction) and `/train/[uid]/[date]` (once it identifies the visitor
 * as the owner via a `GET /Train/mine` match). Both used to independently
 * wire up the identical `trackingId`/`customName`/`defaultName` prop triple
 * for `RenameTrainButton`, plus `DeleteTrainButton`'s own `trackingId`.
 *
 * Deliberately does NOT also bundle `TicketPanel`, even though both pages
 * render one alongside these buttons: `TicketPanel` is already a single,
 * independently reusable Server Component (no props beyond `trackingId`,
 * so nothing to extract there), and the two pages render it in different
 * visual positions relative to `TrainJourney` (immediately after these
 * buttons on `/train/by-id/[trackingId]`, after `TrainJourney` on
 * `/train/[uid]/[date]`). Folding it in here would mean threading
 * `TrainJourney` through as `children` for no real benefit over each
 * page's own one-line `<TicketPanel trackingId={...} />` call.
 *
 * `train` accepts either a full `TrackedTrainState` or the lighter
 * `GET /Train/mine` `TrackedTrainListItem` (`lib/types.ts`) -- both carry
 * every field this needs (`id`, `customName`, `sharedGroupCount`, and the
 * `pin*`/`serviceDate` fields `trackedTrainDisplayName` reads), so no
 * adapter is required for either caller.
 *
 * `afterDelete` passes straight through to `DeleteTrainButton` -- see that
 * component's own doc comment for why it's a plain optional prop (default
 * `'redirect'`, matching `/train/by-id/[trackingId]`'s existing behaviour)
 * rather than a callback: this wrapper is itself rendered directly from a
 * Server Component page on both call sites, which cannot hand a Client
 * Component a function prop. `/train/[uid]/[date]/page.tsx` is the one
 * caller that passes `'refresh'`, since that page's own URL stays valid
 * after the tracked train is deleted.
 *
 * `AddToGroupButton` is the third control, alongside Rename/Delete -- lets
 * the owner share this ALREADY-tracked train into one of their groups after
 * the fact (including a second one), distinct from `TrackDestinationModal`'s
 * track-TIME Personal-vs-group picker, which only ever offers that choice
 * once, at the moment a train is first tracked. It renders nothing itself
 * when the viewer is in zero groups (see its own doc comment), so it adds no
 * visible clutter for the common case. */
export function TrackedTrainOwnerControls({
  train,
  afterDelete,
}: {
  train: {
    id: number;
    customName: string | null;
    pinOriginCrs: string | null;
    pinOriginName: string | null;
    pinDestinationCrs: string | null;
    pinDestinationName: string | null;
    serviceDate: string;
    pinScheduledDeparture?: string | null;
    sharedGroupCount: number;
  };
  afterDelete?: 'redirect' | 'refresh';
}) {
  return (
    <>
      <RenameTrainButton
        trackingId={train.id}
        customName={train.customName}
        defaultName={trackedTrainDisplayName(train)}
      />
      <AddToGroupButton trainSubscriptionId={train.id} />
      <DeleteTrainButton trackingId={train.id} sharedGroupCount={train.sharedGroupCount} afterDelete={afterDelete} />
    </>
  );
}
