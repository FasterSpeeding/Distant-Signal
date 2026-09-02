import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ErrorBoundary from './error';

// `TextLink` is a client component that calls usePathname()/useSearchParams()
// -- same stub `app/page.test.tsx` uses for the same reason.
vi.mock('next/navigation', () => ({
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

function errorWithDigest(message: string, digest?: string): Error & { digest?: string } {
  return Object.assign(new Error(message), { digest });
}

describe('app/error.tsx', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('never renders the raw error message', () => {
    renderWithMantine(
      <ErrorBoundary error={errorWithDigest('Minified React error #130', 'abc123')} reset={() => {}} />,
    );
    expect(screen.queryByText(/Minified React error/)).not.toBeInTheDocument();
  });

  it('logs the error instead of showing it', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const error = errorWithDigest('boom', 'abc123');
    renderWithMantine(<ErrorBoundary error={error} reset={() => {}} />);
    expect(spy).toHaveBeenCalledWith('Unhandled error rendering a page', { digest: 'abc123', error });
  });

  it('shows the digest as a quotable reference when there is one', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom', 'abc123')} reset={() => {}} />);
    expect(screen.getByText('Reference: abc123')).toBeInTheDocument();
  });

  it('omits the reference line entirely for a client-side error with no digest', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={() => {}} />);
    expect(screen.queryByText(/^Reference:/)).not.toBeInTheDocument();
  });

  it('offers a route out, not just a retry that will re-throw', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={() => {}} />);
    const link = screen.getByRole('link', { name: 'Back to your dashboard' });
    expect(link).toHaveAttribute('href', '/');
  });

  it('still offers a Try again button that calls reset', () => {
    const reset = vi.fn();
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={reset} />);
    screen.getByRole('button', { name: 'Try again' }).click();
    expect(reset).toHaveBeenCalled();
  });

  it('renders a generic heading rather than the old hardcoded "status data" title', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={() => {}} />);
    expect(screen.getByRole('heading', { name: 'Something went wrong' })).toBeInTheDocument();
  });
});
