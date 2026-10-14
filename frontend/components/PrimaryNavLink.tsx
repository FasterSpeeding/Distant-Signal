'use client';

import { usePathname } from 'next/navigation';
import { TextLink } from './TextLink';
import { isActiveNavHref, type NavDestination } from '@/lib/navLinks';

/** One inline link in `AppNavBar`, marked `aria-current="page"` (and drawn
 * with the heavier underline `globals.css` keys off that attribute) when it
 * points at the page being shown, or at the section it belongs to.
 *
 * A Client Component only because `usePathname` is client-only;
 * `AppNavBar` itself stays a Server Component and renders this as a plain
 * JSX child. It takes the destination's plain strings, not a richer object,
 * so the Server/Client boundary stays serializable (see `lib/navLinks.ts`). */
export function PrimaryNavLink(destination: NavDestination) {
  const { href, label } = destination;
  const active = isActiveNavHref(usePathname(), destination);
  return (
    <TextLink href={href} ariaCurrent={active ? 'page' : undefined}>
      {label}
    </TextLink>
  );
}
