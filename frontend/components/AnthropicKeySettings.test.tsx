import { describe, it, expect, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AnthropicKeySettings } from './AnthropicKeySettings';
import { getAnthropicApiKey } from '@/lib/anthropicKey';

describe('AnthropicKeySettings', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('shows the one-time disclosure text and no key set state', () => {
    renderWithMantine(<AnthropicKeySettings />);
    expect(screen.getByText(/stored only in your browser/i)).toBeInTheDocument();
    expect(screen.getByText(/never (sent to|seen by) (any )?distant signal/i)).toBeInTheDocument();
    expect(screen.getByText(/no key set/i)).toBeInTheDocument();
  });

  it('saves a key entered in the input', () => {
    renderWithMantine(<AnthropicKeySettings />);
    fireEvent.change(screen.getByLabelText(/anthropic api key/i), { target: { value: 'sk-ant-abc123' } });
    fireEvent.click(screen.getByRole('button', { name: /save/i }));
    expect(getAnthropicApiKey()).toBe('sk-ant-abc123');
    expect(screen.getByText(/key saved/i)).toBeInTheDocument();
  });

  it('masks a previously saved key rather than showing it in full', () => {
    setAnthropicApiKeyForTest('sk-ant-abcdefghijklmnop');
    renderWithMantine(<AnthropicKeySettings />);
    expect(screen.queryByDisplayValue('sk-ant-abcdefghijklmnop')).not.toBeInTheDocument();
    expect(screen.getByText(/key saved/i)).toBeInTheDocument();
  });

  it('clears a saved key', () => {
    setAnthropicApiKeyForTest('sk-ant-abc123');
    renderWithMantine(<AnthropicKeySettings />);
    fireEvent.click(screen.getByRole('button', { name: /clear/i }));
    expect(getAnthropicApiKey()).toBeNull();
    expect(screen.getByText(/no key set/i)).toBeInTheDocument();
  });
});

function setAnthropicApiKeyForTest(key: string) {
  localStorage.setItem('ds-anthropic-api-key', key);
}
