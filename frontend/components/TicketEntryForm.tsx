'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Badge, Button, FileInput, Group, Stack, Tabs, TextInput } from '@mantine/core';
import { TextLink } from './TextLink';
import type { PartialTicket, TicketEntryRequest, TicketSource } from '@/lib/types';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
type Tab = 'manual' | 'pkpass' | 'pdf';

/** The upload/manual-entry flow for one journey, per
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md
 * Decision 2. Collapsed by default -- `label` (either "Add a ticket for
 * this journey" or "Add another ticket", set by the caller) is the entry
 * point that expands this into the real form, matching Decision 1's
 * "entry point that opens TicketEntryForm" -- the exact collapse mechanism
 * is this plan's own choice, since the spec doesn't detail it further,
 * kept self-contained here so `TicketPanel` (Task 5) stays a plain,
 * server-renderable async function with no interactive state of its own.
 *
 * Three ways to arrive at the same underlying field set and the same final
 * submit: manual entry (default, always available, every field optional),
 * `.pkpass` upload, and PDF upload. Both uploads are read-only PREVIEWS --
 * a `200` from either upload route pre-fills the manual-entry fields and
 * switches back to the manual view; it never bypasses that view or offers
 * a one-click accept. `source` is whatever tier produced the current
 * starting point and is NOT reset to 'manual' by a later manual edit --
 * only a user who never touched an upload keeps `source: 'manual'`.
 *
 * Every request here goes through the same-origin `/api/Train/...` proxy
 * (Client Components can't read the server-only `API_BASE_URL` env var
 * `lib/api.ts` relies on -- same reasoning as `PinToggle`/`TrackTrainForm`),
 * fixed for binary uploads by this plan's own Task 1. */
export function TicketEntryForm({ trackingId, label }: { trackingId: number; label: string }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<Tab>('manual');

  const [operator, setOperator] = useState('');
  const [ticketType, setTicketType] = useState('');
  const [originCrs, setOriginCrs] = useState('');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [source, setSource] = useState<TicketSource>('manual');
  const [autoFilled, setAutoFilled] = useState<Set<string>>(new Set());

  const [uploading, setUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);

  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [needsLogin, setNeedsLogin] = useState(false);

  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const destinationValid = destinationCrs.trim() === '' || CRS_PATTERN.test(destinationCrs.trim());

  function resetFields() {
    setOperator('');
    setTicketType('');
    setOriginCrs('');
    setDestinationCrs('');
    setSource('manual');
    setAutoFilled(new Set());
    setTab('manual');
    setUploadError(null);
    setSubmitError(null);
  }

  function clearAutoFilled(field: string) {
    setAutoFilled((current) => {
      const next = new Set(current);
      next.delete(field);
      return next;
    });
  }

  function applyPreview(preview: PartialTicket) {
    const filled = new Set<string>();
    if (preview.operator) {
      setOperator(preview.operator);
      filled.add('operator');
    }
    if (preview.ticketType) {
      setTicketType(preview.ticketType);
      filled.add('ticketType');
    }
    if (preview.originCrs) {
      setOriginCrs(preview.originCrs);
      filled.add('originCrs');
    }
    if (preview.destinationCrs) {
      setDestinationCrs(preview.destinationCrs);
      filled.add('destinationCrs');
    }
    setSource(preview.source);
    setAutoFilled(filled);
    setTab('manual');
  }

  async function handleUpload(file: File | null, kind: 'pkpass' | 'pdf') {
    if (!file) return;
    setUploading(true);
    setUploadError(null);
    setNeedsLogin(false);
    try {
      const formData = new FormData();
      formData.append('file', file);
      // No explicit Content-Type header -- the browser sets the correct
      // 'multipart/form-data; boundary=...' value itself for a FormData
      // body, and Task 1's proxy fix is what lets that boundary survive to
      // the backend.
      const response = await fetch(`/api/Train/${trackingId}/tickets/${kind}`, {
        method: 'POST',
        body: formData,
      });

      if (response.ok) {
        applyPreview((await response.json()) as PartialTicket);
        return;
      }
      if (response.status === 401) {
        setNeedsLogin(true);
        return;
      }
      if (response.status === 400) {
        setUploadError("That doesn't look like a valid upload — try again or fill in the form manually");
        return;
      }
      if (response.status === 422) {
        // Backend's own message is already human-readable, e.g. "could
        // not read this as a train .pkpass: ..." -- safe to surface
        // directly per Decision 2's table.
        setUploadError(await response.text());
        return;
      }
      if (response.status === 504) {
        setUploadError('That file took too long to read — try a smaller or simpler PDF, or fill in the details manually');
        return;
      }
      if (response.status === 413) {
        setUploadError('That file is too large (8 MB limit). Try filling in the details manually');
        return;
      }
      setUploadError("Couldn't read this file. Try filling in the details manually");
    } catch {
      setUploadError("Couldn't read this file. Try filling in the details manually");
    } finally {
      setUploading(false);
    }
  }

  async function handleSubmit() {
    setSubmitting(true);
    setSubmitError(null);
    setNeedsLogin(false);
    try {
      const body: TicketEntryRequest = {
        source,
        ...(operator.trim() ? { operator: operator.trim() } : {}),
        ...(ticketType.trim() ? { ticket_type: ticketType.trim() } : {}),
        ...(originCrs.trim() ? { origin_crs: originCrs.trim().toUpperCase() } : {}),
        ...(destinationCrs.trim() ? { destination_crs: destinationCrs.trim().toUpperCase() } : {}),
      };
      const response = await fetch(`/api/Train/${trackingId}/tickets`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        setOpen(false);
        resetFields();
        router.refresh();
        return;
      }
      if (response.status === 401) {
        setNeedsLogin(true);
        return;
      }
      if (response.status === 400) {
        setSubmitError(await response.text());
        return;
      }
      setSubmitError("Couldn't save this ticket. Try again.");
    } catch {
      setSubmitError("Couldn't save this ticket. Try again.");
    } finally {
      setSubmitting(false);
    }
  }

  if (!open) {
    return (
      <Button variant="light" onClick={() => setOpen(true)}>
        {label}
      </Button>
    );
  }

  return (
    <Stack gap="md">
      <Tabs value={tab} onChange={(value) => setTab((value as Tab) ?? 'manual')}>
        <Tabs.List>
          <Tabs.Tab value="manual">Manual entry</Tabs.Tab>
          <Tabs.Tab value="pkpass">Upload .pkpass</Tabs.Tab>
          <Tabs.Tab value="pdf">Upload PDF e-ticket</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="manual" pt="md">
          <Stack gap="sm">
            <TextInput
              label="Operator"
              value={operator}
              onChange={(event) => {
                setOperator(event.currentTarget.value);
                clearAutoFilled('operator');
              }}
              rightSection={autoFilled.has('operator') ? <Badge size="xs">auto-filled</Badge> : undefined}
            />
            <TextInput
              label="Ticket type"
              value={ticketType}
              onChange={(event) => {
                setTicketType(event.currentTarget.value);
                clearAutoFilled('ticketType');
              }}
              rightSection={autoFilled.has('ticketType') ? <Badge size="xs">auto-filled</Badge> : undefined}
            />
            <TextInput
              label="Origin CRS code"
              value={originCrs}
              onChange={(event) => {
                setOriginCrs(event.currentTarget.value);
                clearAutoFilled('originCrs');
              }}
              error={!originValid ? 'Must be a 3-letter CRS code' : null}
              description={
                autoFilled.has('originCrs') ? 'Auto-filled — please check this is a real 3-letter CRS code' : undefined
              }
            />
            <TextInput
              label="Destination CRS code"
              value={destinationCrs}
              onChange={(event) => {
                setDestinationCrs(event.currentTarget.value);
                clearAutoFilled('destinationCrs');
              }}
              error={!destinationValid ? 'Must be a 3-letter CRS code' : null}
              description={
                autoFilled.has('destinationCrs')
                  ? 'Auto-filled — please check this is a real 3-letter CRS code'
                  : undefined
              }
            />
          </Stack>
        </Tabs.Panel>

        <Tabs.Panel value="pkpass" pt="md">
          <UploadPanel kind="pkpass" accept=".pkpass" uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>

        <Tabs.Panel value="pdf" pt="md">
          <UploadPanel kind="pdf" accept="application/pdf" uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>
      </Tabs>

      {submitError && (
        <Alert color="red" title="Couldn't save this ticket">
          {submitError}
        </Alert>
      )}

      <Group>
        <Button onClick={handleSubmit} disabled={submitting || !originValid || !destinationValid}>
          {submitting ? 'Saving…' : 'Save ticket'}
        </Button>
        <Button variant="subtle" onClick={() => setOpen(false)}>
          Cancel
        </Button>
        {needsLogin && (
          <TextLink href="/api/auth/login" underline="always">
            Log in to save this ticket
          </TextLink>
        )}
      </Group>
    </Stack>
  );
}

function UploadPanel({
  kind,
  accept,
  uploading,
  error,
  onFile,
  onFallback,
}: {
  kind: 'pkpass' | 'pdf';
  accept: string;
  uploading: boolean;
  error: string | null;
  onFile: (file: File | null, kind: 'pkpass' | 'pdf') => void;
  onFallback: () => void;
}) {
  return (
    <Stack gap="sm">
      <FileInput
        label={kind === 'pkpass' ? 'Apple Wallet .pkpass file' : 'PDF e-ticket'}
        accept={accept}
        disabled={uploading}
        onChange={(file) => onFile(file, kind)}
      />
      {error && (
        <Alert color="red">
          <Stack gap={4}>
            <span>{error}</span>
            {/* Always reachable, per Decision 2's table -- the manual form
                is right there and always usable regardless of why the
                upload failed. */}
            <Button variant="subtle" size="xs" onClick={onFallback} style={{ alignSelf: 'flex-start' }}>
              or fill in the details manually
            </Button>
          </Stack>
        </Alert>
      )}
    </Stack>
  );
}
