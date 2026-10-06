'use client';

import { usePathname } from 'next/navigation';
import { TextLink } from './TextLink';
import { isActiveNavHref, type NavDestination } from '@/lib/navLinks';

/** One inline link in `AppNavBar`, marked `aria-current="page"` (and drawn
 * with the heavier underline `globals.css` keys off that attribute) when it
 * points at the page being shown.
 *
 * A Client Component only because `usePathname` is client-only;
 * `AppNavBar` itself stays a Server Component and renders this as a plain
 * JSX child. It takes the destination's two strings, not a richer object,
 * so the Server/Client boundary stays serializable (see `lib/navLinks.ts`). */
export function PrimaryNavLink({ href, label }: NavDestination) {
  const active = isActiveNavHref(usePathname(), href);
  return (
    <TextLink href={href} ariaCurrent={active ? 'page' : undefined}>
      {label}
    </TextLink>
  );
}
