import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { UntrackedLegCard } from './UntrackedLegCard';
import type { JourneyLegDetail } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn(), push: vi.fn() }),
  usePathname: () => '/journeys/1',
  useSearchParams: () => new URLSearchParams(''),
}));

function leg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 5,
    kind: 'untracked',
    originCrs: null,
    originName: "King's Cross St Pancras",
    destinationCrs: null,
    destinationName: 'Victoria',
    serviceDate: '2026-10-21',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    trackedTrainState: null,
    legSkip: null,
    transitMode: 'tube',
    lineName: 'Victoria line',
    plannedDeparture: '2026-10-21T07:05:00Z',
    plannedArrival: '2026-10-21T07:20:00Z',
    untrackedStatus: 'planned',
    ...overrides,
  };
}

/** Spec 2026-10-20 §2.2: each clock-only state's display. */
describe('UntrackedLegCard', () => {
  it.each([
    ['planned', 'Planned'],
    ['inProgress', 'In progress (not live tracked)'],
    ['done', 'Done'],
  ] as const)('shows the %s state as "%s"', (untrackedStatus, label) => {
    renderWithMantine(
      <UntrackedLegCard journeyId={1} leg={leg({ untrackedStatus })} isOwner={false} isOnlyLeg={false} />,
    );
    expect(screen.getByText(label)).toBeInTheDocument();
  });

  it('names the mode, line, places and planned times, and says it is not live', () => {
    renderWithMantine(<UntrackedLegCard journeyId={1} leg={leg()} isOwner={false} isOnlyLeg={false} />);
    expect(screen.getByText('Tube: Victoria line')).toBeInTheDocument();
    expect(screen.getByText("King's Cross St Pancras to Victoria")).toBeInTheDocument();
    expect(screen.getByText('21 Oct 2026, 08:05 to 08:20')).toBeInTheDocument();
    expect(screen.getByText('Times are planned. We have no live data for this leg.')).toBeInTheDocument();
  });

  it('falls back to the mode alone, and one place, when that is all there is', () => {
    renderWithMantine(
      <UntrackedLegCard
        journeyId={1}
        leg={leg({ transitMode: 'walk', lineName: null, originName: null })}
        isOwner={false}
        isOnlyLeg={false}
      />,
    );
    expect(screen.getByText('Walk')).toBeInTheDocument();
    expect(screen.getByText('To Victoria')).toBeInTheDocument();
  });

  it('offers the owner, and only the owner, a way to remove it', () => {
    const { unmount } = renderWithMantine(<UntrackedLegCard journeyId={1} leg={leg()} isOwner isOnlyLeg={false} />);
    expect(screen.getByRole('button', { name: /remove/i })).toBeInTheDocument();
    unmount();
    renderWithMantine(<UntrackedLegCard journeyId={1} leg={leg()} isOwner={false} isOnlyLeg={false} />);
    expect(screen.queryByRole('button', { name: /remove/i })).not.toBeInTheDocument();
  });
});

describe('UntrackedLegCard stop links', () => {
  it("links a saved tram ride's Metrolink stations to their stop pages", () => {
    renderWithMantine(
      <UntrackedLegCard
        journeyId={1}
        leg={leg({
          transitMode: 'tram',
          lineName: 'Blue Line',
          originName: 'Piccadilly',
          destinationName: 'Victoria',
          originStopId: '9400ZZMAPIC',
          destinationStopId: '9400ZZMAVIC',
        })}
        isOwner={false}
        isOnlyLeg={false}
      />,
    );
    expect(screen.getByRole('link', { name: 'Piccadilly' })).toHaveAttribute(
      'href',
      '/transit/metrolink/stops/9400ZZMAPIC',
    );
    expect(screen.getByRole('link', { name: 'Victoria' })).toHaveAttribute(
      'href',
      '/transit/metrolink/stops/9400ZZMAVIC',
    );
  });

  it('links a London tram stop to its TfL stop page, and leaves a free-text place as text', () => {
    renderWithMantine(
      <UntrackedLegCard
        journeyId={1}
        leg={leg({
          transitMode: 'tram',
          lineName: null,
          originName: 'Wimbledon',
          destinationName: 'Home',
          originStopId: '940GZZCRWMB',
          destinationStopId: null,
        })}
        isOwner={false}
        isOnlyLeg={false}
      />,
    );
    expect(screen.getByRole('link', { name: 'Wimbledon' })).toHaveAttribute('href', '/tfl/stops/940GZZCRWMB');
    expect(screen.queryByRole('link', { name: 'Home' })).toBeNull();
  });
});
