import type { Episode, User } from "./types";

// Client-side: use relative URLs — Caddy (prod) or Next.js rewrites (dev) proxy to API
// Server-side: use env var for SSR fetch if needed
const API_BASE =
  typeof window !== "undefined"
    ? ""
    : (process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:3210");

async function apiFetch<T>(
  path: string,
  init?: RequestInit & { skipAuthRedirect?: boolean },
): Promise<T> {
  const { skipAuthRedirect, ...fetchInit } = init ?? {};
  const res = await fetch(`${API_BASE}${path}`, {
    credentials: "include",
    headers: { "Content-Type": "application/json", ...fetchInit?.headers },
    ...fetchInit,
  });
  if (!res.ok) {
    if (res.status === 401 && typeof window !== "undefined" && !skipAuthRedirect) {
      window.location.href = getGoogleLoginUrl();
      return new Promise(() => {});
    }
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new Error(body.error || `API error: ${res.status}`);
  }
  return res.json();
}

export function getExportUrl(id: string, format: string): string {
  const base = typeof window !== "undefined" ? "" : API_BASE;
  return `${base}/api/episodes/${id}/export/${format}`;
}

export async function getMe(): Promise<User> {
  return apiFetch("/api/me");
}

export async function listEpisodes(
  limit = 20,
  offset = 0
): Promise<{ total: number; episodes: Episode[] }> {
  return apiFetch(`/api/episodes?limit=${limit}&offset=${offset}`, { skipAuthRedirect: true });
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
  chapterIndex: number,
  opts?: { lang?: string; regenerate?: boolean }
): Promise<{ content: string; cached?: boolean }> {
  const params = new URLSearchParams();
  if (opts?.lang) params.set("lang", opts.lang);
  if (opts?.regenerate) params.set("regenerate", "true");
  const qs = params.toString();
  return apiFetch(`/api/episodes/${episodeId}/chapters/${chapterIndex}/deep-read${qs ? `?${qs}` : ""}`, {
    method: "POST",
  });
}

// ---- Batch Deep Read ----

export interface BatchStatus {
  status: "idle" | "started" | "running" | "complete" | "cancelled" | "cancelling";
  currentIdx?: number | null;
  total?: number;
  completed?: number;
  failed?: number;
  elapsedMs?: number;
  pending?: number;
}

export async function startBatchDeepRead(episodeId: string, lang?: string): Promise<BatchStatus> {
  const qs = lang ? `?lang=${encodeURIComponent(lang)}` : "";
  return apiFetch(`/api/episodes/${episodeId}/deep-read-all${qs}`, { method: "POST" });
}

export async function getBatchDeepReadStatus(episodeId: string): Promise<BatchStatus> {
  return apiFetch(`/api/episodes/${episodeId}/deep-read-all`);
}

export async function cancelBatchDeepRead(episodeId: string): Promise<BatchStatus> {
  return apiFetch(`/api/episodes/${episodeId}/deep-read-all`, { method: "DELETE" });
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
  // In browser, use relative URL so Caddy proxies to the API
  if (typeof window !== "undefined") return "/auth/google";
  return `${API_BASE}/auth/google`;
}
