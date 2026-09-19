import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import NewGroupPage from './page';

// CreateGroupForm calls useRouter() from next/navigation unconditionally
// at the top of its component body -- throws outside a real Next.js App
// Router tree. Same workaround app/lines/new/page.test.tsx's own top-of-file
// mock uses for the equivalent form.
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/groups/new',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('NewGroupPage', () => {
  it('renders the "Create a group" heading and mounts CreateGroupForm', () => {
    renderWithMantine(<NewGroupPage />);

    expect(screen.getByRole('heading', { name: 'Create a group', level: 1 })).toBeInTheDocument();
    expect(screen.getByLabelText('Group name', { exact: false })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create group' })).toBeInTheDocument();
  });

  // Review §3.2.6: the page said nothing about what happens after
  // submitting -- CreateGroupForm immediately rotates the new group's
  // first invite link and navigates to its detail page, but none of that
  // was visible to someone filling in the form.
  it("explains that an invite link follows group creation", () => {
    renderWithMantine(<NewGroupPage />);
    expect(
      screen.getByText("You'll get an invite link to share as soon as it's created."),
    ).toBeInTheDocument();
  });

  // Review §3.2.8, deliberately on top of Task 1.1's root-cause `<main>`
  // fix, not instead of it: a one-field form stretching the full content
  // width reads as unfinished.
  it('caps the content width at 480px', () => {
    renderWithMantine(<NewGroupPage />);
    const stack = screen.getByRole('heading', { name: 'Create a group', level: 1 }).closest('.mantine-Stack-root');
    expect(stack).toHaveStyle({ maxWidth: 'calc(30rem * var(--mantine-scale))' });
  });
});
