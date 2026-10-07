import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { STATIONS, train } from '@/test/lineTrainsFixtures';
import { ServiceRow, ServiceRowList } from './ServiceRow';

const DATE = '2026-10-06';
const live = {
  status: 'en_route',
  delayMinutes: 0,
  delayProvisional: false,
  cancelled: false,
  lastReportedLocation: null,
};

function renderRow(props: Partial<Parameters<typeof ServiceRow>[0]> = {}) {
  return renderWithMantine(
    <ServiceRowList aria-label="Trains">
      <ServiceRow train={train({ uid: 'C1' })} date={DATE} {...props} />
    </ServiceRowList>,
  );
}

describe('ServiceRow', () => {
  it('makes the whole row one link to the train page when there are no actions', () => {
    renderRow({ stations: STATIONS });
    const links = screen.getAllByRole('link');
    expect(links).toHaveLength(1);
    expect(links[0]).toHaveAttribute('href', `/train/C1/${DATE}`);
    expect(links[0]).toHaveTextContent('08:00');
    expect(links[0]).toHaveTextContent('to Weymouth');
    expect(links[0]).toHaveTextContent('Scheduled');
    expect(links[0]).toHaveTextContent('Then calls at Woking and Weymouth');
  });

  it('shows no stop strip without the line catalogue', () => {
    renderRow();
    expect(screen.getByRole('link')).not.toHaveTextContent('Then calls at');
    expect(screen.getByRole('link')).not.toHaveTextContent('Woking');
  });

  it('marks a time on the next day, visibly and in words', () => {
    renderRow({ timeOverride: '00:20', dayOffset: 1 });
    const link = screen.getByRole('link');
    expect(within(link).getByText('+1')).toHaveAttribute('aria-hidden', 'true');
    expect(link).toHaveTextContent('00:20+1 (next day)');
  });

  it('marks an arrival on the next day', () => {
    renderRow({ timeOverride: '23:40', arrival: '02:15', arrivalDayOffset: 1 });
    expect(screen.getByRole('link')).toHaveTextContent('arriving at 02:15+1 (next day)');
    expect(screen.getByRole('link')).not.toHaveTextContent('23:40+1');
  });

  it('shows no marker for the same day', () => {
    renderRow({ timeOverride: '08:00', dayOffset: 0 });
    expect(screen.queryByText('+1')).not.toBeInTheDocument();
  });

  it('shows the details line', () => {
    renderRow({ details: 'From Reading · South Western Railway (SW)' });
    expect(screen.getByRole('link')).toHaveTextContent('From Reading · South Western Railway (SW)');
  });

  it('moves the link onto the destination when the row has actions, so no control sits in a link', () => {
    renderRow({ dayOffset: 1, actions: <button type="button">Track this train</button> });
    const link = screen.getByRole('link');
    const button = screen.getByRole('button', { name: 'Track this train' });
    expect(link).toHaveAttribute('href', `/train/C1/${DATE}`);
    // The link names the time too (the visible time cell is hidden from
    // assistive tech, so it is not read twice).
    expect(link).toHaveTextContent('08:00 (next day) to Weymouth');
    expect(link).not.toContainElement(button);
    expect(button.closest('a')).toBeNull();
    expect(screen.getByRole('listitem')).toHaveTextContent('Scheduled');
  });

  it('says early running as early and strikes a cancelled time', () => {
    const { unmount } = renderRow({ train: train({ uid: 'C1', live: { ...live, delayMinutes: -3 } }) });
    expect(screen.getByText('3 min early')).toBeInTheDocument();
    unmount();
    renderRow({ train: train({ uid: 'C1', live: { ...live, cancelled: true } }) });
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
  });

  it('says "Timetable only" for a bus, with its badge', () => {
    renderRow({ train: train({ uid: 'C1', serviceMode: 'bus', liveTracking: false }) });
    expect(screen.getByRole('link')).toHaveTextContent('Timetable only');
    expect(screen.getByRole('link')).toHaveTextContent('Bus service');
  });
});
