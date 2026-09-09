import { RenameTrainButton } from './RenameTrainButton';
import { DeleteTrainButton } from './DeleteTrainButton';
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
 * every field this needs (`id`, `customName`, and the
 * `pin*`/`serviceDate` fields `trackedTrainDisplayName` reads), so no
 * adapter is required for either caller. */
export function TrackedTrainOwnerControls({
  train,
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
  };
}) {
  return (
    <>
      <RenameTrainButton
        trackingId={train.id}
        customName={train.customName}
        defaultName={trackedTrainDisplayName(train)}
      />
      <DeleteTrainButton trackingId={train.id} />
    </>
  );
}
