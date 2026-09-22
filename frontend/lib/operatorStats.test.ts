import { describe, expect, it } from 'vitest';
import { formatOperatorSampleSummary } from './operatorStats';

describe('formatOperatorSampleSummary', () => {
  it('renders the TfL-specific hedge when a TfL rollup has no sample stats', () => {
    expect(formatOperatorSampleSummary({ code: 'TfL', sampleStats: undefined })).toBe(
      "Not measured by this app — status is TfL's own.",
    );
  });

  it('renders a generic hedge for any other operator with no sample stats', () => {
    expect(formatOperatorSampleSummary({ code: 'SW', sampleStats: undefined })).toBe(
      'No delay/cancellation data available for this operator.',
    );
  });

  it('renders delay and cancellation percentage when stats are present', () => {
    expect(
      formatOperatorSampleSummary({
        code: 'SW',
        sampleStats: { total: 20, delayed: 4, cancelled: 2, skipped: 0, avgDelayMinutes: 3.4 },
      }),
    ).toBe('Avg delay 3.4 min · 10% cancelled');
  });

  it('omits the cancellation clause when total is zero', () => {
    expect(
      formatOperatorSampleSummary({
        code: 'SW',
        sampleStats: { total: 0, delayed: 0, cancelled: 0, skipped: 0, avgDelayMinutes: 0 },
      }),
    ).toBe('Avg delay 0.0 min');
  });
});
