import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ConnectClaudeAuthorizeRetiredPage, { metadata } from './page';

describe('/connect-claude/authorize (retired consent bridge)', () => {
  it('explains the change and links to the instructions page', () => {
    renderWithMantine(<ConnectClaudeAuthorizeRetiredPage />);
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1);
    expect(screen.getByText(/no longer go through this page/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'How to connect Claude' })).toHaveAttribute('href', '/connect-claude');
  });

  it('is kept out of search indexes', () => {
    expect(metadata.robots).toEqual({ index: false });
  });
});
