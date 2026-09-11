'use client';

import { useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Stack, TextInput } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

/** `/groups/new`'s form -- creates a group, then immediately rotates its
 * first invite link (spec §6: "on success, immediately generate the
 * group's first invite link") before navigating to the new group's detail
 * page, where `GroupInviteLinkCard` (Task 12) displays it. Both calls go
 * through the same-origin `/api/*` proxy -- this is a Client Component and
 * can't read the server-only `API_BASE_URL` env var, same reasoning as
 * `TrackTrainForm`/`DeleteTrainButton`.
 *
 * The invite-link call is best-effort: the group itself has already been
 * created by the time it runs, so its failure must never block navigating
 * to the new group -- its own detail page's "Regenerate" control (visible
 * to the owner, Task 12) can create one later if this fails. */
export function CreateGroupForm() {
  const router = useRouter();
  const [name, setName] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch('/api/groups', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      const created: { id: string } = await response.json();
      try {
        await fetch(`/api/groups/${created.id}/invite-link`, { method: 'POST' });
      } catch {
        // Best-effort -- see this component's own doc comment.
      }
      router.push(`/groups/${created.id}`);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <TextInput
        label="Group name"
        placeholder="e.g. Family or Commute crew"
        value={name}
        onChange={(event) => setName(event.currentTarget.value)}
        maxLength={100}
        required
        data-autofocus
      />
      {error && (
        <Alert color="red" title="Couldn't create this group">
          {error}
        </Alert>
      )}
      <Button type="submit" disabled={name.trim().length === 0 || submitting}>
        {submitting ? 'Creating…' : 'Create group'}
      </Button>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to create a group.
      </LoginPromptModal>
    </Stack>
  );
}
