import type { LineStatusReport, LineStatusHistoryEntry } from './types';

function baseUrl(): string {
  const url = process.env.API_BASE_URL;
  if (!url) {
    throw new Error('API_BASE_URL environment variable is not set');
  }
  return url;
}

async function fetchJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw new Error(`API request to ${url} failed: ${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
}

export async function getLineStatusForMode(mode: string): Promise<LineStatusReport[]> {
  return fetchJson<LineStatusReport[]>(`${baseUrl()}/Line/Mode/${mode}/Status`, {
    next: { revalidate: 30 },
  });
}

export async function getLineStatus(ids: string[], detail: boolean): Promise<LineStatusReport[]> {
  const idsParam = ids.join(',');
  const query = detail ? '?detail=true' : '';
  return fetchJson<LineStatusReport[]>(`${baseUrl()}/Line/${idsParam}/Status${query}`, {
    next: { revalidate: 30 },
  });
}

export async function getStopPointDisruption(crs: string): Promise<LineStatusReport[]> {
  return fetchJson<LineStatusReport[]>(`${baseUrl()}/StopPoint/${crs}/Disruption`, {
    cache: 'no-store',
  });
}

export async function getLineStatusHistory(
  id: string,
  from: string,
  to: string,
): Promise<LineStatusHistoryEntry[]> {
  return fetchJson<LineStatusHistoryEntry[]>(
    `${baseUrl()}/Line/${id}/Status/${from}/to/${to}`,
    { cache: 'no-store' },
  );
}
