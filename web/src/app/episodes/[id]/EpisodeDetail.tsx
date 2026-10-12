"use client";

import { useEffect, useState, useRef, useCallback, useMemo } from "react";
import Link from "next/link";
import { getEpisode, formatTimestamp, formatDuration, getExportUrl } from "@/lib/api";
import type { Episode } from "@/lib/types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function extractYouTubeId(url: string): string | null {
  const m =
    url.match(/(?:youtube\.com\/watch\?.*v=|youtu\.be\/)([\w-]{11})/) ??
    url.match(/youtube\.com\/embed\/([\w-]{11})/);
  return m ? m[1] : null;
}

function extractBilibiliId(url: string): string | null {
  const m = url.match(/bilibili\.com\/video\/(BV[\w]+)/i) ?? url.match(/b23\.tv\/(BV[\w]+)/i);
  return m ? m[1] : null;
}

const platformIcons: Record<string, string> = {
  youtube: "🎬", bilibili: "📺", spotify: "🎵",
  apple: "🍎", twitter: "𝕏", xiaoyuzhou: "🎧", default: "🎙️",
};

// ---------------------------------------------------------------------------
// YouTube Player with IFrame API
// ---------------------------------------------------------------------------

interface PlayerHandle {
  seekTo: (t: number) => void;
  getCurrentTime: () => number;
}

function YouTubePlayer({
  videoId,
  onReady,
  onTimeUpdate,
}: {
  videoId: string;
  onReady?: (h: PlayerHandle) => void;
  onTimeUpdate?: (t: number) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const playerRef = useRef<any>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    let cancelled = false;

    function init() {
      if (cancelled || !containerRef.current) return;
      const div = document.createElement("div");
      div.id = `yt-player-${videoId}`;
      containerRef.current.innerHTML = "";
      containerRef.current.appendChild(div);

      playerRef.current = new (window as any).YT.Player(div.id, {
        videoId,
        playerVars: { rel: 0, modestbranding: 1, cc_load_policy: 0 },
        events: {
          onReady: () => {
            const handle: PlayerHandle = {
              seekTo: (t) => playerRef.current?.seekTo(t, true),
              getCurrentTime: () => playerRef.current?.getCurrentTime() ?? 0,
            };
            onReady?.(handle);
            intervalRef.current = setInterval(() => {
              if (playerRef.current?.getPlayerState?.() === 1) {
                onTimeUpdate?.(playerRef.current.getCurrentTime());
              }
            }, 250);
          },
        },
      });
    }

    if ((window as any).YT?.Player) {
      init();
    } else {
      const tag = document.createElement("script");
      tag.src = "https://www.youtube.com/iframe_api";
      document.head.appendChild(tag);
      (window as any).onYouTubeIframeAPIReady = init;
    }

    return () => {
      cancelled = true;
      if (intervalRef.current) clearInterval(intervalRef.current);
      playerRef.current?.destroy?.();
    };
  }, [videoId]);

  return <div ref={containerRef} className="w-full aspect-video rounded-xl overflow-hidden bg-black" />;
}

function BilibiliPlayer({ bvid }: { bvid: string }) {
  return (
    <iframe
      src={`https://player.bilibili.com/player.html?bvid=${bvid}&high_quality=1&danmaku=0`}
      className="w-full aspect-video rounded-xl"
      allowFullScreen
      sandbox="allow-scripts allow-same-origin allow-popups"
    />
  );
}

// ---------------------------------------------------------------------------
// Markmap (dynamic import)
// ---------------------------------------------------------------------------

function MarkmapView({ markdown }: { markdown: string }) {
  const svgRef = useRef<SVGSVGElement>(null);
  const mmRef = useRef<any>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const { Transformer } = await import("markmap-lib");
      const { Markmap } = await import("markmap-view");
      if (cancelled || !svgRef.current) return;

      const transformer = new Transformer();
      const { root } = transformer.transform(markdown);

      svgRef.current.innerHTML = "";
      mmRef.current = Markmap.create(svgRef.current, { autoFit: true, duration: 300 }, root);
    })();
    return () => { cancelled = true; };
  }, [markdown]);

  return (
    <div className="w-full rounded-xl border border-zinc-200 dark:border-zinc-800 overflow-hidden markmap-wrap" style={{ height: "min(70vh, 600px)", background: "#fff" }}>
      <svg ref={svgRef} className="w-full h-full" />
      <style>{`.markmap-wrap .markmap-node text, .markmap-wrap .markmap-foreign div { color: #1a1a2e !important; } .markmap-wrap .markmap-link { opacity: 0.7; }`}</style>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

const SPEAKER_COLORS = [
  "text-blue-600 dark:text-blue-400",
  "text-emerald-600 dark:text-emerald-400",
  "text-amber-600 dark:text-amber-400",
  "text-rose-600 dark:text-rose-400",
  "text-purple-600 dark:text-purple-400",
  "text-cyan-600 dark:text-cyan-400",
];

function speakerColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) hash = ((hash << 5) - hash + name.charCodeAt(i)) | 0;
  return SPEAKER_COLORS[Math.abs(hash) % SPEAKER_COLORS.length];
}

const TABS = ["Summary", "Chapters", "Mindmap", "Transcript", "Keywords", "Highlights", "Q&A"] as const;
type TabName = (typeof TABS)[number];

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export default function EpisodeDetail({ id }: { id: string }) {
  const [episode, setEpisode] = useState<Episode | null>(null);
  const [activeTab, setActiveTab] = useState<TabName>("Summary");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const [currentTime, setCurrentTime] = useState(0);
  const playerHandleRef = useRef<PlayerHandle | null>(null);

  useEffect(() => {
    if (!id) return;
    getEpisode(id)
      .then(setEpisode)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [id]);

  const seekTo = useCallback((time: number) => {
    playerHandleRef.current?.seekTo(time);
  }, []);

  const youtubeId = useMemo(() => episode ? extractYouTubeId(episode.source.url) : null, [episode]);
  const bilibiliId = useMemo(() => episode ? extractBilibiliId(episode.source.url) : null, [episode]);
  const hasVideo = !!(youtubeId || bilibiliId);

  if (loading) return <div className="min-h-screen flex items-center justify-center text-zinc-400">Loading...</div>;
  if (error) return <div className="min-h-screen flex items-center justify-center text-red-500">{error}</div>;
  if (!episode) return null;

  const icon = platformIcons[episode.source.platform] || platformIcons.default;

  return (
    <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950">
      {/* Header */}
      <header className="border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
        <div className="max-w-7xl mx-auto px-4 py-4">
          <Link href="/" className="text-sm text-blue-600 hover:underline mb-2 inline-block">← Back</Link>
          <h1 className="text-xl font-bold text-zinc-900 dark:text-zinc-100 mt-1">{episode.source.title}</h1>
          <div className="flex items-center gap-2 mt-1.5 text-sm text-zinc-500 flex-wrap">
            <span>{icon} {episode.source.platform}</span>
            {episode.source.author && <><span>·</span><span>{episode.source.author}</span></>}
            <span>·</span><span>{formatDuration(episode.source.duration)}</span>
            {episode.processing.llmModel && <><span>·</span><span>🤖 {episode.processing.llmModel}</span></>}
            {episode.tags.length > 0 && episode.tags.map((t) => (
              <span key={t} className="px-2 py-0.5 text-xs rounded-full bg-zinc-100 dark:bg-zinc-800">{t}</span>
            ))}
            <a href={episode.source.url} target="_blank" rel="noopener" className="text-blue-500 hover:underline ml-1">
              Source ↗
            </a>
            <span className="text-zinc-300 dark:text-zinc-600">|</span>
            <a href={getExportUrl(id, "markdown")} className="text-blue-500 hover:underline text-xs">📝 MD</a>
            <a href={getExportUrl(id, "srt")} className="text-blue-500 hover:underline text-xs">📄 SRT</a>
            <a href={getExportUrl(id, "json")} className="text-blue-500 hover:underline text-xs">📦 JSON</a>
          </div>
        </div>
      </header>

      {/* Two-column layout */}
      <div className="max-w-7xl mx-auto px-4 py-6">
        <div className={hasVideo ? "flex flex-col lg:flex-row gap-6" : ""}>

          {/* Left Column: Video + Chapters */}
          {hasVideo && (
            <div className="lg:w-[480px] xl:w-[560px] shrink-0 space-y-4 lg:sticky lg:top-4 lg:self-start">
              {youtubeId && (
                <YouTubePlayer
                  videoId={youtubeId}
                  onReady={(h) => { playerHandleRef.current = h; }}
                  onTimeUpdate={setCurrentTime}
                />
              )}
              {bilibiliId && <BilibiliPlayer bvid={bilibiliId} />}

              {/* Mini chapters under video */}
              {episode.chapters.length > 0 && (
                <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-3 max-h-64 overflow-y-auto">
                  <h3 className="text-xs font-semibold text-zinc-400 uppercase tracking-wider mb-2">Chapters</h3>
                  {episode.chapters.map((ch, i) => {
                    const isActive = currentTime >= ch.startTime && currentTime < ch.endTime;
                    return (
                      <button
                        key={i}
                        onClick={() => seekTo(ch.startTime)}
                        className={`w-full text-left px-2.5 py-1.5 rounded-lg text-sm transition-colors flex items-center gap-2 ${
                          isActive
                            ? "bg-blue-50 dark:bg-blue-950 text-blue-700 dark:text-blue-400"
                            : "hover:bg-zinc-50 dark:hover:bg-zinc-800 text-zinc-700 dark:text-zinc-300"
                        }`}
                      >
                        <span className="font-mono text-xs text-zinc-400 shrink-0 w-10">{formatTimestamp(ch.startTime)}</span>
                        <span className="truncate">{ch.title}</span>
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {/* Right Column: Tabs + Content */}
          <div className="flex-1 min-w-0">
            {/* Tab Bar */}
            <div className="sticky top-0 z-10 bg-zinc-50 dark:bg-zinc-950 pb-1">
              <div className="flex gap-1 overflow-x-auto bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-1">
                {TABS.map((tab) => (
                  <button
                    key={tab}
                    onClick={() => setActiveTab(tab)}
                    className={`px-4 py-2 text-sm font-medium rounded-lg whitespace-nowrap transition-colors ${
                      activeTab === tab
                        ? "bg-blue-600 text-white shadow-sm"
                        : "text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800"
                    }`}
                  >
                    {tab}
                  </button>
                ))}
              </div>
            </div>

            {/* Tab Content */}
            <div className="mt-4">
              {activeTab === "Summary" && <SummaryTab episode={episode} />}
              {activeTab === "Chapters" && <ChaptersTab episode={episode} currentTime={currentTime} seekTo={seekTo} />}
              {activeTab === "Mindmap" && <MindmapTab episode={episode} />}
              {activeTab === "Transcript" && <TranscriptTab episode={episode} currentTime={currentTime} seekTo={seekTo} />}
              {activeTab === "Keywords" && <KeywordsTab episode={episode} />}
              {activeTab === "Highlights" && <HighlightsTab episode={episode} seekTo={seekTo} />}
              {activeTab === "Q&A" && <QATab episode={episode} />}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Tab Components
// ---------------------------------------------------------------------------

function SummaryTab({ episode }: { episode: Episode }) {
  if (!episode.summary) return <EmptyState text="No summary available" />;
  const s = episode.summary;
  return (
    <div className="space-y-6">
      {/* One-liner hero */}
      <div className="p-5 rounded-xl bg-gradient-to-br from-blue-50 to-indigo-50 dark:from-blue-950/50 dark:to-indigo-950/50 border border-blue-100 dark:border-blue-900">
        <p className="text-lg font-semibold text-blue-900 dark:text-blue-100">{s.oneLiner}</p>
      </div>

      {/* Abstract */}
      <div className="prose dark:prose-invert max-w-none">
        <p className="text-zinc-700 dark:text-zinc-300 leading-relaxed whitespace-pre-wrap">{s.abstract}</p>
      </div>

      {/* Key Takeaways */}
      {s.keyTakeaways.length > 0 && (
        <section className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-5">
          <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 mb-3 flex items-center gap-2">
            <span className="w-7 h-7 rounded-lg bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center text-sm">💡</span>
            Key Takeaways
          </h3>
          <ul className="space-y-2.5">
            {s.keyTakeaways.map((t, i) => (
              <li key={i} className="flex gap-3 text-zinc-700 dark:text-zinc-300">
                <span className="shrink-0 w-6 h-6 rounded-full bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 flex items-center justify-center text-xs font-bold">{i + 1}</span>
                <span>{t}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {/* Thinking Questions */}
      {s.thinkingQuestions.length > 0 && (
        <section className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-5">
          <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 mb-3 flex items-center gap-2">
            <span className="w-7 h-7 rounded-lg bg-purple-100 dark:bg-purple-900/30 flex items-center justify-center text-sm">🤔</span>
            Thinking Questions
          </h3>
          <ul className="space-y-2">
            {s.thinkingQuestions.map((q, i) => (
              <li key={i} className="p-3 rounded-lg bg-purple-50 dark:bg-purple-950/30 text-zinc-700 dark:text-zinc-300 text-sm">
                {q}
              </li>
            ))}
          </ul>
        </section>
      )}

      {/* Glossary */}
      {s.glossary.length > 0 && (
        <section className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-5">
          <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 mb-3 flex items-center gap-2">
            <span className="w-7 h-7 rounded-lg bg-green-100 dark:bg-green-900/30 flex items-center justify-center text-sm">📖</span>
            Glossary
          </h3>
          <div className="grid gap-2 sm:grid-cols-2">
            {s.glossary.map((g) => (
              <div key={g.term} className="p-3 rounded-lg bg-zinc-50 dark:bg-zinc-800/50">
                <span className="font-semibold text-zinc-900 dark:text-zinc-100 text-sm">{g.term}</span>
                <p className="text-zinc-500 text-sm mt-0.5">{g.definition}</p>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function ChaptersTab({
  episode,
  currentTime,
  seekTo,
}: {
  episode: Episode;
  currentTime: number;
  seekTo: (t: number) => void;
}) {
  if (episode.chapters.length === 0) return <EmptyState text="No chapters available" />;
  const totalDuration = episode.source.duration || episode.chapters.at(-1)?.endTime || 1;

  return (
    <div className="space-y-4">
      {/* Visual timeline bar */}
      <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-4">
        <div className="relative h-8 bg-zinc-100 dark:bg-zinc-800 rounded-full overflow-hidden">
          {episode.chapters.map((ch, i) => {
            const left = (ch.startTime / totalDuration) * 100;
            const width = ((ch.endTime - ch.startTime) / totalDuration) * 100;
            const isActive = currentTime >= ch.startTime && currentTime < ch.endTime;
            const colors = ["bg-blue-400", "bg-purple-400", "bg-teal-400", "bg-amber-400", "bg-rose-400", "bg-indigo-400", "bg-emerald-400", "bg-orange-400"];
            return (
              <button
                key={i}
                onClick={() => seekTo(ch.startTime)}
                title={ch.title}
                className={`absolute top-0 h-full ${colors[i % colors.length]} ${isActive ? "opacity-100 ring-2 ring-blue-600 z-10" : "opacity-60 hover:opacity-80"} transition-opacity`}
                style={{ left: `${left}%`, width: `${Math.max(width, 0.5)}%` }}
              />
            );
          })}
          {/* Playhead */}
          <div
            className="absolute top-0 w-0.5 h-full bg-red-500 z-20 pointer-events-none"
            style={{ left: `${(currentTime / totalDuration) * 100}%` }}
          />
        </div>
      </div>

      {/* Chapter cards */}
      <div className="space-y-3">
        {episode.chapters.map((ch, i) => {
          const isActive = currentTime >= ch.startTime && currentTime < ch.endTime;
          return (
            <button
              key={i}
              onClick={() => seekTo(ch.startTime)}
              className={`w-full text-left p-4 rounded-xl border transition-all ${
                isActive
                  ? "bg-blue-50 dark:bg-blue-950/50 border-blue-300 dark:border-blue-800 shadow-sm"
                  : "bg-white dark:bg-zinc-900 border-zinc-200 dark:border-zinc-800 hover:border-blue-300 dark:hover:border-blue-700"
              }`}
            >
              <div className="flex items-start gap-3">
                <div className="shrink-0 w-16 text-center">
                  <span className="font-mono text-sm text-blue-600 dark:text-blue-400">{formatTimestamp(ch.startTime)}</span>
                  <div className="text-xs text-zinc-400 mt-0.5">{formatDuration(ch.endTime - ch.startTime)}</div>
                </div>
                <div className="flex-1 min-w-0">
                  <h3 className={`font-semibold ${isActive ? "text-blue-700 dark:text-blue-300" : "text-zinc-900 dark:text-zinc-100"}`}>
                    {ch.title}
                  </h3>
                  <p className="text-sm text-zinc-500 mt-1 line-clamp-2">{ch.summary}</p>
                </div>
                {ch.hasAds && <span className="text-xs px-2 py-0.5 rounded-full bg-amber-100 text-amber-700">AD</span>}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function MindmapTab({ episode }: { episode: Episode }) {
  if (!episode.mindmap?.markdown) return <EmptyState text="No mindmap available" />;
  return (
    <div className="space-y-3">
      <MarkmapView markdown={episode.mindmap.markdown} />
      <details className="text-sm">
        <summary className="text-zinc-400 cursor-pointer hover:text-zinc-600">View Markdown Source</summary>
        <pre className="mt-2 p-4 rounded-lg bg-zinc-100 dark:bg-zinc-800 text-xs overflow-x-auto whitespace-pre-wrap">
          {episode.mindmap.markdown}
        </pre>
      </details>
    </div>
  );
}

function TranscriptTab({
  episode,
  currentTime,
  seekTo,
}: {
  episode: Episode;
  currentTime: number;
  seekTo: (t: number) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  useEffect(() => {
    if (autoScroll && activeRef.current) {
      activeRef.current.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }, [currentTime, autoScroll]);

  if (!episode.transcript) return <EmptyState text="No transcript available" />;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm text-zinc-400">{episode.transcript.paragraphs.length} paragraphs</span>
        <label className="flex items-center gap-2 text-sm text-zinc-500 cursor-pointer">
          <input type="checkbox" checked={autoScroll} onChange={(e) => setAutoScroll(e.target.checked)} className="rounded" />
          Auto-scroll
        </label>
      </div>
      <div ref={containerRef} className="space-y-1">
        {episode.transcript.paragraphs.map((para) => {
          const startTime = para.sentences[0]?.words[0]?.start ?? 0;
          const endTime = para.sentences.at(-1)?.words.at(-1)?.end ?? 0;
          const isActive = currentTime >= startTime && currentTime <= endTime;
          return (
            <div
              key={para.id}
              ref={isActive ? activeRef : undefined}
              onClick={() => seekTo(startTime)}
              className={`px-3 py-2 rounded-lg cursor-pointer transition-all text-sm leading-relaxed ${
                isActive
                  ? "bg-blue-50 dark:bg-blue-950/60 border-l-[3px] border-blue-500 text-zinc-900 dark:text-zinc-100"
                  : "hover:bg-zinc-100 dark:hover:bg-zinc-800/50 text-zinc-600 dark:text-zinc-400"
              }`}
            >
              <span className="font-mono text-xs text-blue-500 mr-2">{formatTimestamp(startTime)}</span>
              {para.speaker && <span className={`text-xs font-semibold mr-1.5 ${speakerColor(para.speaker)}`}>[{para.speaker}]</span>}
              {para.sentences.map((s, si) => (
                <span key={s.id || si} className="sentence-span">{si > 0 ? " " : ""}{s.text}</span>
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function KeywordsTab({ episode }: { episode: Episode }) {
  if (episode.keywords.length === 0) return <EmptyState text="No keywords available" />;
  return (
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
      {episode.keywords.map((kw) => (
        <div key={kw.key} className="p-4 rounded-xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 hover:shadow-md transition-shadow">
          <span className="text-base font-bold text-zinc-900 dark:text-zinc-100">{kw.key}</span>
          {kw.desc && <p className="mt-1.5 text-sm text-zinc-500 leading-relaxed">{kw.desc}</p>}
        </div>
      ))}
    </div>
  );
}

function HighlightsTab({ episode, seekTo }: { episode: Episode; seekTo: (t: number) => void }) {
  if (episode.highlights.length === 0) return <EmptyState text="No highlights available" />;
  return (
    <div className="space-y-3">
      {episode.highlights.map((h, i) => (
        <div
          key={i}
          onClick={() => seekTo(h.time)}
          className="flex gap-4 p-4 rounded-xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 cursor-pointer hover:border-amber-300 dark:hover:border-amber-700 hover:shadow-sm transition-all"
        >
          <div className="shrink-0">
            <span className="inline-block px-2 py-1 font-mono text-xs rounded-lg bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400">
              {formatTimestamp(h.time)}
            </span>
          </div>
          <div>
            <p className="text-zinc-800 dark:text-zinc-200 font-medium">🌟 {h.content}</p>
            {h.note && <p className="mt-1 text-sm text-zinc-500 italic">{h.note}</p>}
          </div>
        </div>
      ))}
    </div>
  );
}

function QATab({ episode }: { episode: Episode }) {
  if (episode.qas.length === 0) return <EmptyState text="No Q&A available" />;
  return (
    <div className="space-y-4">
      {episode.qas.map((qa, i) => (
        <div key={i} className="rounded-xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 overflow-hidden">
          <div className="p-4 bg-blue-50 dark:bg-blue-950/30 border-b border-zinc-200 dark:border-zinc-800">
            <div className="flex items-start gap-2">
              <span className="shrink-0 w-6 h-6 rounded-full bg-blue-600 text-white flex items-center justify-center text-xs font-bold">Q</span>
              <div>
                <p className="font-medium text-zinc-900 dark:text-zinc-100">{qa.question}</p>
                {qa.questionSpeaker && <span className="text-xs text-zinc-400 mt-1">— {qa.questionSpeaker}</span>}
              </div>
            </div>
          </div>
          <div className="p-4">
            <div className="flex items-start gap-2">
              <span className="shrink-0 w-6 h-6 rounded-full bg-green-600 text-white flex items-center justify-center text-xs font-bold">A</span>
              <div>
                <p className="text-zinc-700 dark:text-zinc-300 leading-relaxed">{qa.answer}</p>
                {qa.answerSpeaker && <span className="text-xs text-zinc-400 mt-1">— {qa.answerSpeaker}</span>}
              </div>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function EmptyState({ text }: { text: string }) {
  return <div className="text-center py-16 text-zinc-400"><p>{text}</p></div>;
}
