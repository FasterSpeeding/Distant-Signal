import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainNotificationsSwitch } from './TrainNotificationsSwitch';

// The switch's label wraps its description too, so the name starts with it.
const LABEL = /^Notify me about this train/;

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('TrainNotificationsSwitch', () => {
  it('shows the saved state', () => {
    renderWithMantine(<TrainNotificationsSwitch trackingId={7} initiallyEnabled={false} />);
    expect(screen.getByRole('switch', { name: LABEL })).not.toBeChecked();
  });

  it('turns the train off with a PATCH', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ notificationsEnabled: false }), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainNotificationsSwitch trackingId={7} initiallyEnabled />);
    const toggle = screen.getByRole('switch', { name: LABEL });
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeEnabled());
    expect(toggle).not.toBeChecked();
    expect(fetchMock).toHaveBeenCalledWith('/api/Train/7', {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ notificationsEnabled: false }),
    });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('puts the switch back and says so when the save fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('', { status: 500 })),
    );
    renderWithMantine(<TrainNotificationsSwitch trackingId={7} initiallyEnabled />);
    const toggle = screen.getByRole('switch', { name: LABEL });
    fireEvent.click(toggle);
    expect(await screen.findByText("Couldn't save this setting. Try again.")).toBeInTheDocument();
    expect(toggle).toBeChecked();
  });
});
