import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AppMantineProvider } from './AppMantineProvider';

// Regression coverage for the fix this component exists for: `app/layout.tsx`
// (a Server Component) used to pass `theme={theme}` straight into
// `MantineProvider`, which broke `next build` once `theme` gained a
// `variantColorResolver` function (Next's RSC boundary refuses to serialize
// a function passed as a prop from a Server Component into a Client
// Component). jsdom/@testing-library can't reproduce that RSC serialization
// failure itself (only a real `next build` does -- see this file's sibling
// comment in AppMantineProvider.tsx), so this just locks down that the
// wrapper still renders its children under the real production theme.
describe('AppMantineProvider', () => {
  it('renders children under the real theme (grape primary colour)', () => {
    render(
      <AppMantineProvider>
        <div data-testid="probe">child</div>
      </AppMantineProvider>,
    );
    expect(screen.getByTestId('probe')).toBeInTheDocument();
    const anchorColor = getComputedStyle(document.documentElement).getPropertyValue('--mantine-color-anchor');
    expect(anchorColor).toBe('var(--mantine-color-grape-6)');
  });
});
