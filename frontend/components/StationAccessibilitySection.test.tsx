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
    expect(screen.getByRole('button', { name: 'Show 1 item' })).toBeInTheDocument();
  });

  it('pluralizes the item-count control rather than saying "1 items"', () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { carParks: [{ spaces: 120 }, { spaces: 40 }] } }}
      />,
    );
    expect(screen.getByRole('button', { name: 'Show 2 items' })).toBeInTheDocument();
  });

  it('keeps an array-of-objects list collapsed until asked, then reveals each item', async () => {
    renderWithMantine(
      <StationAccessibilitySection
        result={{ coverage: 'present', data: { carParks: [{ spaces: 120 }, { spaces: 40 }] } }}
      />,
    );
    expect(screen.queryByText('120')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Show 2 items' }));

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
    const control = screen.getByRole('button', { name: 'Show raw data' });
    expect(control).toBeInTheDocument();

    fireEvent.click(control);
    expect(await screen.findByText(/too deep/)).toBeInTheDocument();
  });

  it('renders the heading "Accessibility & facilities", not bare "Accessibility"', () => {
    renderWithMantine(<StationAccessibilitySection result={{ coverage: 'empty' }} />);
    expect(screen.getByRole('heading', { name: 'Accessibility & facilities' })).toBeInTheDocument();
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
    expect(screen.queryByRole('button', { name: 'Show raw data' })).not.toBeInTheDocument();
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
});
