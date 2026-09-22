import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import dayjs from 'dayjs';
import { renderWithMantine } from '@/test/render';
import { RunTemplateNowButton } from './RunTemplateNowButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: vi.fn() }),
  usePathname: () => '/journeys/templates/167',
  useSearchParams: () => new URLSearchParams(''),
}));

// Computed at test time, not hardcoded -- matches exactly what the
// component's own `dayjs().format('YYYY-MM-DD')` produces, regardless of
// what day this suite happens to run on.
const today = dayjs().format('YYYY-MM-DD');

describe('RunTemplateNowButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('defaults the service date field to today on open', async () => {
    renderWithMantine(<RunTemplateNowButton templateId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Run now' }));

    expect(await screen.findByLabelText('Service date')).toHaveValue(today);
  });

  it('submits {serviceDate} to the materialize endpoint', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ journeyId: 42, legIds: [1] }), { status: 200 }),
    );

    renderWithMantine(<RunTemplateNowButton templateId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Run now' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create journey' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/JourneyTemplates/167/materialize',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ serviceDate: today }),
        }),
      );
    });
  });

  it('navigates to /journeys/{journeyId} using the response journeyId on success', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response(JSON.stringify({ journeyId: 42, legIds: [1, 2] }), { status: 200 }),
    );

    renderWithMantine(<RunTemplateNowButton templateId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Run now' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create journey' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/journeys/42'));
  });

  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<RunTemplateNowButton templateId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Run now' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create journey' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to run this template' });
    expect(loginLink).toBeInTheDocument();
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('surfaces the server error text on a non-401 failure', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('template has no legs', { status: 400 }));

    renderWithMantine(<RunTemplateNowButton templateId={167} />);
    fireEvent.click(screen.getByRole('button', { name: 'Run now' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Create journey' }));

    expect(await screen.findByText('template has no legs')).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });
});
