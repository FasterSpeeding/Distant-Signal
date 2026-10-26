/** A `fetch` stand-in for the station search (`/api/stations?q=`) that
 * knows `names` (code to name): a code query finds its station, so a code
 * lookup field (`components/StationField.tsx`) can show it by name. Any
 * other request goes to `otherwise` (`[]` by default). */
export function stationNamesFetch(
  names: Record<string, string>,
  otherwise: (url: string) => Response = () => new Response('[]', { status: 200 }),
) {
  return async (input: RequestInfo | URL): Promise<Response> => {
    const url = String(input);
    if (url.startsWith('/api/stations?')) {
      const q = (new URLSearchParams(url.split('?')[1]).get('q') ?? '').toUpperCase();
      const name = names[q];
      return new Response(JSON.stringify(name === undefined ? [] : [{ code: q, name }]), { status: 200 });
    }
    return otherwise(url);
  };
}
