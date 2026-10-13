'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * The confirm step is `ConfirmActionButton`'s.
 *
 * `delete_line` requires `AuthenticatedUser` (`crates/api/src/routes/lines.rs`),
 * and `/lines/[id]/page.tsx` now only renders this button for the line's
 * real owner (see that page's `isOwner` gate). So a `401` here can, in
 * practice, only happen from a session that lapses between page load and
 * this click — the same narrow race `TicketPanel`'s design already
 * reasoned about (Decision 4,
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md).
 * Matches `PinToggle`'s established `needsLogin` pattern: catch the `401`
 * specifically and show a login prompt, never the raw backend rejection
 * text this used to fall through to.
 *
 * Used on the line page's heading row and in `CustomLineForm`'s edit-mode
 * footer (Task 3.4.11), with the same trigger in both: the app has one
 * confirm-trigger style. */
export function DeleteLineButton({ id }: { id: string }) {
  return (
    <ConfirmActionButton
      label="Delete line"
      confirmLabel="Delete"
      actionName="delete line"
      title="Delete this line?"
      request={() => fetch(`/api/lines/${id}`, { method: 'DELETE' })}
      failure={['delete', 'this line']}
      loginPrompt="Log in to delete a line"
      onSuccess={(router) => router.push('/lines')}
    >
      This cannot be undone.
    </ConfirmActionButton>
  );
}
