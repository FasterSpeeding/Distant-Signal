import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import NewCustomLinePage from './page';

// CustomLineForm calls useRouter() from next/navigation unconditionally
// at the top of its component body -- throws outside a real Next.js App
// Router tree. Same workaround CustomLineForm.test.tsx's own top-of-file
// mock uses; usePathname/useSearchParams are included too since a real
// 401 on this page would mount LoginLink, which needs them, even though
// no test below exercises that path (CustomLineForm.test.tsx already
// covers 401 handling directly).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/lines/new',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('NewCustomLinePage', () => {
  it('renders the "New custom line" heading and mounts CustomLineForm in create mode with cancelHref="/lines"', () => {
    renderWithMantine(<NewCustomLinePage />);

    expect(screen.getByRole('heading', { name: 'New custom line', level: 1 })).toBeInTheDocument();
    // Create mode, not edit: the Name field is present and the submit
    // button reads "Create line" (CustomLineForm's own create-vs-edit
    // label, unchanged).
    expect(screen.getByLabelText('Name')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create line' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Cancel' })).toHaveAttribute('href', '/lines');
  });
});
