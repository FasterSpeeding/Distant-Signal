import '@/app/globals.css';
import { MantineProvider, ColorSchemeScript, mantineHtmlProps } from '@mantine/core';
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
        <MantineProvider>{children}</MantineProvider>
      </body>
    </html>
  );
}
