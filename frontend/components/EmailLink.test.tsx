import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { renderToString } from 'react-dom/server';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { renderWithMantine } from '@/test/render';
import { EmailLink } from './EmailLink';

describe('EmailLink', () => {
  it('server-renders no address pattern for Cloudflare to rewrite', () => {
    const html = renderToString(
      <MantineProvider theme={theme}>
        <EmailLink address="info@example.co.uk" />
      </MantineProvider>,
    );
    expect(html).not.toContain('info@example.co.uk');
    expect(html).toContain('info at example.co.uk');
  });

  it('becomes a mailto link once mounted', () => {
    renderWithMantine(<EmailLink address="info@example.co.uk" />);
    expect(screen.getByRole('link', { name: 'info@example.co.uk' })).toHaveAttribute(
      'href',
      'mailto:info@example.co.uk',
    );
  });
});
