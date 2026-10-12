import type { Episode, User } from "./types";

const API_BASE = process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:3210";

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    credentials: "include",
    headers: { "Content-Type": "application/json", ...init?.headers },
    ...init,
  });
  if (!res.ok) {
    if (res.status === 401 && typeof window !== "undefined") {
      window.location.href = getGoogleLoginUrl();
      return new Promise(() => {});
    }
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || `API error: ${res.status}`);
  }
  return res.json();
}

export function getExportUrl(id: string, format: string): string {
  return `${API_BASE}/api/episodes/${id}/export/${format}`;
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

// ---- Chat ----

export interface ChatMsg {
  id: string;
  episodeId: string;
  role: string;
  content: string;
  createdAt: string;
}

export async function sendChatMessage(
  episodeId: string,
  content: string
): Promise<{ userMessage: ChatMsg; assistantMessage: ChatMsg }> {
  return apiFetch(`/api/episodes/${episodeId}/chat`, {
    method: "POST",
    body: JSON.stringify({ role: "user", content }),
  });
}

export async function listChatMessages(episodeId: string): Promise<{ messages: ChatMsg[] }> {
  return apiFetch(`/api/episodes/${episodeId}/chat`);
}

// ---- Chapter Deep Read ----

export async function requestChapterDeepRead(
  episodeId: string,
  chapterIndex: number
): Promise<{ content: string }> {
  return apiFetch(`/api/episodes/${episodeId}/chapters/${chapterIndex}/deep-read`, {
    method: "POST",
  });
}

// ---- Formatting helpers ----

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
