import { getApiConfig } from "../config";
import type { Episode } from "../types/messages";

async function apiFetch<T>(path: string): Promise<T> {
  const { baseUrl, token } = await getApiConfig();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }
  const res = await fetch(`${baseUrl}${path}`, { headers });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || `API error: ${res.status}`);
  }
  return res.json();
}

export async function lookupEpisodeByUrl(
  url: string
): Promise<Episode | null> {
  try {
    return await apiFetch<Episode>(
      `/api/episodes/lookup?url=${encodeURIComponent(url)}`
    );
  } catch {
    return null;
  }
}

export async function getEpisode(id: string): Promise<Episode> {
  return apiFetch<Episode>(`/api/episodes/${id}`);
}

export function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0)
    return `${h}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function formatTimestamp(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}
