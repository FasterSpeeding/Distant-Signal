import { type JsonLdObject, serializeJsonLd } from '@/lib/jsonLd';
import { getSiteOrigin } from '@/lib/siteOrigin';

/** The origin for structured data's absolute URLs (`getSiteOrigin`), or
 * null outside a request (no `Host` to fall back on), where the caller
 * renders no structured data rather than failing the page over it. */
export async function structuredDataOrigin(): Promise<string | null> {
  try {
    return await getSiteOrigin();
  } catch {
    return null;
  }
}

/** Server-rendered schema.org structured data. A JSON-LD script is a data
 * block, never executed, so it needs no CSP nonce: `script-src` (nonce and
 * `strict-dynamic`, `proxy.ts`) only governs scripts that run.
 * `serializeJsonLd` escapes `<`, so the data can't close the element. */
export function JsonLd({ data }: { data: JsonLdObject | JsonLdObject[] }) {
  return <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: serializeJsonLd(data) }} />;
}
