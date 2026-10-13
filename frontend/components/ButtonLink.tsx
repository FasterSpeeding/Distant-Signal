'use client';

import type { ReactNode } from 'react';
import Link from 'next/link';
import { Button, type ButtonProps } from '@mantine/core';

/** An in-app link drawn as a Mantine `Button`: "Create a group", "Open
 * group", "Edit", "Back to chat".
 *
 * It renders one `<a class="mantine-Button-root">`, not a `<button>` inside
 * an `<a>` (invalid HTML: interactive content inside a link, and two tab
 * stops for one action). A Client Component so a Server Component page can
 * use it: only this file passes `Link` as Mantine's polymorphic
 * `component`, which a Server Component can't do (a component isn't a
 * serialisable prop). For the login link use `LoginControl`. */
export function ButtonLink({
  href,
  children,
  variant,
  size,
  leftSection,
}: {
  href: string;
  children: ReactNode;
  variant?: ButtonProps['variant'] | undefined;
  size?: ButtonProps['size'] | undefined;
  leftSection?: ReactNode | undefined;
}) {
  return (
    <Button
      component={Link}
      href={href}
      {...(variant !== undefined && { variant })}
      {...(size !== undefined && { size })}
      {...(leftSection !== undefined && { leftSection })}
    >
      {children}
    </Button>
  );
}
