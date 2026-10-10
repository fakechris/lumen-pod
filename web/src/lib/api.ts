import type { Episode, User } from "./types";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:3210";

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    credentials: "include",
    headers: { "Content-Type": "application/json", ...init?.headers },
    ...init,
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || `API error: ${res.status}`);
  }
  return res.json();
}

export async function getMe(): Promise<User> {
  return apiFetch("/api/me");
}

export async function listEpisodes(
  limit = 20,
  offset = 0
): Promise<{ total: number; episodes: Episode[] }> {
  return apiFetch(`/api/episodes?limit=${limit}&offset=${offset}`);
}

export async function getEpisode(id: string): Promise<Episode> {
  return apiFetch(`/api/episodes/${id}`);
}

export async function searchEpisodes(
  q: string,
  limit = 10
): Promise<{ results: Episode[] }> {
  return apiFetch(`/api/search?q=${encodeURIComponent(q)}&limit=${limit}`);
}

export async function updateEpisode(
  id: string,
  updates: Record<string, unknown>
): Promise<Episode> {
  return apiFetch(`/api/episodes/${id}`, {
    method: "PATCH",
    body: JSON.stringify(updates),
  });
}

export async function deleteEpisode(id: string): Promise<void> {
  await apiFetch(`/api/episodes/${id}`, { method: "DELETE" });
}

export function formatDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function formatTimestamp(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function getGoogleLoginUrl(): string {
  return `${API_BASE}/auth/google`;
}
