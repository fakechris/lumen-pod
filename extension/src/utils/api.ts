import { getApiConfig } from "../config";
import type { Article, Episode } from "../types/messages";

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const { baseUrl, token } = await getApiConfig();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }
  const res = await fetch(`${baseUrl}${path}`, {
    credentials: "include",
    ...init,
    headers: { ...headers, ...(init?.headers as Record<string, string> | undefined) },
  });
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

export async function submitEpisodeUrl(
  url: string,
  title?: string,
  visibility: "public" | "private" = "public"
): Promise<{ id: string; url: string; visibility?: string; status: string; message: string }> {
  return apiFetch(`/api/episodes/submit-url`, {
    method: "POST",
    body: JSON.stringify({ url, title, visibility }),
  });
}

export async function requestEpisodeArticle(
  episodeId: string,
  opts?: { lang?: string; focus?: string; regenerate?: boolean }
): Promise<{ article: Article; cached?: boolean }> {
  const startRes = await apiFetch<{
    status?: string;
    article?: Article;
    cached?: boolean;
    error?: string;
  }>(`/api/episodes/${episodeId}/article`, {
    method: "POST",
    body: JSON.stringify({
      lang: opts?.lang ?? "zh",
      focus: opts?.focus || undefined,
      regenerate: opts?.regenerate ?? false,
      async: true,
    }),
  });

  if (startRes.article) {
    return { article: startRes.article, cached: startRes.cached };
  }

  const maxPolls = 1800; // up to 90 minutes (3s interval) for 3-4 hour episodes
  let consecutiveNetErrors = 0;
  for (let i = 0; i < maxPolls; i++) {
    await new Promise((r) => setTimeout(r, 3000));
    let pollRes: { status: string; article?: Article; error?: string };
    try {
      pollRes = await apiFetch<{
        status: string;
        article?: Article;
        error?: string;
      }>(`/api/episodes/${episodeId}/article`);
      consecutiveNetErrors = 0;
    } catch (err) {
      consecutiveNetErrors++;
      if (consecutiveNetErrors >= 3) throw err;
      continue;
    }

    if (pollRes.status === "complete" && pollRes.article) {
      return { article: pollRes.article, cached: false };
    }
    if (pollRes.status === "error") {
      throw new Error(pollRes.error || "Article generation failed");
    }
    if (pollRes.status === "idle") {
      throw new Error("Article generation stopped unexpectedly");
    }
  }
  throw new Error("Article generation timed out");
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
