'use client';

import { ConfirmActionButton } from './ConfirmActionButton';

/** Deletes a journey template the caller owns
 * (`DELETE /JourneyTemplates/{id}`). Every journey this template ever
 * produced (`journeys.source_template_id`) survives untouched — see
 * `crates/api/src/data/journey_templates.rs::delete_template`'s own doc
 * comment (`ON DELETE SET NULL`, not a cascade) — so the confirm copy
 * below says so explicitly, unlike `RemoveJourneyLegButton`'s "this
 * cannot be undone" alone, since a caller might otherwise reasonably fear
 * losing journeys they've already run from this template. Always
 * redirects to `/journeys/templates` on success (mirrors
 * `RemoveJourneyLegButton`'s `afterDelete`-style "closest surviving list"
 * target for a delete that removes the CURRENT page's own object) rather
 * than a bare `router.refresh()`, since this component is used both on
 * the list (Task 7) and the detail page (Task 8) and the detail page
 * itself is gone after a successful delete. */
export function DeleteJourneyTemplateButton({ templateId }: { templateId: number }) {
  return (
    <ConfirmActionButton
      label="Delete template"
      actionName="delete template"
      title="Delete this template?"
      request={() => fetch(`/api/JourneyTemplates/${templateId}`, { method: 'DELETE' })}
      failure={['delete', 'this template']}
      loginPrompt="Log in to delete this template"
      onSuccess={(router) => router.push('/journeys/templates')}
    >
      This cannot be undone. Only the reusable template is deleted, not any journeys you&apos;ve already run from it.
    </ConfirmActionButton>
  );
}
