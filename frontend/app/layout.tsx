import '@/app/globals.css';
import { MantineProvider, ColorSchemeScript, mantineHtmlProps, Group, Text } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { ThemeToggle } from '@/components/ThemeToggle';

export const metadata: Metadata = {
  title: 'National Rail Status',
  description: 'Line status for UK National Rail, TfL-style.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript defaultColorScheme="auto" />
      </head>
      <body>
        <MantineProvider defaultColorScheme="auto">
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
                boundary serialization check (see LineStatusCard fix).
                `ThemeToggle` below doesn't hit this: it's imported and
                rendered as a plain JSX element (a Client Component child
                of this Server Component), not passed as a value into a
                Mantine `component` prop — a different, safe pattern. */}
            <Link href="/" style={{ textDecoration: 'none', color: 'inherit' }}>
              <Text fw={700}>National Rail Line Status</Text>
            </Link>
            <Group gap="lg">
              <Link href="/lines" style={{ textDecoration: 'none' }}>
                <Text c="blue">All Lines</Text>
              </Link>
              <Link href="/stations" style={{ textDecoration: 'none' }}>
                <Text c="blue">Station Lookup</Text>
              </Link>
              <ThemeToggle />
            </Group>
          </Group>
          {children}
        </MantineProvider>
      </body>
    </html>
  );
}
