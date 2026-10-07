import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { badgeTextTransform } from '@/test/badge';
import { TrackedTrainStatusBadge } from './TrackedTrainStatusBadge';

// Direct unit tests for the component review §2.9 asked to be shared
// (previously two byte-identical copies, `app/page.tsx`'s own
// `TrackedTrainStatusBadge` and `app/track/mine/page.tsx`'s
// `RowStatusBadge`, plus a third place -- `/groups/[id]`'s shared-train
// card -- that printed the raw `status` string with no wording at all).
// The three call sites' own page tests already exercise this indirectly;
// these pin the component's own branching once, independent of any page.
describe('TrackedTrainStatusBadge', () => {
  it('shows the resolution status when not yet resolved', () => {
    renderWithMantine(
      <TrackedTrainStatusBadge train={{ resolutionStatus: 'pending', status: null, delayMinutes: null }} />,
    );
    expect(badgeTextTransform(screen.getByText('Pending match'))).toBe('none');
  });

  it('falls back to the raw resolution-status token when it has no label', () => {
    renderWithMantine(
      <TrackedTrainStatusBadge train={{ resolutionStatus: 'something_new', status: null, delayMinutes: null }} />,
    );
    expect(screen.getByText('something_new')).toBeInTheDocument();
  });

  it('shows the journey status word once resolved, not the raw enum token', () => {
    renderWithMantine(
      <TrackedTrainStatusBadge train={{ resolutionStatus: 'resolved', status: 'en_route', delayMinutes: null }} />,
    );
    expect(screen.getByText('En route')).toBeInTheDocument();
    expect(screen.queryByText('en_route')).not.toBeInTheDocument();
  });

  it('adds a delay badge alongside the status badge when delayMinutes is positive', () => {
    renderWithMantine(
      <TrackedTrainStatusBadge train={{ resolutionStatus: 'resolved', status: 'en_route', delayMinutes: 5 }} />,
    );
    expect(badgeTextTransform(screen.getByText('En route'))).toBe('none');
    expect(badgeTextTransform(screen.getByText('5 min late'))).toBe('none');
  });

  it('says early running as minutes early, not "On time"', () => {
    renderWithMantine(
      <TrackedTrainStatusBadge train={{ resolutionStatus: 'resolved', status: 'en_route', delayMinutes: -2 }} />,
    );
    expect(screen.getByText('2 min early')).toBeInTheDocument();
    expect(screen.queryByText('On time')).not.toBeInTheDocument();
  });

  it('renders "On time" rather than a delay badge when delayMinutes is zero', () => {
    renderWithMantine(
      <TrackedTrainStatusBadge train={{ resolutionStatus: 'resolved', status: 'completed', delayMinutes: 0 }} />,
    );
    expect(badgeTextTransform(screen.getByText('On time'))).toBe('none');
  });

  it('renders no status badge when resolved with a null journey status', () => {
    // Not `container.textContent` -- MantineProvider injects `<style>`
    // tags into the render tree (see EtaBadge.test.tsx's own comment on
    // the same issue), so the container is never literally text-empty.
    // No `.mantine-Badge-root` at all is the actual claim this test makes.
    const { container } = renderWithMantine(
      <TrackedTrainStatusBadge train={{ resolutionStatus: 'resolved', status: null, delayMinutes: null }} />,
    );
    expect(container.querySelectorAll('.mantine-Badge-root')).toHaveLength(0);
  });
});

describe('TrackedTrainStatusBadge: buses and ferries', () => {
  it('a tracked bus shows its mode and "Timetabled only", not a stuck match status', () => {
    renderWithMantine(
      <TrackedTrainStatusBadge
        train={{
          resolutionStatus: 'schedule_matched',
          status: null,
          delayMinutes: null,
          serviceMode: 'replacementBus',
          liveTracking: false,
        }}
      />,
    );
    expect(screen.getByText('Rail replacement bus')).toBeInTheDocument();
    expect(screen.getByText('Timetabled only')).toBeInTheDocument();
    expect(screen.queryByText('Matched to schedule')).not.toBeInTheDocument();
  });
});
