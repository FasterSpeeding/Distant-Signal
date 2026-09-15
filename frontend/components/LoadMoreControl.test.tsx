import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoadMoreControl } from './LoadMoreControl';

const END_MESSAGE = "You've reached the end — no more results.";

describe('LoadMoreControl', () => {
  it('renders the button, and no end-of-results copy, while there is another page', () => {
    renderWithMantine(
      <LoadMoreControl hasMore loading={false} onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();
    expect(screen.queryByText(END_MESSAGE)).not.toBeInTheDocument();
  });

  it('replaces the button with the end-of-results message once there is no next page', () => {
    renderWithMantine(
      <LoadMoreControl hasMore={false} loading={false} onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
    expect(screen.getByText(END_MESSAGE)).toBeInTheDocument();
  });

  it('keeps the live region mounted, and silent, while there are more pages', () => {
    renderWithMantine(
      <LoadMoreControl hasMore loading={false} onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    // Present from the start -- a region inserted into the DOM together with
    // its text is announced inconsistently, so the end message has to land in
    // a region that was already there -- but silent while paging continues.
    expect(screen.getByRole('status')).toBeEmptyDOMElement();
  });

  it('puts the end-of-results message in that same live region', () => {
    renderWithMantine(
      <LoadMoreControl hasMore={false} loading={false} onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.getByRole('status')).toHaveTextContent(END_MESSAGE);
  });

  it('calls onLoadMore when the button is pressed', () => {
    const onLoadMore = vi.fn();
    renderWithMantine(
      <LoadMoreControl hasMore loading={false} onLoadMore={onLoadMore} endMessage={END_MESSAGE} />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Load more' }));

    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it('disables the button while a page is in flight', () => {
    renderWithMantine(<LoadMoreControl hasMore loading onLoadMore={() => {}} endMessage={END_MESSAGE} />);

    expect(screen.getByRole('button', { name: 'Load more' })).toBeDisabled();
  });

  it('reports a failed page and keeps the button for a retry -- it must NOT read as the end of the list', () => {
    renderWithMantine(
      <LoadMoreControl hasMore loading={false} failed onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.getByRole('status')).toHaveTextContent("Couldn't load more results. Try again.");
    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();
    expect(screen.queryByText(END_MESSAGE)).not.toBeInTheDocument();
  });

  it('hides the previous failure, and disables the button, while the retry is in flight', () => {
    renderWithMantine(
      <LoadMoreControl hasMore loading failed onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.getByRole('status')).toBeEmptyDOMElement();
    expect(screen.getByRole('button', { name: 'Load more' })).toBeDisabled();
  });

  it('reports the failure rather than claiming the end of the list when there is no cursor left to retry with', () => {
    renderWithMantine(
      <LoadMoreControl hasMore={false} loading={false} failed onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    // No "Try again" -- there is nothing left to retry with -- but the list
    // must still not claim to be complete.
    expect(screen.getByRole('status')).toHaveTextContent("Couldn't load more results.");
    expect(screen.queryByText(/Try again/)).not.toBeInTheDocument();
    expect(screen.queryByText(END_MESSAGE)).not.toBeInTheDocument();
  });
});
