import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { SaveAsTemplateButton } from './SaveAsTemplateButton';

const pushMock = vi.fn();

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/journeys/42',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('SaveAsTemplateButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockReset();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('opens the modal on click and closes it', async () => {
    renderWithMantine(<SaveAsTemplateButton journeyId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Make this a template' }));
    expect(await screen.findByText('Save this journey as a reusable template')).toBeInTheDocument();

    // Mantine's Modal renders a close button labelled "Close".
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(screen.queryByText('Save this journey as a reusable template')).not.toBeInTheDocument();
    });
  });

  it('submits {mode: fromJourney, journeyId} with no customName when left blank', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ templateId: 99 }), { status: 200 }));

    renderWithMantine(<SaveAsTemplateButton journeyId={7} />);
    fireEvent.click(screen.getByRole('button', { name: 'Make this a template' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save as template' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/JourneyTemplates',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ mode: 'fromJourney', journeyId: 7 }),
        }),
      );
    });
  });

  it('includes a trimmed customName when one is provided', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ templateId: 99 }), { status: 200 }));

    renderWithMantine(<SaveAsTemplateButton journeyId={7} />);
    fireEvent.click(screen.getByRole('button', { name: 'Make this a template' }));
    fireEvent.change(await screen.findByLabelText('Template name (optional)'), {
      target: { value: '  Weekday commute  ' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save as template' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/JourneyTemplates',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ mode: 'fromJourney', journeyId: 7, customName: 'Weekday commute' }),
        }),
      );
    });
  });

  it('shows a login prompt on a 401 rather than the raw rejection text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('unauthorized', { status: 401 }));

    renderWithMantine(<SaveAsTemplateButton journeyId={7} />);
    fireEvent.click(screen.getByRole('button', { name: 'Make this a template' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save as template' }));

    expect(await screen.findByText('Log in to save a template')).toBeInTheDocument();
    expect(screen.queryByText('unauthorized')).not.toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('shows the server\'s own error message text on a non-401 failure', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('Something went wrong', { status: 500 }));

    renderWithMantine(<SaveAsTemplateButton journeyId={7} />);
    fireEvent.click(screen.getByRole('button', { name: 'Make this a template' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save as template' }));

    expect(await screen.findByText('Something went wrong')).toBeInTheDocument();
    expect(screen.queryByText('Log in to save a template')).not.toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('navigates to the new template detail page on success, using the id from the response', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ templateId: 123 }), { status: 200 }));

    renderWithMantine(<SaveAsTemplateButton journeyId={7} />);
    fireEvent.click(screen.getByRole('button', { name: 'Make this a template' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Save as template' }));

    await waitFor(() => {
      expect(pushMock).toHaveBeenCalledWith('/journeys/templates/123');
    });
    // The modal closes on success.
    await waitFor(() => {
      expect(screen.queryByText('Save this journey as a reusable template')).not.toBeInTheDocument();
    });
  });
});
