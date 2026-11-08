"use client";

import { useEffect, useState, useCallback, useRef } from "react";
import Link from "next/link";
import { listEpisodes, listMyTags, setFavorite, setMyTags, formatDuration, formatDate, getGoogleLoginUrl } from "@/lib/api";
import { useAuth } from "@/lib/auth";
import type { Episode } from "@/lib/types";

const platformIcons: Record<string, string> = {
  youtube: "🎬",
  bilibili: "📺",
  spotify: "🎵",
  apple: "🍎",
  twitter: "𝕏",
  xiaoyuzhou: "🎧",
  default: "🎙️",
};

const PAGE_SIZE = 50;

const statusColors: Record<string, string> = {
  pending: "bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400",
  processing: "bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400",
  done: "bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400",
  error: "bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400",
};

function EpisodeCard({
  episode,
  onToggleFavorite,
  onRemoveMyTag,
}: {
  episode: Episode;
  onToggleFavorite: (id: string, favorited: boolean) => void;
  onRemoveMyTag: (id: string, tag: string) => void;
}) {
  const icon = platformIcons[episode.source.platform] || platformIcons.default;
  const isPending = episode.processing.status === "pending";

  return (
    <div
      className="group relative rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 hover:border-blue-400 dark:hover:border-blue-600 transition-all hover:shadow-lg overflow-hidden"
    >
      <button
        aria-label={episode.isFavorite ? "取消收藏" : "收藏"}
        title={episode.isFavorite ? "取消收藏" : "收藏"}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onToggleFavorite(episode.id, !episode.isFavorite);
        }}
        className={`absolute top-3 right-3 z-10 w-7 h-7 flex items-center justify-center rounded-full text-base leading-none transition-all ${
          episode.isFavorite
            ? "text-amber-400 opacity-100"
            : "text-zinc-300 dark:text-zinc-600 opacity-0 group-hover:opacity-100 hover:text-amber-400"
        }`}
      >
        {episode.isFavorite ? "★" : "☆"}
      </button>
      <Link href={`/episodes/${episode.id}`} className="block">
      <div className="flex gap-4 p-4">
        {episode.source.thumbnailUrl ? (
          <img
            src={episode.source.thumbnailUrl}
            alt={episode.source.title}
            className="w-28 h-20 rounded-lg object-cover shrink-0"
          />
        ) : (
          <div className={`w-28 h-20 rounded-lg flex items-center justify-center text-3xl shrink-0 ${isPending ? "bg-gradient-to-br from-amber-400 to-orange-500" : "bg-gradient-to-br from-blue-500 to-purple-600"}`}>
            {isPending ? "⏳" : icon}
          </div>
        )}
        <div className="flex-1 min-w-0">
          <div className="flex items-start gap-2 pr-8">
            <h2 className="font-semibold text-zinc-900 dark:text-zinc-100 line-clamp-2 group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">
              {episode.source.title}
            </h2>
            {!episode.isRead && (
              <span className="shrink-0 mt-1 w-2 h-2 rounded-full bg-blue-500" />
            )}
          </div>
          <div className="flex items-center gap-2 mt-1 text-sm text-zinc-500 dark:text-zinc-400 flex-wrap">
            {episode.source.author && <span>{episode.source.author}</span>}
            {episode.source.author && <span>·</span>}
            <span>{icon} {episode.source.platform}</span>
            {formatDate(episode.source.publishedAt) && (
              <>
                <span>·</span>
                <span title="原发布时间">{formatDate(episode.source.publishedAt)}</span>
              </>
            )}
            {episode.source.duration > 0 && (
              <>
                <span>·</span>
                <span>{formatDuration(episode.source.duration)}</span>
              </>
            )}
            <span className={`px-1.5 py-0.5 text-xs rounded-full ${statusColors[episode.processing.status] || statusColors.pending}`}>
              {episode.processing.status}
            </span>
            {episode.visibility === "private" && (
              <span className="px-1.5 py-0.5 text-xs rounded-full bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-300 font-medium">
                🔒 私密
              </span>
            )}
            {episode.isRead && (
              <span className="px-1.5 py-0.5 text-xs rounded-full bg-emerald-50 dark:bg-emerald-950/40 text-emerald-600 dark:text-emerald-400">
                ✓ 已读
              </span>
            )}
          </div>
          {episode.summary && (
            <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-400 line-clamp-2">
              {episode.summary.oneLiner}
            </p>
          )}
          {isPending && (
            <p className="mt-2 text-sm text-amber-600 dark:text-amber-400">
              Awaiting agent processing — use MCP or skill to process this episode
            </p>
          )}
          {(episode.tags.length > 0 || episode.myTags.length > 0) && (
            <div className="mt-2 flex gap-1 flex-wrap">
              {episode.myTags.slice(0, 5).map((tag) => (
                <span
                  key={`my-${tag}`}
                  title={`我的标签：${tag}（点击移除）`}
                  className="group/mytag inline-flex items-center gap-0.5 pl-2 pr-1 py-0.5 text-xs rounded-full bg-blue-50 dark:bg-blue-950/50 text-blue-600 dark:text-blue-300 border border-blue-200 dark:border-blue-900"
                >
                  {tag}
                  <button
                    aria-label={`移除个人标签 ${tag}`}
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      onRemoveMyTag(episode.id, tag);
                    }}
                    className="w-3.5 h-3.5 flex items-center justify-center rounded-full text-blue-400 hover:bg-blue-200 dark:hover:bg-blue-800 leading-none"
                  >
                    ×
                  </button>
                </span>
              ))}
              {episode.tags.slice(0, 5).map((tag) => (
                <span
                  key={tag}
                  title="共享标签"
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
    </div>
  );
}

function SubmitUrlForm({
  onSubmitted,
  canPublishPrivate,
}: {
  onSubmitted: () => void;
  canPublishPrivate: boolean;
}) {
  const [url, setUrl] = useState("");
  const [visibility, setVisibility] = useState<"public" | "private">("public");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const handleSubmit = async () => {
    const trimmed = url.trim();
    if (!trimmed) return;

    setSubmitting(true);
    setError(null);
    setSuccess(null);

    try {
      const res = await fetch("/api/episodes/submit-url", {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ url: trimmed, visibility }),
      });
      const data = await res.json();
      if (res.ok) {
        setSuccess(
          `✓ Submitted (${visibility === "private" ? "🔒 Private" : "🌐 Public"})! Episode ID: ${data.id}`
        );
        setUrl("");
        onSubmitted();
      } else {
        setError(data.error || "Failed to submit");
      }
    } catch {
      setError("Network error");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="flex gap-2 flex-wrap sm:flex-nowrap">
        <input
          type="url"
          placeholder="Paste a podcast or video URL (YouTube, Bilibili, Spotify, Apple...)"
          value={url}
          onChange={(e) => { setUrl(e.target.value); setError(null); setSuccess(null); }}
          onKeyDown={(e) => e.key === "Enter" && handleSubmit()}
          disabled={submitting}
          className="flex-1 min-w-[220px] px-4 py-2.5 rounded-xl border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 text-sm focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none disabled:opacity-50 placeholder:text-zinc-400"
        />
        <select
          value={visibility}
          onChange={(e) => setVisibility(e.target.value as "public" | "private")}
          disabled={submitting}
          className="px-3 py-2.5 rounded-xl border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-700 dark:text-zinc-200 text-sm outline-none focus:ring-2 focus:ring-blue-500"
          title={
            canPublishPrivate
              ? "选择公开或仅自己可见的私密视频"
              : "发布私密视频需管理员授权 (can_publish_private)"
          }
        >
          <option value="public">🌐 公开 (Public)</option>
          <option value="private" disabled={!canPublishPrivate}>
            🔒 私密 (仅自己可见){!canPublishPrivate ? " — 需管理员授权" : ""}
          </option>
        </select>
        <button
          onClick={handleSubmit}
          disabled={submitting || !url.trim()}
          className="px-5 py-2.5 text-sm font-medium rounded-xl bg-blue-600 hover:bg-blue-700 disabled:bg-zinc-300 dark:disabled:bg-zinc-700 text-white transition-colors whitespace-nowrap"
        >
          {submitting ? "Submitting..." : "➕ Add Episode"}
        </button>
      </div>
      {error && (
        <p className="text-sm text-red-500">{error}</p>
      )}
      {success && (
        <p className="text-sm text-green-600 dark:text-green-400">{success}</p>
      )}
      <p className="text-xs text-zinc-400">
        Supported: YouTube, Bilibili, Spotify, Apple Podcasts, Twitter/X, 小宇宙, and 1000+ more via yt-dlp
      </p>
    </div>
  );
}

export default function Home() {
  const { user, loading: authLoading, logout } = useAuth();
  const [episodes, setEpisodes] = useState<Episode[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filterMode, setFilterMode] = useState<"all" | "unread" | "favorites">("all");
  const [sortMode, setSortMode] = useState<"recent" | "unread">("recent");
  const [tagFilter, setTagFilter] = useState<string | null>(null);
  const [myTagCloud, setMyTagCloud] = useState<[string, number][]>([]);
  const offsetRef = useRef(0);
  const reqSeqRef = useRef(0);
  const loadingMoreRef = useRef(false);

  // mode "reset"：回到第一页整体替换（初始加载/筛选切换/提交新节目）；
  // mode "more"：从 offsetRef 续拉下一页，追加去重后并入列表。
  const fetchEpisodes = useCallback(
    (mode: "reset" | "more" = "reset") => {
      const seq = ++reqSeqRef.current;
      if (mode === "more") {
        if (loadingMoreRef.current) return;
        loadingMoreRef.current = true;
        setLoadingMore(true);
      } else {
        offsetRef.current = 0;
        setLoading(true);
      }
      listEpisodes(PAGE_SIZE, mode === "more" ? offsetRef.current : 0, {
        filter: filterMode === "all" ? undefined : filterMode,
        tag: tagFilter ?? undefined,
        sort: sortMode,
      })
        .then((data) => {
          if (seq !== reqSeqRef.current) return;
          setTotal(data.total);
          setError(null);
          if (mode === "more") {
            offsetRef.current += data.episodes.length;
            setEpisodes((prev) => {
              const seen = new Set(prev.map((ep) => ep.id));
              return [...prev, ...data.episodes.filter((ep) => !seen.has(ep.id))];
            });
          } else {
            offsetRef.current = data.episodes.length;
            setEpisodes(data.episodes);
          }
        })
        .catch((e) => {
          if (seq !== reqSeqRef.current) return;
          if (e.message?.includes("401")) {
            setError(null);
          } else {
            setError(e.message);
          }
        })
        .finally(() => {
          if (mode === "more") {
            loadingMoreRef.current = false;
            setLoadingMore(false);
          } else {
            setLoading(false);
          }
        });
    },
    [filterMode, tagFilter, sortMode]
  );

  const refreshTagCloud = useCallback(() => {
    listMyTags()
      .then((res) => setMyTagCloud(res.tags || []))
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!authLoading && user) {
      fetchEpisodes("reset");
      refreshTagCloud();
    } else if (!authLoading) {
      setLoading(false);
    }
  }, [authLoading, user, fetchEpisodes, refreshTagCloud, filterMode, tagFilter, sortMode]);

  const handleToggleFavorite = useCallback(
    (id: string, favorited: boolean) => {
      setEpisodes((prev) =>
        prev.map((ep) => (ep.id === id ? { ...ep, isFavorite: favorited } : ep))
      );
      setFavorite(id, favorited).catch(() => {
        // revert on failure
        setEpisodes((prev) =>
          prev.map((ep) => (ep.id === id ? { ...ep, isFavorite: !favorited } : ep))
        );
      });
    },
    []
  );

  const handleRemoveMyTag = useCallback(
    (id: string, tag: string) => {
      const target = episodes.find((ep) => ep.id === id);
      if (!target) return;
      const next = target.myTags.filter((t) => t !== tag);
      setEpisodes((prev) =>
        prev.map((ep) => (ep.id === id ? { ...ep, myTags: next } : ep))
      );
      setMyTags(id, next)
        .then(() => refreshTagCloud())
        .catch(() => fetchEpisodes("reset"));
    },
    [episodes, fetchEpisodes, refreshTagCloud]
  );

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
            {authLoading ? null : user ? (
              <div className="flex items-center gap-2">
                {user.role === "admin" && (
                  <Link
                    href="/admin"
                    className="px-3 py-1.5 text-sm rounded-lg bg-amber-100 dark:bg-amber-900/30 hover:bg-amber-200 dark:hover:bg-amber-900/50 text-amber-700 dark:text-amber-400 transition-colors"
                  >
                    ⚙️ Admin
                  </Link>
                )}
                {user.avatar_url ? (
                  <img
                    src={user.avatar_url}
                    alt={user.name || user.email}
                    className="w-8 h-8 rounded-full"
                    title={user.name || user.email}
                  />
                ) : (
                  <div className="w-8 h-8 rounded-full bg-blue-600 flex items-center justify-center text-white text-sm font-medium">
                    {(user.name || user.email)[0].toUpperCase()}
                  </div>
                )}
                <button
                  onClick={logout}
                  className="px-3 py-1.5 text-sm rounded-lg bg-zinc-100 dark:bg-zinc-800 hover:bg-zinc-200 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300 transition-colors"
                >
                  Sign out
                </button>
              </div>
            ) : (
              <a
                href={getGoogleLoginUrl()}
                className="px-3 py-1.5 text-sm rounded-lg bg-blue-600 hover:bg-blue-700 text-white transition-colors"
              >
                Sign in
              </a>
            )}
          </div>
        </div>
      </header>

      {user && !user.approved && (
        <div className="bg-amber-50 dark:bg-amber-950 border-b border-amber-200 dark:border-amber-800">
          <div className="max-w-5xl mx-auto px-4 py-3 text-sm text-amber-700 dark:text-amber-400 text-center">
            ⏳ Your account is pending admin approval. You can browse but some features are restricted.
          </div>
        </div>
      )}

      <main className="max-w-5xl mx-auto px-4 py-8">
        {/* URL Submit Box — shown to logged-in approved users */}
        {user && user.approved && (
          <div className="mb-8 p-5 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 shadow-sm">
            <h2 className="text-base font-semibold text-zinc-900 dark:text-zinc-100 mb-3">
              Add a Podcast or Video
            </h2>
            <SubmitUrlForm
              onSubmitted={fetchEpisodes}
              canPublishPrivate={Boolean(user.role === "admin" || user.can_publish_private)}
            />
          </div>
        )}

        {loading && (
          <div className="text-center py-20 text-zinc-400">Loading...</div>
        )}
        {error && (
          <div className="text-center py-20 text-red-500">Error: {error}</div>
        )}
        {!loading && !error && episodes.length === 0 && (
          <div className="text-center py-20">
            <p className="text-4xl mb-4">🎙️</p>
            <p className="text-zinc-400 text-lg">
              {filterMode === "favorites"
                ? "还没有收藏"
                : filterMode === "unread"
                  ? "没有未读的节目"
                  : tagFilter
                    ? `没有标签为「${tagFilter}」的节目`
                    : "No episodes yet"}
            </p>
            {user && !filterMode && !tagFilter && (
              <p className="text-zinc-500 text-sm mt-2">
                Paste a URL above, or use the MCP agent to process your first podcast.
              </p>
            )}
            {!user && (
              <p className="text-zinc-500 text-sm mt-2">
                Sign in to add your first podcast.
              </p>
            )}
          </div>
        )}
        {user && (
          <div className="mb-4 flex items-center gap-2 flex-wrap text-sm">
            {([
              ["all", "全部"],
              ["unread", "未读"],
              ["favorites", "⭐ 收藏"],
            ] as const).map(([mode, label]) => (
              <button
                key={mode}
                onClick={() => setFilterMode(mode)}
                className={`px-3 py-1.5 rounded-lg transition-colors ${
                  filterMode === mode
                    ? "bg-blue-600 text-white"
                    : "bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300 hover:bg-zinc-200 dark:hover:bg-zinc-700"
                }`}
              >
                {label}
              </button>
            ))}
            <select
              value={tagFilter ?? ""}
              onChange={(e) => setTagFilter(e.target.value || null)}
              className="px-2.5 py-1.5 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-700 dark:text-zinc-200 outline-none focus:ring-2 focus:ring-blue-500"
            >
              <option value="">我的标签：全部</option>
              {myTagCloud.map(([tag, count]) => (
                <option key={tag} value={tag}>
                  {tag} ({count})
                </option>
              ))}
            </select>
            <select
              value={sortMode}
              onChange={(e) => setSortMode(e.target.value as "recent" | "unread")}
              className="px-2.5 py-1.5 rounded-lg border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-700 dark:text-zinc-200 outline-none focus:ring-2 focus:ring-blue-500"
              title="排序方式"
            >
              <option value="recent">最新优先</option>
              <option value="unread">未读优先</option>
            </select>
          </div>
        )}
        <div className="space-y-3">
          {episodes.map((ep) => (
            <EpisodeCard
              key={ep.id}
              episode={ep}
              onToggleFavorite={handleToggleFavorite}
              onRemoveMyTag={handleRemoveMyTag}
            />
          ))}
        </div>
        {!loading && !error && episodes.length > 0 && episodes.length < total && (
          <div className="mt-6 text-center">
            <button
              onClick={() => fetchEpisodes("more")}
              disabled={loadingMore}
              className="px-5 py-2.5 text-sm font-medium rounded-xl border border-zinc-300 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-50 transition-colors"
            >
              {loadingMore ? "加载中…" : `加载更多（剩余 ${total - episodes.length}）`}
            </button>
          </div>
        )}
      </main>
    </div>
  );
}
