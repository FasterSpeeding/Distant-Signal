'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, Modal, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

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
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleDelete() {
    setDeleting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/JourneyTemplates/${templateId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      router.push('/journeys/templates');
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Delete template
      </Button>
      <Modal opened={opened} onClose={close} title="Delete this template?">
        <Text>
          This cannot be undone. Any journeys you&apos;ve already run from this template are
          NOT deleted — only the reusable template itself.
        </Text>
        {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to delete this template</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button
            color="red"
            onClick={handleDelete}
            loading={deleting}
            aria-label="Confirm delete template"
          >
            Delete template
          </Button>
        </Group>
      </Modal>
    </>
  );
}
