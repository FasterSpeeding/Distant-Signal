import { createRef } from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ConfirmActionButton, type ConfirmActionButtonHandle } from './ConfirmActionButton';

const refreshMock = vi.fn();
const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock, push: pushMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

function renderDelete(props: Partial<React.ComponentProps<typeof ConfirmActionButton>> = {}) {
  const request = vi.fn(() => fetch('/api/thing/1', { method: 'DELETE' }));
  renderWithMantine(
    <ConfirmActionButton
      label="Delete"
      actionName="delete thing"
      title="Delete this thing?"
      request={request}
      failure={['delete', 'this thing']}
      loginPrompt="Log in to delete this thing"
      {...props}
    >
      This cannot be undone.
    </ConfirmActionButton>,
  );
  return request;
}

async function confirm(actionName = 'delete thing') {
  fireEvent.click(await screen.findByRole('button', { name: `Confirm ${actionName}` }));
}

describe('ConfirmActionButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders one trigger style: an xs red outline button for a danger action', () => {
    renderDelete();
    const trigger = screen.getByRole('button', { name: 'Delete' });
    expect(trigger).toHaveAttribute('data-variant', 'outline');
    expect(trigger).toHaveAttribute('data-size', 'xs');
    expect(trigger).toHaveStyle({ '--button-color': 'var(--mantine-color-red-outline)' });
  });

  it('renders a default (not red) xs trigger for a neutral action', () => {
    renderDelete({ tone: 'neutral', label: 'Promote to admin', actionName: 'promote member' });
    const trigger = screen.getByRole('button', { name: 'Promote to admin' });
    expect(trigger).toHaveAttribute('data-variant', 'default');
    expect(trigger).toHaveAttribute('data-size', 'xs');
  });

  it('sends nothing until confirmed, and names the confirm button "Confirm <action>"', async () => {
    const request = renderDelete({ confirmLabel: 'Delete for good' });
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(await screen.findByText('Delete this thing?')).toBeInTheDocument();
    expect(screen.getByText('This cannot be undone.')).toBeInTheDocument();
    const confirmButton = screen.getByRole('button', { name: 'Confirm delete thing' });
    expect(confirmButton).toHaveTextContent('Delete for good');
    expect(request).not.toHaveBeenCalled();
  });

  it('refreshes on success by default', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(null, { status: 204 }));
    const request = renderDelete();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await confirm();
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
    expect(request).toHaveBeenCalledTimes(1);
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('runs onSuccess instead of refreshing when given', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(null, { status: 204 }));
    renderDelete({ onSuccess: (router) => router.push('/things') });
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await confirm();
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/things'));
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('shows describeFailure copy for a failed request, as an alert', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('gone', { status: 404 }));
    renderDelete();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await confirm();
    expect(await screen.findByRole('alert')).toHaveTextContent("Couldn't delete this thing. It may have been removed.");
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('shows the login prompt on a 401, never the backend text', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    renderDelete();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await confirm();
    const link = await screen.findByRole('link', { name: 'Log in to delete this thing' });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Fgroups%2Fgrp-1');
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
  });

  it('shows unauthorizedMessage instead of a login link when given', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    renderDelete({ unauthorizedMessage: 'Your session has expired.' });
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await confirm();
    expect(await screen.findByRole('alert')).toHaveTextContent('Your session has expired.');
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('appends networkFailureNote when the request itself fails', async () => {
    vi.mocked(fetch).mockRejectedValue(new TypeError('Failed to fetch'));
    renderDelete({ networkFailureNote: 'Nothing was deleted.' });
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await confirm();
    expect(await screen.findByRole('alert')).toHaveTextContent(
      "Couldn't delete this thing. Try again. Nothing was deleted.",
    );
  });

  it('keeps the confirm button disabled while confirmDisabled', async () => {
    renderDelete({ confirmDisabled: true });
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(await screen.findByRole('button', { name: 'Confirm delete thing' })).toBeDisabled();
  });

  it('calls onCancel when cancelled', async () => {
    const onCancel = vi.fn();
    renderDelete({ onCancel });
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalled();
  });

  it('renders no trigger with hideTrigger, and opens through its ref', async () => {
    const ref = createRef<ConfirmActionButtonHandle>();
    renderWithMantine(
      <ConfirmActionButton
        ref={ref}
        hideTrigger
        label="Stop tracking"
        actionName="stop tracking"
        title="Stop tracking this train?"
        request={() => fetch('/api/Train/1', { method: 'DELETE' })}
        failure={['remove', 'this train']}
      />,
    );
    expect(screen.queryByRole('button', { name: 'Stop tracking' })).not.toBeInTheDocument();
    act(() => ref.current?.open());
    expect(await screen.findByRole('button', { name: 'Confirm stop tracking' })).toBeInTheDocument();
  });
});
