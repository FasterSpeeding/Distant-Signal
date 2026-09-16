'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Select, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import type { LineSummary } from '@/lib/types';

/** Picker sourced from the caller's OWN custom lines, mirroring
 * `AddTrainToGroupButton`'s "picker sourced from the caller's own
 * resources" shape exactly.
 *
 * `/api/lines` (`GET /public/lines`) is already caller-scoped for custom
 * entries -- `list_custom_lines_for_user` returns only the caller's own,
 * and custom-line group sharing deliberately did NOT widen it -- so
 * filtering to `source === 'custom'` here yields exactly "lines I own".
 * That filter is belt-and-braces on top of the real boundary, which is
 * server-side: `groups::grant_custom_line` re-checks
 * `WHERE id = $1 AND user_id = $2` and 404s anything else. A group's
 * `admin`/`owner` has no standing to share a member's private line, and no
 * amount of client-side tampering here changes that.
 *
 * `excludeLineIds` hides lines already shared into this group -- re-sharing
 * one is harmless (the grant is idempotent) but offering it again would be
 * confusing. */
export function AddCustomLineToGroupButton({
  groupId,
  excludeLineIds,
}: {
  groupId: string;
  excludeLineIds: string[];
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [loading, setLoading] = useState(false);
  const [lines, setLines] = useState<LineSummary[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleOpen() {
    setError(null);
    setSelected(null);
    open();
    setLoading(true);
    try {
      const response = await fetch('/api/lines');
      if (!response.ok) {
        setError('Could not load your custom lines.');
        setLoading(false);
        return;
      }
      const all: LineSummary[] = await response.json();
      setLines(all.filter((l) => l.source === 'custom' && !excludeLineIds.includes(l.id)));
      setLoading(false);
    } catch {
      setError('Could not load your custom lines.');
      setLoading(false);
    }
  }

  async function handleAdd() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/lines/custom`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ lineId: selected }),
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
        Share one of my custom lines
      </Button>
      <Modal opened={opened} onClose={close} title="Share a custom line with this group">
        <Text size="sm" c="dimmed" mb="sm">
          Everyone in this group will be able to see this line&apos;s stations, operators and live
          status. Only you can edit or delete it, and you can stop sharing it at any time.
        </Text>
        {loading && <Text c="dimmed">Loading your custom lines…</Text>}
        {!loading && lines !== null && lines.length === 0 && (
          <Text c="dimmed">
            You have no custom lines left to share with this group.
          </Text>
        )}
        {!loading && lines !== null && lines.length > 0 && (
          <Select
            label="Custom line"
            placeholder="Pick one"
            data={lines.map((l) => ({ value: l.id, label: l.name }))}
            value={selected}
            onChange={setSelected}
          />
        )}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to share a line</LoginLink>
        )}
        <Button mt="md" onClick={handleAdd} disabled={!selected} loading={submitting}>
          Share with group
        </Button>
      </Modal>
    </>
  );
}
