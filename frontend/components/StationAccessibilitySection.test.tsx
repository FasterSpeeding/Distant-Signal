import { describe, it, expect } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
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

  it('keeps an array-of-objects list collapsed until asked, then reveals each item', async () => {
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

  it('renders a raw-JSON fallback inside a collapsed control for an unexpected deep shape, without throwing', async () => {
    const deeplyNested = { level1: { level2: { level3: 'too deep' } } };
    expect(() =>
      renderWithMantine(
        <StationAccessibilitySection
          result={{ coverage: 'present', data: { stationFacilities: deeplyNested } }}
        />,
      ),
    ).not.toThrow();
    expect(screen.getByText('Station facilities')).toBeInTheDocument();
    // Collapsed by default, so the raw JSON itself is not in the document
    // until the control is used -- only the control is asserted here.
    expect(screen.queryByText(/too deep/)).not.toBeInTheDocument();
    const control = screen.getByRole('button', { name: 'Station facilities: Raw data' });
    expect(control).toBeInTheDocument();

    fireEvent.click(control);
    expect(await screen.findByText(/too deep/)).toBeInTheDocument();
  });

  it('renders the heading "Accessibility & facilities", not bare "Accessibility"', () => {
    renderWithMantine(<StationAccessibilitySection result={{ coverage: 'empty' }} />);
    expect(screen.getByRole('heading', { name: 'Accessibility & facilities' })).toBeInTheDocument();
  });

  // Mantine's `Accordion` panel is a `role="region"` landmark named by its
  // own control, so two disclosures whose visible labels happen to match
  // ("1 item" under Lifts and "1 item" under Car parks; "Raw data" under
  // each of two unmodelled keys) produced two identically-named landmarks:
  // an axe `landmark-unique` failure, and a screen-reader landmark list
  // offering several indistinguishable entries. Caught by running axe
  // against /stations/[crs] with every disclosure expanded.
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
  // but this component does not treat that as a hard guarantee (design spec
  // Decision 6).
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

  it('drops an empty-valued row inside a shallow object instead of labelling blank space', () => {
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
});
