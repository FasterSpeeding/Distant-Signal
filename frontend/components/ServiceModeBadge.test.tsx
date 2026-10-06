import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ServiceModeBadge } from './ServiceModeBadge';
import { isTimetableOnly, serviceModeLabel, serviceNoun } from '@/lib/serviceMode';

describe('ServiceModeBadge', () => {
  it.each([
    ['replacementBus', 'Rail replacement bus'],
    ['bus', 'Bus service'],
    ['ferry', 'Ferry'],
  ] as const)('%s: its own label and its own icon', (mode, label) => {
    const { container } = renderWithMantine(<ServiceModeBadge mode={mode} />);
    expect(screen.getByText(label)).toBeInTheDocument();
    const icon = container.querySelector('svg[data-icon]');
    expect(icon).toHaveAttribute('data-icon', mode);
    expect(icon).toHaveAttribute('aria-hidden', 'true');
  });

  it('the three icons are drawn differently', () => {
    const drawn = (['replacementBus', 'bus', 'ferry'] as const).map((mode) => {
      const { container, unmount } = renderWithMantine(<ServiceModeBadge mode={mode} />);
      const markup = container.querySelector('svg')?.innerHTML ?? '';
      unmount();
      return markup;
    });
    expect(new Set(drawn).size).toBe(3);
  });

  it.each([['train'], [null], [undefined]] as const)('renders nothing for %s', (mode) => {
    const { container } = renderWithMantine(<ServiceModeBadge mode={mode} />);
    expect(container.querySelector('[data-service-mode]')).toBeNull();
  });
});

describe('lib/serviceMode', () => {
  it('labels only the non-train modes', () => {
    expect(serviceModeLabel('train')).toBeNull();
    expect(serviceModeLabel(undefined)).toBeNull();
    expect(serviceModeLabel('replacementBus')).toBe('Rail replacement bus');
    expect(serviceModeLabel('bus')).toBe('Bus service');
    expect(serviceModeLabel('ferry')).toBe('Ferry');
  });

  it('timetable-only follows liveTracking, then serviceMode; absent fields are a train', () => {
    expect(isTimetableOnly(undefined)).toBe(false);
    expect(isTimetableOnly({})).toBe(false);
    expect(isTimetableOnly({ serviceMode: 'train', liveTracking: true })).toBe(false);
    expect(isTimetableOnly({ serviceMode: 'bus', liveTracking: false })).toBe(true);
    expect(isTimetableOnly({ liveTracking: false })).toBe(true);
    expect(isTimetableOnly({ serviceMode: 'ferry' })).toBe(true);
  });

  it('names the vehicle in running text', () => {
    expect(serviceNoun('train')).toBe('Train');
    expect(serviceNoun('replacementBus')).toBe('Bus');
    expect(serviceNoun('bus')).toBe('Bus');
    expect(serviceNoun('ferry')).toBe('Ferry');
  });
});
