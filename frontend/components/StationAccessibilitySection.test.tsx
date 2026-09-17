import { describe, it, expect } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { loadAccessibilityFixture } from '@/test/fixtures/accessibility';
import { StationAccessibilitySection } from './StationAccessibilitySection';

describe('StationAccessibilitySection', () => {
  it('renders the "not yet captured" copy for coverage: unavailable', () => {
    renderWithMantine(<StationAccessibilitySection result={{ coverage: 'unavailable' }} />);
    expect(
      screen.getByText("We don't have station reference data for this station yet."),
    ).toBeInTheDocument();
  });

  it('renders the "nothing published" copy for coverage: empty, distinct from unavailable', () => {
    renderWithMantine(<StationAccessibilitySection result={{ coverage: 'empty' }} />);
    expect(
      screen.getByText('No accessibility or facilities details have been published for this station.'),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("We don't have station reference data for this station yet."),
    ).not.toBeInTheDocument();
  });

  it('renders only the group headings whose keys are present in the data', () => {
    renderWithMantine(
      <StationAccessibilitySection result={{ coverage: 'present', data: { lifts: { count: 2 } } }} />,
    );
    expect(screen.getByText('Facilities')).toBeInTheDocument();
    expect(screen.queryByText('Step-free access & assistance')).not.toBeInTheDocument();
    expect(screen.queryByText('Platform & station facilities')).not.toBeInTheDocument();
    expect(screen.queryByText('Getting here')).not.toBeInTheDocument();
  });

  it('renders humanized field labels and at least one rendered value per present key', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: {
            staffAssistance: 'Available 06:00-23:00',
            carParks: [{ spaces: 120 }],
          },
        }}
      />,
    );
    expect(screen.getByText('Staff assistance')).toBeInTheDocument();
    expect(screen.getByText('Available 06:00-23:00')).toBeInTheDocument();
    expect(screen.getByText('Car parks')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Car parks: 1 item' })).toBeInTheDocument();
  });

  it('pluralizes the item-count control rather than saying "1 items"', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { carParks: [{ spaces: 120 }, { spaces: 40 }] } }}
      />,
    );
    expect(screen.getByRole('button', { name: 'Car parks: 2 items' })).toBeInTheDocument();
  });

  it('keeps an item list collapsed until asked, then reveals each item', async () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { carParks: [{ spaces: 120 }, { spaces: 40 }] } }}
      />,
    );
    expect(screen.queryByText('120')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Car parks: 2 items' }));

    expect(await screen.findByText('120')).toBeInTheDocument();
    expect(screen.getByText('40')).toBeInTheDocument();
  });

  it('renders the heading "Accessibility & facilities", not bare "Accessibility"', () => {
    renderWithMantine(<StationAccessibilitySection result={{ coverage: 'empty' }} />);
    expect(screen.getByRole('heading', { name: 'Accessibility & facilities' })).toBeInTheDocument();
  });

  // Mantine's `Accordion` panel is a `role="region"` landmark named by its
  // own control, so two disclosures whose visible labels happen to match
  // ("1 item" under Lifts and "1 item" under Car parks) produced two
  // identically-named landmarks: an axe `landmark-unique` failure, and a
  // screen-reader landmark list offering several indistinguishable entries.
  // Caught by running axe against /stations/[crs] with every disclosure
  // expanded.
  it('gives colliding disclosures distinct accessible names, qualified by their field', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: { carParks: [{ spaces: 120 }], lifts: [{ note: 'Platform 1' }] },
        }}
      />,
    );
    // Same visible text on both controls...
    expect(screen.getAllByText('1 item')).toHaveLength(2);
    // ...but two different accessible names, so the two landmarks differ.
    expect(screen.getByRole('button', { name: 'Car parks: 1 item' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Lifts: 1 item' })).toBeInTheDocument();

    const names = screen.getAllByRole('button').map((b) => b.getAttribute('aria-label'));
    expect(new Set(names).size).toBe(names.length);
  });

  // WCAG 2.5.3 (Label in Name): the qualifier is a PREFIX, never a
  // replacement, so voice control still activates the control by what is
  // written on it.
  it('keeps the visible label inside the accessible name', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { carParks: [{ spaces: 120 }] } }}
      />,
    );
    const control = screen.getByRole('button', { name: /1 item$/ });
    expect(control.getAttribute('aria-label')).toContain(control.textContent?.trim() ?? '');
  });

  // The backend forwards any non-null allowlisted value verbatim, including
  // `{}` and `[]` (both appear in its own fixtures) -- those must not print
  // a bare label with nothing under it.
  it('skips a key whose value renders to nothing, and its group with it', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { cycling: {}, carParks: [], lifts: { count: 2 } } }}
      />,
    );
    expect(screen.queryByText('Cycling')).not.toBeInTheDocument();
    expect(screen.queryByText('Car parks')).not.toBeInTheDocument();
    expect(screen.queryByText('Getting here')).not.toBeInTheDocument();
    expect(screen.getByText('Facilities')).toBeInTheDocument();
    expect(screen.getByText('Lifts')).toBeInTheDocument();
  });

  // Defense in depth: the route already drops null-valued allowlisted keys,
  // but this component does not treat that as a hard guarantee.
  it('skips a null-valued key instead of rendering it as raw "null"', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { cycling: null, lifts: { count: 2 } } }}
      />,
    );
    expect(screen.queryByText('Cycling')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Raw data/ })).not.toBeInTheDocument();
    expect(screen.getByText('Lifts')).toBeInTheDocument();
  });

  it('renders groups in the spec-defined order, not the response key order', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: {
            cycling: 'Racks on the forecourt',
            lifts: { count: 2 },
            stationAccessibility: { stepFree: true },
            helpAndSupport: '0800 123 4567',
          },
        }}
      />,
    );
    const headings = screen
      .getAllByText(
        /^(Step-free access & assistance|Facilities|Platform & station facilities|Getting here)$/,
      )
      .map((el) => el.textContent);
    expect(headings).toEqual([
      'Step-free access & assistance',
      'Facilities',
      'Platform & station facilities',
      'Getting here',
    ]);
  });

  // The regression this guards: a 200 whose every allowlisted value is
  // empty is `coverage: 'present'` at the page level (the response object
  // has keys), but renders no groups -- without this, the section was a
  // bare heading with no sentence and no content under it.
  it('falls back to the "nothing published" copy when every present key renders to nothing', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { cycling: {}, carParks: [], lifts: { notes: null } } }}
      />,
    );
    expect(
      screen.getByText('No accessibility or facilities details have been published for this station.'),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("We don't have station reference data for this station yet."),
    ).not.toBeInTheDocument();
    expect(screen.queryByText('Facilities')).not.toBeInTheDocument();
  });

  it('does not show the "nothing published" copy alongside real content', () => {
    renderWithMantine(
      <StationAccessibilitySection result={{ coverage: 'present', data: { lifts: { count: 2 } } }} />,
    );
    expect(
      screen.queryByText('No accessibility or facilities details have been published for this station.'),
    ).not.toBeInTheDocument();
  });

  it('drops an empty-valued row inside an object instead of labelling blank space', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { lifts: { count: 2, features: [], notes: '' } } }}
      />,
    );
    expect(screen.getByText('Count:')).toBeInTheDocument();
    expect(screen.queryByText('Features:')).not.toBeInTheDocument();
    expect(screen.queryByText('Notes:')).not.toBeInTheDocument();
  });

  it('counts only the items it will actually show, so the control never over-promises', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { carParks: [{ spaces: 120 }, {}] } }}
      />,
    );
    expect(screen.getByRole('button', { name: 'Car parks: 1 item' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /2 items/ })).not.toBeInTheDocument();
  });

  it('skips an array whose every item renders to nothing', () => {
    renderWithMantine(
      <StationAccessibilitySection result={{ coverage: 'present', data: { carParks: [{}, {}] } }} />,
    );
    expect(screen.queryByText('Car parks')).not.toBeInTheDocument();
    expect(
      screen.getByText('No accessibility or facilities details have been published for this station.'),
    ).toBeInTheDocument();
  });

  it('still reaches a collapsed raw-JSON control for a shape no pattern describes, without throwing', async () => {
    // A non-plain object is the only thing left that gets here -- the whole
    // point of the redesign is that real payloads no longer do (design
    // §4.9). `renderAccessibilityValue` is a total function over `unknown`,
    // and the wire type is twelve `unknown`s, so this stays reachable.
    expect(() =>
      renderWithMantine(
        <StationAccessibilitySection
          result={{ coverage: 'present', data: { stationFacilities: new Date('2026-09-16') } }}
        />,
      ),
    ).not.toThrow();
    const control = screen.getByRole('button', { name: 'Station facilities: Raw data' });
    expect(control).toBeInTheDocument();
    fireEvent.click(control);
    expect(await screen.findByText(/2026-09-16/)).toBeInTheDocument();
  });

  it('renders a deeply nested object as labelled rows now, not as a JSON dump', () => {
    // The exact input the old renderer bailed on. §2.1: that bail fired on
    // 96.2% of real key-renders.
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: { stationFacilities: { level1: { level2: { level3: 'too deep' } } } },
        }}
      />,
    );
    expect(screen.queryByRole('button', { name: /Raw data/ })).not.toBeInTheDocument();
    expect(screen.getByText('Level3:')).toBeInTheDocument();
    expect(screen.getByText('too deep')).toBeInTheDocument();
  });
});

describe('StationAccessibilitySection, pattern rendering', () => {
  // §8's first "rule axe cannot enforce": the icon must never be the only
  // carrier of the availability fact (§4.2).
  it('states availability in words, with the icon purely decorative', () => {
    const { container } = renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: { lifts: { available: false, statement: 'There are no lifts' } },
        }}
      />,
    );
    expect(screen.getByText(/Not available/)).toBeInTheDocument();
    const icons = container.querySelectorAll('svg');
    expect(icons).toHaveLength(1);
    expect(icons[0]).toHaveAttribute('aria-hidden', 'true');
    // Strip every aria-hidden subtree and the fact must survive.
    const clone = container.cloneNode(true) as HTMLElement;
    clone.querySelectorAll('[aria-hidden="true"]').forEach((hidden) => hidden.remove());
    expect(clone.textContent).toContain('Not available');
  });

  it('names a Pattern A key once, on its own availability line', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { lifts: { available: true } } }}
      />,
    );
    expect(screen.getByText('Lifts — Available')).toBeInTheDocument();
    // Not also as a separate label above it.
    expect(screen.queryByText('Lifts')).not.toBeInTheDocument();
  });

  it('renders sanitized note markup as real elements, not as literal tag soup', () => {
    const { container } = renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: {
            stationAccessibility: {
              available: true,
              notes:
                '<p>Call <a href="https://example.com/assist">Passenger Assist</a> in advance.</p>',
            },
          },
        }}
      />,
    );
    const link = screen.getByRole('link', { name: 'Passenger Assist' });
    expect(link).toHaveAttribute('href', 'https://example.com/assist');
    expect(link).toHaveAttribute('target', '_blank');
    expect(container.textContent).not.toContain('<p>');
  });

  it('never injects a heading from feed copy into the page outline', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: {
            helpAndSupport: {
              available: true,
              notes: '<h2>Help points</h2><p>There are help points on every platform.</p>',
            },
          },
        }}
      />,
    );
    // The section's own h2 and nothing else -- axe's `heading-order` cannot
    // see this, because an h2 following an h2 is not a skipped level (§8).
    const headings = screen.getAllByRole('heading');
    expect(headings).toHaveLength(1);
    expect(headings[0]).toHaveTextContent('Accessibility & facilities');
    // Demoted, not discarded: the emphasis survives as bold text.
    expect(screen.getByText('Help points').tagName).toBe('STRONG');
  });

  it('renders a token list as chips rather than a comma-joined string', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: { staffAssistance: { customerInformation: ['DepartureScreens', 'Announcements'] } },
        }}
      />,
    );
    expect(screen.getByText('Departure screens')).toBeInTheDocument();
    expect(screen.getByText('Announcements')).toBeInTheDocument();
    expect(screen.queryByText('DepartureScreens, Announcements')).not.toBeInTheDocument();
  });

  it('renders Pattern D bullets as a real list under each item name', async () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: {
            platformFacilities: {
              platforms: [
                {
                  helpPointClose: 'There is a Help Point close to this platform',
                  name: 'Platform 3',
                  seatingAtIntervals: 'Seating is limited on this platform',
                  waitingType: null,
                },
              ],
            },
          },
        }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /Platforms: 1 item/ }));
    expect(await screen.findByText('Platform 3')).toBeInTheDocument();
    const items = screen.getAllByRole('listitem');
    expect(items.map((item) => item.textContent)).toEqual([
      'There is a Help Point close to this platform',
      'Seating is limited on this platform',
    ]);
  });

  it('links a nearest accessible station to its own page', async () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: {
            stationAccessibility: {
              nearestAccessibleStations: {
                notes: null,
                stations: [{ crsCode: 'SWA', name: 'Swansea' }],
              },
            },
          },
        }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /Stations: 1 item/ }));
    const link = await screen.findByRole('link', { name: 'Swansea (SWA)' });
    expect(link).toHaveAttribute('href', '/stations/SWA');
  });

  it('renders opening times as compacted day ranges and HH:MM times', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{
          coverage: 'present',
          data: {
            staffAssistance: {
              available: true,
              openingTimes: [
                {
                  daysOfTheWeek: ['Saturday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday'],
                  openPeriod: [{ endTime: '00:45:00.000', startTime: '05:00:00.000' }],
                  openingStatus: 'Specific Hours',
                },
              ],
            },
          },
        }}
      />,
    );
    expect(screen.getByText('Mon–Sat, 05:00–00:45')).toBeInTheDocument();
  });
});

describe('StationAccessibilitySection, against real station payloads', () => {
  // DNO (Dunrobin Castle), a seasonal request stop -- the design's own
  // named fixture for an `available: false` record with explanatory notes.
  it('renders DNO without a single raw-JSON disclosure', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: loadAccessibilityFixture('DNO') }}
      />,
    );
    expect(screen.queryByRole('button', { name: /Raw data/ })).not.toBeInTheDocument();
    expect(screen.getByText('Lifts — Not available')).toBeInTheDocument();
  });

  // BAL (smallest payload) and MAN (largest).
  it.each(['BAL', 'MAN'])('renders %s, and every disclosure keeps a unique accessible name', (crs) => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: loadAccessibilityFixture(crs) }}
      />,
    );
    expect(screen.queryByRole('button', { name: /Raw data/ })).not.toBeInTheDocument();
    // The axe `landmark-unique` property, asserted on the collapsed page --
    // `e2e/accessibility.spec.ts` covers the expanded one.
    const names = screen
      .getAllByRole('button')
      .map((button) => button.getAttribute('aria-label') ?? button.textContent);
    expect(new Set(names).size).toBe(names.length);
  });

  it('shows all four category groups for a full payload', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: loadAccessibilityFixture('MAN') }}
      />,
    );
    for (const heading of [
      'Step-free access & assistance',
      'Facilities',
      'Platform & station facilities',
      'Getting here',
    ]) {
      // `getAllBy`, not `getBy`: MAN's own note copy contains the word
      // "Facilities" inside a demoted heading, so the group label is not
      // the only node carrying that text.
      expect(screen.getAllByText(heading).length).toBeGreaterThan(0);
    }
    expect(
      screen.queryByText('No accessibility or facilities details have been published for this station.'),
    ).not.toBeInTheDocument();
  });
});
