import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CopyableLink } from './CopyableLink';
import { COPIED_RESET_MS } from './useShareOrCopy';

// jsdom has neither `navigator.share` nor `navigator.clipboard`; each test
// defines what's "available", matching how useShareOrCopy feature-detects.
function stubNavigator(name: 'share' | 'clipboard', value: unknown) {
  Object.defineProperty(navigator, name, { value, writable: true, configurable: true });
}

describe('CopyableLink', () => {
  let writeText: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    writeText = vi.fn().mockResolvedValue(undefined);
    stubNavigator('clipboard', { writeText });
    stubNavigator('share', undefined);
  });

  afterEach(() => {
    // @ts-expect-error -- removing a property TS believes is always present.
    delete navigator.share;
    // @ts-expect-error -- same, for clipboard.
    delete navigator.clipboard;
    vi.useRealTimers();
  });

  it('renders a read-only field with a visible label and a "Copy <label>" button', () => {
    renderWithMantine(<CopyableLink value="https://example.test/mcp" label="MCP server URL" />);
    const field = screen.getByLabelText('MCP server URL');
    expect(field).toHaveValue('https://example.test/mcp');
    expect(field).toHaveAttribute('readonly');
    expect(screen.getByRole('button', { name: 'Copy MCP server URL' })).toBeInTheDocument();
  });

  it('uses the label as the field name only, mid-sentence on the button, with hideLabel', () => {
    renderWithMantine(<CopyableLink value="https://example.test/j/1" label="Invite link" hideLabel share />);
    expect(screen.getByRole('textbox', { name: 'Invite link' })).toBeInTheDocument();
    expect(screen.queryByText('Invite link')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Share invite link' })).toBeInTheDocument();
  });

  it('sizes the button to tokens.iconButton.field (36px), matching the input beside it', () => {
    renderWithMantine(<CopyableLink value="x" label="Connector URL" />);
    expect(screen.getByRole('button', { name: 'Copy Connector URL' })).toHaveStyle({
      '--ai-size': 'calc(2.25rem * var(--mantine-scale))',
    });
  });

  it('copies the value and announces "Copied." for a couple of seconds', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true, toFake: ['setTimeout', 'clearTimeout'] });
    renderWithMantine(<CopyableLink value="claude mcp add x" label="Claude Code command" monospace />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy Claude Code command' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('claude mcp add x'));
    expect(await screen.findByRole('status')).toHaveTextContent('Copied.');
    await act(async () => {
      await vi.advanceTimersByTimeAsync(COPIED_RESET_MS);
    });
    expect(screen.getByRole('status')).toHaveTextContent('');
  });

  it('says how to copy by hand when the clipboard refuses', async () => {
    writeText.mockRejectedValue(new Error('denied'));
    renderWithMantine(<CopyableLink value="x" label="MCP server URL" />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy MCP server URL' }));
    expect(await screen.findByText("Couldn't copy. Select the text and copy it yourself.")).toBeInTheDocument();
  });

  it('opens the share sheet first with share, and does not touch the clipboard', async () => {
    const share = vi.fn().mockResolvedValue(undefined);
    stubNavigator('share', share);
    renderWithMantine(
      <CopyableLink value="https://example.test/g/1" label="Invite link" hideLabel share shareTitle="Join my group" />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Share invite link' }));
    await waitFor(() =>
      expect(share).toHaveBeenCalledWith({ url: 'https://example.test/g/1', title: 'Join my group' }),
    );
    expect(writeText).not.toHaveBeenCalled();
  });

  it('does nothing more when the share sheet is cancelled (AbortError)', async () => {
    const share = vi.fn().mockRejectedValue(Object.assign(new Error('cancelled'), { name: 'AbortError' }));
    stubNavigator('share', share);
    renderWithMantine(<CopyableLink value="u" label="Invite link" hideLabel share />);
    fireEvent.click(screen.getByRole('button', { name: 'Share invite link' }));
    await waitFor(() => expect(share).toHaveBeenCalled());
    expect(writeText).not.toHaveBeenCalled();
    expect(screen.getByRole('status')).toHaveTextContent('');
  });

  it('falls back to the clipboard when sharing fails for another reason', async () => {
    stubNavigator('share', vi.fn().mockRejectedValue(new Error('no target')));
    renderWithMantine(<CopyableLink value="u" label="Invite link" hideLabel share />);
    fireEvent.click(screen.getByRole('button', { name: 'Share invite link' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('u'));
  });
});
