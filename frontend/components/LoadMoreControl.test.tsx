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

  it('announces the end-of-results message as a live region, since it replaces the button that was just pressed', () => {
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
    renderWithMantine(
      <LoadMoreControl hasMore loading onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.getByRole('button', { name: 'Load more' })).toBeDisabled();
  });

  it('reports a failed page as an error and keeps the button for a retry -- it must NOT read as the end of the list', () => {
    renderWithMantine(
      <LoadMoreControl hasMore loading={false} failed onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't load more results. Try again.");
    expect(screen.getByRole('button', { name: 'Load more' })).toBeEnabled();
    expect(screen.queryByText(END_MESSAGE)).not.toBeInTheDocument();
  });

  it('hides the previous failure while the retry is in flight', () => {
    renderWithMantine(
      <LoadMoreControl hasMore loading failed onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('reports the failure rather than claiming the end of the list when there is no cursor left to retry with', () => {
    renderWithMantine(
      <LoadMoreControl hasMore={false} loading={false} failed onLoadMore={() => {}} endMessage={END_MESSAGE} />,
    );

    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't load more results.");
    expect(screen.queryByText(END_MESSAGE)).not.toBeInTheDocument();
  });
});
