'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * -- this is a Client Component and cannot reach the `api` service directly.
 * `/api/Train/tickets/{ticketId}` is passed straight through to the
 * backend's `DELETE /Train/tickets/{ticketId}`
 * (`crates/api/src/routes/train.rs::delete_ticket`) with no `/public/`
 * prefix inserted -- see that proxy's own `resolveTargetPath` comment.
 * The confirm step, the 401 login prompt and the failure copy are
 * `ConfirmActionButton`'s. Unlike `DeleteTrainButton` it keeps the default
 * `router.refresh()` on success, never `router.push(...)`.
 *
 * This is a deliberate divergence from `DeleteTrainButton`: deleting a
 * tracked train removes the entire subject of the page it's rendered on,
 * so navigating away is correct there. Deleting a *ticket* always happens
 * from inside a list of other things (a train's other tickets, or
 * `/journeys`'s other trains/tickets) that remain valid and worth
 * showing afterwards -- `router.refresh()` is the same mechanism
 * `AttachTicketAction.tsx:41` and `PinToggle.tsx:99` already use for this
 * "mutate one row, stay on this page" shape. It re-runs the enclosing
 * Server Component (`TicketPanel`, or `app/journeys/page.tsx`), which
 * naturally drops the now-deleted ticket from its next render -- no
 * client-side list-splicing logic needed here.
 *
 * A `401` here can only really happen from a session that lapses between
 * page load and this click (every call site only ever renders this button
 * for a ticket the enclosing Server Component just fetched) -- same narrow
 * race `DeleteTrainButton` already reasons about. A `404` (a
 * double-click/stale-render race) is not treated as a distinguishable case
 * either, same posture.
 *
 * Known, accepted trade-off shared with every other per-row action in this
 * app (e.g. `PinToggle` on a list page): this component is rendered once
 * per ticket, each instance with its own independent `opened`/`deleting`
 * state, so nothing prevents a caller from having two different tickets'
 * confirm modals open at once. Not a new risk this component introduces --
 * no per-row action in this codebase guards against that today. */
export function DeleteTicketButton({ ticketId }: { ticketId: number }) {
  return (
    <ConfirmActionButton
      label="Delete"
      actionName="delete ticket"
      title="Delete this ticket?"
      request={() => fetch(`/api/Train/tickets/${ticketId}`, { method: 'DELETE' })}
      failure={['delete', 'this ticket']}
      loginPrompt="Log in to delete this ticket"
    >
      This cannot be undone.
    </ConfirmActionButton>
  );
}
