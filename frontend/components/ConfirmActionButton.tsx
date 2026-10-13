'use client';

import { forwardRef, useImperativeHandle, useState, type ReactNode } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, Modal, Stack, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginControl } from './LoginControl';
import { describeFailure, failureFromResponse } from '@/lib/failure';

type AppRouter = ReturnType<typeof useRouter>;

/** Imperative handle, for a caller that opens the dialog from somewhere
 * other than the trigger (a `Menu.Item`; see `TrackedTrainRowMenu`). */
export interface ConfirmActionButtonHandle {
  open: () => void;
}

export interface ConfirmActionButtonProps {
  /** The trigger's visible text: "Delete", "Remove leg", "Leave group". */
  label: string;
  /** The modal's confirm button text. Defaults to `label`. */
  confirmLabel?: string | undefined;
  /** The action in lower case, starting with the confirm button's visible
   * words: "delete ticket", "remove member". The confirm button is named
   * "Confirm <actionName>" so it never shares a name with the trigger, and
   * its name still contains its visible label (WCAG 2.5.3). */
  actionName: string;
  /** The modal title, a question: "Delete this ticket?" */
  title: string;
  /** What happens, in a sentence or two. A function gets the busy flag,
   * for a body with its own inputs (`DeleteAccountButton`). */
  children?: ReactNode | ((busy: boolean) => ReactNode) | undefined;
  /** `danger` (the default) for anything that removes or takes away, in
   * red; `neutral` for a reversible change (promoting a member). */
  tone?: 'danger' | 'neutral' | undefined;
  /** Sends the request. Called once per confirm click. */
  request: () => Promise<Response>;
  /** `describeFailure`'s verb and noun: `['delete', 'this ticket']`. */
  failure: readonly [verb: string, noun: string];
  /** The 401 prompt, a "Log in to ..." sentence rendered as a login link. */
  loginPrompt?: string | undefined;
  /** Shown instead of a login link on a 401, for a page where logging in
   * again can't help the dialog finish (`DeleteAccountButton`). */
  unauthorizedMessage?: string | undefined;
  /** Appended to the network-failure sentence ("Nothing was deleted."). */
  networkFailureNote?: string | undefined;
  /** Runs after a 2xx. Defaults to `router.refresh()`. */
  onSuccess?: ((router: AppRouter, response: Response) => void | Promise<void>) | undefined;
  /** Keeps the confirm button disabled (a typed confirmation not yet met). */
  confirmDisabled?: boolean | undefined;
  /** Runs when the dialog closes without acting, to reset caller state. */
  onCancel?: (() => void) | undefined;
  /** Renders no trigger; the caller opens the dialog through the ref. */
  hideTrigger?: boolean | undefined;
}

/** The app's one "are you sure?" control: a trigger button that opens a
 * confirmation modal, which sends one request and then refreshes (or does
 * whatever `onSuccess` says).
 *
 * One trigger style everywhere: `size="xs"`, red outline for `danger`,
 * default for `neutral`. A 401 shows `loginPrompt` as a login link rather
 * than the backend's rejection text; any other failure reads through
 * `failureFromResponse`, and a network failure through `describeFailure`.
 * The modal can't be dismissed while the request is in flight. */
export const ConfirmActionButton = forwardRef<ConfirmActionButtonHandle, ConfirmActionButtonProps>(
  function ConfirmActionButton(
    {
      label,
      confirmLabel = label,
      actionName,
      title,
      children,
      tone = 'danger',
      request,
      failure,
      loginPrompt,
      unauthorizedMessage,
      networkFailureNote,
      onSuccess,
      confirmDisabled = false,
      onCancel,
      hideTrigger = false,
    },
    ref,
  ) {
    const router = useRouter();
    const [opened, { open, close }] = useDisclosure(false);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const needsLoginState = useNeedsLogin();
    const [verb, noun] = failure;

    useImperativeHandle(ref, () => ({ open }), [open]);

    function handleClose() {
      if (busy) return;
      setError(null);
      needsLoginState.reset();
      onCancel?.();
      close();
    }

    async function handleConfirm() {
      setBusy(true);
      setError(null);
      needsLoginState.reset();
      try {
        const response = await request();
        if (!response.ok) {
          if (response.status === 401 && unauthorizedMessage) {
            setError(unauthorizedMessage);
          } else if (response.status === 401) {
            needsLoginState.markNeedsLogin();
          } else {
            setError(await failureFromResponse(verb, noun, response));
          }
          setBusy(false);
          return;
        }
        if (onSuccess) {
          // Stays busy: a navigation or refresh replaces this control.
          await onSuccess(router, response);
          return;
        }
        close();
        router.refresh();
        setBusy(false);
      } catch {
        const message = describeFailure(verb, noun);
        setError(networkFailureNote ? `${message} ${networkFailureNote}` : message);
        setBusy(false);
      }
    }

    const danger = tone === 'danger';
    return (
      <>
        {!hideTrigger && (
          <Button
            variant={danger ? 'outline' : 'default'}
            {...(danger && { color: 'red' })}
            size="xs"
            onClick={open}
            data-confirm-trigger
          >
            {label}
          </Button>
        )}
        <Modal opened={opened} onClose={handleClose} title={title}>
          <Stack gap="sm">
            {typeof children === 'function' ? (
              children(busy)
            ) : typeof children === 'string' ? (
              <Text>{children}</Text>
            ) : (
              children
            )}
            {error && (
              <Text c="var(--ds-color-error-text)" role="alert">
                {error}
              </Text>
            )}
            {needsLoginState.needsLogin && loginPrompt && <LoginControl underline="always">{loginPrompt}</LoginControl>}
            <Group justify="end" mt="xs">
              <Button variant="default" onClick={handleClose} disabled={busy}>
                Cancel
              </Button>
              <Button
                {...(danger && { color: 'red' })}
                onClick={handleConfirm}
                loading={busy}
                disabled={confirmDisabled}
                aria-label={`Confirm ${actionName}`}
              >
                {confirmLabel}
              </Button>
            </Group>
          </Stack>
        </Modal>
      </>
    );
  },
);
