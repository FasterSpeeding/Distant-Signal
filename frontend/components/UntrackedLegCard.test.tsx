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

  // DS-239: a walk saved as "Vauxhall to Vauxhall" says which end is which
  // when the journey's other legs tell.
  it('says which end of an alike-named walk is National Rail and which the Tube, from the legs either side', () => {
    const walk = leg({ transitMode: 'walk', lineName: null, originName: 'Vauxhall', destinationName: 'Vauxhall' });
    renderWithMantine(
      <UntrackedLegCard
        journeyId={1}
        leg={walk}
        isOwner={false}
        isOnlyLeg={false}
        neighbours={{
          previous: leg({ kind: 'train', transitMode: null, destinationCrs: 'VXH', destinationName: 'Vauxhall' }),
          next: leg({ originName: 'Vauxhall', originStopId: '940GZZLUVXL', destinationStopId: '940GZZLUEUS' }),
        }}
      />,
    );
    expect(screen.getByText('Vauxhall (National Rail) to Vauxhall (Tube)')).toBeInTheDocument();
  });

  it('keeps the stop links when it tags alike names from their stop ids', () => {
    renderWithMantine(
      <UntrackedLegCard
        journeyId={1}
        leg={leg({
          transitMode: 'metro',
          lineName: null,
          originName: 'Canning Town',
          originStopId: '940GZZDLCGT',
          destinationName: 'Canning Town',
          destinationStopId: '940GZZLUCGT',
        })}
        isOwner={false}
        isOnlyLeg={false}
      />,
    );
    expect(screen.getByRole('link', { name: 'Canning Town (DLR)' })).toHaveAttribute('href', '/tfl/stops/940GZZDLCGT');
    expect(screen.getByRole('link', { name: 'Canning Town (Tube)' })).toHaveAttribute('href', '/tfl/stops/940GZZLUCGT');
  });

  it('shows a timetabled leg as scheduled, with its line, stops and any change, never as live', () => {
    renderWithMantine(
      <UntrackedLegCard
        journeyId={1}
        leg={leg({
          transitNetwork: 'tfl',
          transitLineId: 'victoria',
          headsign: 'Brixton',
          originStopId: '940GZZLUKSX',
          destinationStopId: '940GZZLUVIC',
          timetable: {
            network: 'tfl',
            line: { id: 'tfl:victoria', name: 'Victoria line', colour: '#0098d4', mode: 'tube' },
            state: 'retimed',
            scheduledDeparture: '2026-10-21T07:06:00Z',
            scheduledArrival: '2026-10-21T07:21:00Z',
            headsign: 'Brixton',
            calls: [
              {
                stopId: '940GZZLUKSX',
                name: "King's Cross St. Pancras",
                platform: 'Southbound',
                scheduledArrival: null,
                scheduledDeparture: '2026-10-21T07:06:00Z',
              },
              {
                stopId: '940GZZLUVIC',
                name: 'Victoria',
                platform: null,
                scheduledArrival: '2026-10-21T07:21:00Z',
                scheduledDeparture: null,
              },
            ],
            headwayMinutes: 2,
            notices: [
              {
                kind: 'disruption',
                lineId: 'tfl:victoria',
                severity: 9,
                description: 'Minor Delays',
                message: 'Disruption reported on the Victoria line',
                reason: 'Signal failure.',
              },
            ],
          },
        })}
        isOwner={false}
        isOnlyLeg={false}
      />,
    );
    expect(screen.getByText('Scheduled')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Victoria line' })).toHaveAttribute('href', '/transit/tfl/lines/victoria');
    expect(screen.getByText('Towards Brixton')).toBeInTheDocument();
    expect(screen.getByText('Disruption reported on the Victoria line')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'More about this disruption' })).toHaveAttribute(
      'href',
      '/lines/tfl-victoria',
    );
    expect(screen.getByText('The timetable now has this train at 08:06 to 08:21.')).toBeInTheDocument();
    expect(screen.getByText('Timetabled stops')).toBeInTheDocument();
    expect(screen.getAllByText(/King's Cross St. Pancras/).length).toBeGreaterThan(0);
    expect(
      screen.getByText("Times from TfL's timetable, not live. Trains usually run every 2 minutes."),
    ).toBeInTheDocument();
    expect(screen.queryByText('Times are planned. We have no live data for this leg.')).not.toBeInTheDocument();
    expect(screen.queryByText(/on time|expected|delayed/i)).not.toBeInTheDocument();
  });
});
