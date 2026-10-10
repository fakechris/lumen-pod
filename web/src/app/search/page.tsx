"use client";

import { useState } from "react";
import Link from "next/link";
import { searchEpisodes, formatDuration } from "@/lib/api";
import type { Episode } from "@/lib/types";

export default function SearchPage() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Episode[]>([]);
  const [searched, setSearched] = useState(false);
  const [loading, setLoading] = useState(false);

  const handleSearch = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;
    setLoading(true);
    try {
      const data = await searchEpisodes(query);
      setResults(data.results);
      setSearched(true);
    } catch {
      setResults([]);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
      <header className="border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
        <div className="max-w-3xl mx-auto px-4 py-4">
          <Link href="/" className="text-sm text-blue-600 hover:underline">← Home</Link>
          <form onSubmit={handleSearch} className="mt-4 flex gap-2">
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search episodes, transcripts, summaries..."
              className="flex-1 px-4 py-2 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
              autoFocus
            />
            <button
              type="submit"
              disabled={loading}
              className="px-6 py-2 rounded-lg bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
            >
              {loading ? "..." : "Search"}
            </button>
          </form>
        </div>
      </header>

      <main className="max-w-3xl mx-auto px-4 py-6 space-y-3">
        {searched && results.length === 0 && (
          <p className="text-center text-zinc-400 py-12">No results found for &ldquo;{query}&rdquo;</p>
        )}
        {results.map((ep) => (
          <Link
            key={ep.id}
            href={`/episodes/${ep.id}`}
            className="block p-4 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-blue-400 transition-colors"
          >
            <h3 className="font-semibold text-zinc-900 dark:text-zinc-100">{ep.source.title}</h3>
            <div className="flex items-center gap-2 mt-1 text-sm text-zinc-500">
              <span>{ep.source.author}</span>
              <span>·</span>
              <span>{formatDuration(ep.source.duration)}</span>
            </div>
            {ep.summary && (
              <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-400 line-clamp-2">{ep.summary.oneLiner}</p>
            )}
          </Link>
        ))}
      </main>
    </div>
  );
}
