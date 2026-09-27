import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AccountDeletedPage from './page';

describe('AccountDeletedPage', () => {
  it('confirms deletion and states the backup window', () => {
    renderWithMantine(<AccountDeletedPage />);
    expect(screen.getByRole('heading', { name: 'Your account has been deleted' })).toBeInTheDocument();
    expect(screen.getByText(/gone from them within 7 days/)).toBeInTheDocument();
  });
});
