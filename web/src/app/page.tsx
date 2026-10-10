"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { listEpisodes, formatDuration, getGoogleLoginUrl } from "@/lib/api";
import type { Episode } from "@/lib/types";

const platformIcons: Record<string, string> = {
  youtube: "🎬",
  bilibili: "📺",
  spotify: "🎵",
  apple: "🍎",
  twitter: "𝕏",
  default: "🎙️",
};

function EpisodeCard({ episode }: { episode: Episode }) {
  const icon = platformIcons[episode.source.platform] || platformIcons.default;

  return (
    <Link
      href={`/episodes/${episode.id}`}
      className="group block rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-blue-400 dark:hover:border-blue-600 transition-all hover:shadow-lg overflow-hidden"
    >
      <div className="flex gap-4 p-4">
        {episode.source.thumbnailUrl ? (
          <img
            src={episode.source.thumbnailUrl}
            alt={episode.source.title}
            className="w-28 h-20 rounded-lg object-cover shrink-0"
          />
        ) : (
          <div className="w-28 h-20 rounded-lg bg-gradient-to-br from-blue-500 to-purple-600 flex items-center justify-center text-3xl shrink-0">
            {icon}
          </div>
        )}
        <div className="flex-1 min-w-0">
          <div className="flex items-start gap-2">
            <h2 className="font-semibold text-zinc-900 dark:text-zinc-100 line-clamp-2 group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
              {episode.source.title}
            </h2>
            {!episode.isRead && (
              <span className="shrink-0 mt-1 w-2 h-2 rounded-full bg-blue-500" />
            )}
          </div>
          <div className="flex items-center gap-2 mt-1 text-sm text-zinc-500 dark:text-zinc-400">
            <span>{episode.source.author}</span>
            <span>·</span>
            <span>{icon} {episode.source.platform}</span>
            <span>·</span>
            <span>{formatDuration(episode.source.duration)}</span>
          </div>
          {episode.summary && (
            <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-400 line-clamp-2">
              {episode.summary.oneLiner}
            </p>
          )}
          {episode.tags.length > 0 && (
            <div className="mt-2 flex gap-1 flex-wrap">
              {episode.tags.slice(0, 5).map((tag) => (
                <span
                  key={tag}
                  className="px-2 py-0.5 text-xs rounded-full bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400"
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      </div>
    </Link>
  );
}

export default function Home() {
  const [episodes, setEpisodes] = useState<Episode[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listEpisodes(50, 0)
      .then((data) => {
        setEpisodes(data.episodes);
        setTotal(data.total);
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, []);

  return (
    <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
      <header className="border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
        <div className="max-w-5xl mx-auto px-4 py-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <span className="text-2xl">🎙️</span>
            <h1 className="text-xl font-bold text-zinc-900 dark:text-zinc-100">
              Lumen Pod
            </h1>
            <span className="text-sm text-zinc-400">{total} episodes</span>
          </div>
          <div className="flex items-center gap-3">
            <Link
              href="/search"
              className="px-3 py-1.5 text-sm rounded-lg bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300 transition-colors"
            >
              🔍 Search
            </Link>
            <a
              href={getGoogleLoginUrl()}
              className="px-3 py-1.5 text-sm rounded-lg bg-blue-600 hover:bg-blue-700 text-white transition-colors"
            >
              Sign in
            </a>
          </div>
        </div>
      </header>

      <main className="max-w-5xl mx-auto px-4 py-8">
        {loading && (
          <div className="text-center py-20 text-zinc-400">Loading...</div>
        )}
        {error && (
          <div className="text-center py-20 text-red-500">Error: {error}</div>
        )}
        {!loading && !error && episodes.length === 0 && (
          <div className="text-center py-20">
            <p className="text-zinc-400 text-lg">No episodes yet</p>
            <p className="text-zinc-500 text-sm mt-2">
              Use the MCP tools or agent skills to process your first podcast.
            </p>
          </div>
        )}
        <div className="space-y-3">
          {episodes.map((ep) => (
            <EpisodeCard key={ep.id} episode={ep} />
          ))}
        </div>
      </main>
    </div>
  );
}
