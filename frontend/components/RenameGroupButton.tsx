'use client';

import { useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Modal, Stack, TextInput } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** `admin`/`owner`-only rename control for `PUT /groups/{id}` (spec §3:
 * "Rename the group | `admin` or `owner`"). A button that opens a modal
 * holding the same form shape `CreateGroupForm.tsx` uses -- same
 * `maxLength`, same trim-and-require-non-empty submit gate, same
 * "show the backend's own message verbatim" error `Alert` -- rather than
 * an always-visible inline input, so the group header stays a header.
 *
 * The 100-character `maxLength` mirrors the backend's
 * `MAX_GROUP_NAME_LENGTH` (`crates/api/src/routes/groups.rs`) exactly;
 * `validate_group_name` there is still the real authority, and its
 * rejection message is written for verbatim display, so an over-long name
 * that somehow got past this input surfaces the server's own copy.
 *
 * Unlike `CreateGroupForm` (which navigates to the brand-new group), this
 * stays put and `router.refresh()`es -- the page it's on is already the
 * right page, it just needs the new name. Uses `LoginLink` rather than
 * `CreateGroupForm`'s `LoginPromptModal` for the 401 case: a modal on top
 * of this component's own modal would be a modal stack, so the prompt is
 * rendered inline alongside the error, matching `RemoveMemberButton` and
 * every other confirm-modal button in this feature. */
export function RenameGroupButton({ groupId, currentName }: { groupId: string; currentName: string }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [name, setName] = useState(currentName);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  /** Reopening after a failed or abandoned attempt starts clean, from the
   * name the group actually has right now -- not from whatever half-edited
   * string was left in the input last time. */
  function handleOpen() {
    setName(currentName);
    setError(null);
    needsLoginState.reset();
    open();
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}`, {
        method: 'PUT',
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
      setSubmitting(false);
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Rename
      </Button>
      <Modal opened={opened} onClose={close} title="Rename this group">
        <Stack gap="md" component="form" onSubmit={handleSubmit}>
          <TextInput
            label="Group name"
            value={name}
            onChange={(event) => setName(event.currentTarget.value)}
            maxLength={100}
            required
            data-autofocus
          />
          {error && (
            <Alert color="red" title="Couldn't rename this group">
              {error}
            </Alert>
          )}
          {needsLoginState.needsLogin && <LoginLink underline="always">Log in to rename this group</LoginLink>}
          <Group justify="end">
            <Button variant="default" onClick={close} disabled={submitting}>
              Cancel
            </Button>
            <Button type="submit" disabled={name.trim().length === 0 || submitting} aria-label="Confirm rename group">
              {submitting ? 'Saving…' : 'Save'}
            </Button>
          </Group>
        </Stack>
      </Modal>
    </>
  );
}
