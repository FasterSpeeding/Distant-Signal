import { NextResponse } from 'next/server';
import { LEGAL_CONFIG, legalPagesPublished } from '@/lib/legal';
import { getConfiguredSiteOrigin } from '@/lib/siteOrigin';

// LEG-2: RFC 9116 security.txt. Published only together with the legal
// pages (`legalPagesPublished()`: LEGAL_PAGES_PUBLISHED=true and every
// LEGAL_CONFIG placeholder filled in), because its Contact is the same
// LEGAL_CONFIG.CONTACT_EMAIL -- until then it 404s, so a placeholder
// address is never advertised. Read per request, like the legal pages.
export const dynamic = 'force-dynamic';

/** RFC 9116 asks for an Expires under a year away, so the file is revisited.
 * The contact comes from live config, so the expiry is rolled forward from
 * each response rather than hand-edited. */
export const SECURITY_TXT_VALIDITY_DAYS = 180;

export function buildSecurityTxt(now: Date = new Date(), siteOrigin: string | undefined = getConfiguredSiteOrigin()): string {
  const expires = new Date(now.getTime() + SECURITY_TXT_VALIDITY_DAYS * 24 * 60 * 60 * 1000);
  const lines = [`Contact: mailto:${LEGAL_CONFIG.CONTACT_EMAIL}`, `Expires: ${expires.toISOString().replace(/\.\d{3}Z$/, 'Z')}`];
  if (siteOrigin) {
    lines.push(`Canonical: ${siteOrigin}/.well-known/security.txt`);
    lines.push(`Policy: ${siteOrigin}/contact`);
  }
  lines.push('Preferred-Languages: en');
  return `${lines.join('\n')}\n`;
}

export function GET(): NextResponse {
  if (!legalPagesPublished()) {
    return new NextResponse('Not Found', { status: 404 });
  }
  return new NextResponse(buildSecurityTxt(), {
    headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'public, max-age=86400' },
  });
}
