import '@/app/globals.css';
import { MantineProvider, ColorSchemeScript, mantineHtmlProps, Group, Text } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';

export const metadata: Metadata = {
  title: 'National Rail Status',
  description: 'Line status for UK National Rail, TfL-style.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript />
      </head>
      <body>
        <MantineProvider>
          <Group
            component="nav"
            justify="space-between"
            p="md"
            style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}
          >
            {/* Plain `<Link>` wrapping Mantine's `Text`, rather than
                `component={Link}` on a Mantine polymorphic prop: this file
                is a Server Component, and passing the `Link` component
                reference into a Mantine `component` prop from a Server
                Component previously broke `next build`'s Server/Client
                boundary serialization check (see LineStatusCard fix). */}
            <Link href="/" style={{ textDecoration: 'none', color: 'inherit' }}>
              <Text fw={700}>National Rail Line Status</Text>
            </Link>
            <Link href="/stations" style={{ textDecoration: 'none' }}>
              <Text c="blue">Station Lookup</Text>
            </Link>
          </Group>
          {children}
        </MantineProvider>
      </body>
    </html>
  );
}
