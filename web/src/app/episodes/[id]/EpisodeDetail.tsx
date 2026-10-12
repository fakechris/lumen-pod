"use client";

import { useEffect, useState, useRef, useCallback, useMemo } from "react";
import Link from "next/link";
import { getEpisode, formatTimestamp, formatDuration, getExportUrl, sendChatMessage, requestChapterDeepRead, type ChatMsg } from "@/lib/api";
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

function extractXTweetId(url: string): string | null {
  const m = url.match(/(?:twitter\.com|x\.com)\/\w+\/status\/(\d+)/);
  return m ? m[1] : null;
}

const platformIcons: Record<string, string> = {
  youtube: "🎬", bilibili: "📺", spotify: "🎵",
  apple: "🍎", twitter: "𝕏", xiaoyuzhou: "🎧", default: "🎙️",
};

// ---------------------------------------------------------------------------
// Audio Player — self-hosted HTML5 audio with custom controls
// ---------------------------------------------------------------------------

interface PlayerHandle {
  seekTo: (t: number) => void;
  getCurrentTime: () => number;
}

function AudioPlayer({
  src,
  onReady,
  onTimeUpdate,
}: {
  src: string;
  onReady?: (h: PlayerHandle) => void;
  onTimeUpdate?: (t: number) => void;
}) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [speed, setSpeed] = useState(1);
  const [volume, setVolume] = useState(1);
  const progressRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    const handle: PlayerHandle = {
      seekTo: (t) => { audio.currentTime = t; },
      getCurrentTime: () => audio.currentTime,
    };

    const onLoaded = () => {
      setDuration(audio.duration);
      onReady?.(handle);
    };
    const onTime = () => {
      setCurrentTime(audio.currentTime);
      onTimeUpdate?.(audio.currentTime);
    };
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    const onEnded = () => setPlaying(false);

    audio.addEventListener("loadedmetadata", onLoaded);
    audio.addEventListener("timeupdate", onTime);
    audio.addEventListener("play", onPlay);
    audio.addEventListener("pause", onPause);
    audio.addEventListener("ended", onEnded);

    return () => {
      audio.removeEventListener("loadedmetadata", onLoaded);
      audio.removeEventListener("timeupdate", onTime);
      audio.removeEventListener("play", onPlay);
      audio.removeEventListener("pause", onPause);
      audio.removeEventListener("ended", onEnded);
    };
  }, [src]);

  const togglePlay = () => {
    const audio = audioRef.current;
    if (!audio) return;
    playing ? audio.pause() : audio.play();
  };

  const handleProgressClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const rect = progressRef.current?.getBoundingClientRect();
    if (!rect || !audioRef.current) return;
    const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    audioRef.current.currentTime = ratio * duration;
  };

  const changeSpeed = () => {
    const speeds = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
    const next = speeds[(speeds.indexOf(speed) + 1) % speeds.length];
    setSpeed(next);
    if (audioRef.current) audioRef.current.playbackRate = next;
  };

  const skip = (delta: number) => {
    if (audioRef.current) audioRef.current.currentTime += delta;
  };

  return (
    <div className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-4">
      <audio ref={audioRef} src={src} preload="metadata" />
      <div className="flex items-center gap-3">
        {/* Skip back */}
        <button onClick={() => skip(-15)} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 transition-colors" title="Back 15s">
          <svg viewBox="0 0 24 24" fill="currentColor" className="w-5 h-5"><path d="M11.99 5V1l-5 5 5 5V7c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6h-2c0 4.42 3.58 8 8 8s8-3.58 8-8-3.58-8-8-8z"/><text x="12" y="16" textAnchor="middle" fontSize="7" fill="currentColor">15</text></svg>
        </button>

        {/* Play/Pause */}
        <button onClick={togglePlay} className="w-10 h-10 rounded-full bg-blue-600 hover:bg-blue-700 text-white flex items-center justify-center transition-colors shadow-sm">
          {playing ? (
            <svg viewBox="0 0 24 24" fill="currentColor" className="w-5 h-5"><path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z"/></svg>
          ) : (
            <svg viewBox="0 0 24 24" fill="currentColor" className="w-5 h-5 ml-0.5"><path d="M8 5v14l11-7z"/></svg>
          )}
        </button>

        {/* Skip forward */}
        <button onClick={() => skip(30)} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 transition-colors" title="Forward 30s">
          <svg viewBox="0 0 24 24" fill="currentColor" className="w-5 h-5"><path d="M12.01 5V1l5 5-5 5V7c-3.31 0-6 2.69-6 6s2.69 6 6 6 6-2.69 6-6h2c0 4.42-3.58 8-8 8s-8-3.58-8-8 3.58-8 8-8z"/><text x="12" y="16" textAnchor="middle" fontSize="7" fill="currentColor">30</text></svg>
        </button>

        {/* Time */}
        <span className="text-xs font-mono text-zinc-500 shrink-0 w-24 text-center">
          {formatTimestamp(currentTime)} / {formatTimestamp(duration)}
        </span>

        {/* Progress */}
        <div ref={progressRef} onClick={handleProgressClick} className="flex-1 h-2 bg-zinc-200 dark:bg-zinc-700 rounded-full cursor-pointer group relative">
          <div className="h-full bg-blue-600 rounded-full transition-all relative" style={{ width: duration ? `${(currentTime / duration) * 100}%` : "0%" }}>
            <div className="absolute right-0 top-1/2 -translate-y-1/2 w-3.5 h-3.5 bg-blue-600 rounded-full shadow-md opacity-0 group-hover:opacity-100 transition-opacity" />
          </div>
        </div>

        {/* Speed */}
        <button onClick={changeSpeed} className="px-2 py-1 text-xs font-mono rounded-lg bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-200 dark:hover:bg-zinc-700 transition-colors" title="Playback speed">
          {speed}x
        </button>

        {/* Volume */}
        <div className="flex items-center gap-1">
          <button onClick={() => { const v = volume > 0 ? 0 : 1; setVolume(v); if (audioRef.current) audioRef.current.volume = v; }} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300">
            {volume === 0 ? (
              <svg viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4"><path d="M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51A8.796 8.796 0 0021 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3L3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06a8.99 8.99 0 003.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4L9.91 6.09 12 8.18V4z"/></svg>
            ) : (
              <svg viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4"><path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z"/></svg>
            )}
          </button>
          <input
            type="range"
            min={0}
            max={1}
            step={0.05}
            value={volume}
            onChange={(e) => { const v = parseFloat(e.target.value); setVolume(v); if (audioRef.current) audioRef.current.volume = v; }}
            className="w-16 h-1 accent-blue-600"
          />
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// YouTube Player — thumbnail-first, click to activate (avoids bot detection)
// ---------------------------------------------------------------------------

function YouTubePlayer({
  videoId,
  thumbnailUrl,
  onReady,
  onTimeUpdate,
}: {
  videoId: string;
  thumbnailUrl?: string | null;
  onReady?: (h: PlayerHandle) => void;
  onTimeUpdate?: (t: number) => void;
}) {
  const [activated, setActivated] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const playerRef = useRef<any>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const thumb = thumbnailUrl || `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg`;

  useEffect(() => {
    if (!activated) return;
    let cancelled = false;

    function init() {
      if (cancelled || !containerRef.current) return;
      const div = document.createElement("div");
      div.id = `yt-player-${videoId}-${Date.now()}`;
      containerRef.current.innerHTML = "";
      containerRef.current.appendChild(div);

      playerRef.current = new (window as any).YT.Player(div.id, {
        videoId,
        host: "https://www.youtube-nocookie.com",
        playerVars: {
          autoplay: 1, rel: 0, modestbranding: 1, cc_load_policy: 0,
          origin: typeof window !== "undefined" ? window.location.origin : "",
        },
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
  }, [activated, videoId]);

  if (!activated) {
    return (
      <div className="relative w-full aspect-video rounded-xl overflow-hidden bg-black group cursor-pointer" onClick={() => setActivated(true)}>
        <img src={thumb} alt="Video thumbnail" className="w-full h-full object-cover" />
        <div className="absolute inset-0 flex items-center justify-center bg-black/20 group-hover:bg-black/30 transition-colors">
          <div className="w-16 h-16 rounded-full bg-red-600 flex items-center justify-center shadow-lg group-hover:scale-110 transition-transform">
            <svg viewBox="0 0 24 24" fill="white" className="w-8 h-8 ml-1"><path d="M8 5v14l11-7z"/></svg>
          </div>
        </div>
        <a
          href={`https://www.youtube.com/watch?v=${videoId}`}
          target="_blank"
          rel="noopener"
          onClick={(e) => e.stopPropagation()}
          className="absolute bottom-3 right-3 text-white text-xs bg-black/60 hover:bg-black/80 rounded-lg px-2 py-1 transition-colors"
        >
          Watch on YouTube ↗
        </a>
      </div>
    );
  }

  return <div ref={containerRef} className="w-full aspect-video rounded-xl overflow-hidden bg-black" />;
}

function BilibiliPlayer({ bvid }: { bvid: string }) {
  return (
    <iframe
      src={`https://player.bilibili.com/player.html?bvid=${bvid}&high_quality=1&danmaku=0&autoplay=0`}
      className="w-full aspect-video rounded-xl"
      allowFullScreen
      sandbox="allow-scripts allow-same-origin allow-popups"
    />
  );
}

function XTweetEmbed({ tweetId, url }: { tweetId: string; url: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (!containerRef.current) return;
    let cancelled = false;

    function renderWidget() {
      if (cancelled || !containerRef.current) return;
      const w = (window as any).twttr;
      if (w?.widgets) {
        w.widgets.createTweet(tweetId, containerRef.current, {
          theme: "dark",
          dnt: true,
          align: "center",
        }).then(() => { if (!cancelled) setLoaded(true); });
      }
    }

    if ((window as any).twttr?.widgets) {
      renderWidget();
    } else {
      const tag = document.createElement("script");
      tag.src = "https://platform.twitter.com/widgets.js";
      tag.async = true;
      tag.onload = renderWidget;
      document.head.appendChild(tag);
    }

    return () => { cancelled = true; };
  }, [tweetId]);

  return (
    <div className="rounded-xl overflow-hidden">
      <div ref={containerRef} className="min-h-[200px]" />
      {!loaded && (
        <a
          href={url}
          target="_blank"
          rel="noopener"
          className="block p-4 bg-zinc-900 rounded-xl text-center text-blue-400 hover:text-blue-300 transition-colors"
        >
          View on X/Twitter ↗
        </a>
      )}
    </div>
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
// Speaker color palette
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

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

const TABS = ["Summary", "Chapters", "Mindmap", "Transcript", "Keywords", "Highlights", "Q&A", "Chat"] as const;
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
  const tweetId = useMemo(() => episode ? extractXTweetId(episode.source.url) : null, [episode]);
  const hasAudio = !!episode?.source.audioPath;
  const hasEmbed = !!(youtubeId || bilibiliId || tweetId);
  const hasPlayer = hasAudio || hasEmbed;

  // Available translation languages
  const translationLangs = useMemo(() => {
    if (!episode?.transcript?.translations) return [];
    return Object.keys(episode.transcript.translations);
  }, [episode]);

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
            {episode.source.language && <><span>·</span><span>🌐 {episode.source.language.toUpperCase()}</span></>}
            {episode.processing.llmModel && <><span>·</span><span>🤖 {episode.processing.llmModel}</span></>}
            {episode.tags.length > 0 && episode.tags.map((t) => (
              <span key={t} className="px-2 py-0.5 text-xs rounded-full bg-zinc-100 dark:bg-zinc-800">{t}</span>
            ))}
            <a href={episode.source.url} target="_blank" rel="noopener" className="text-blue-500 hover:underline ml-1">Source ↗</a>
            <span className="text-zinc-300 dark:text-zinc-600">|</span>
            <a href={getExportUrl(id, "markdown")} className="text-blue-500 hover:underline text-xs">📝 MD</a>
            <a href={getExportUrl(id, "srt")} className="text-blue-500 hover:underline text-xs">📄 SRT</a>
            <a href={getExportUrl(id, "json")} className="text-blue-500 hover:underline text-xs">📦 JSON</a>
            <a href={getExportUrl(id, "notion")} className="text-blue-500 hover:underline text-xs">📓 Notion</a>
            <a href={getExportUrl(id, "obsidian")} className="text-blue-500 hover:underline text-xs">🗄️ Obsidian</a>
          </div>
        </div>
      </header>

      {/* Two-column layout */}
      <div className="max-w-7xl mx-auto px-4 py-6">
        <div className={hasPlayer ? "flex flex-col lg:flex-row gap-6" : ""}>

          {/* Left Column: Player + Chapters */}
          {hasPlayer && (
            <div className="lg:w-[480px] xl:w-[560px] shrink-0 space-y-4 lg:sticky lg:top-4 lg:self-start">
              {/* Self-hosted audio player (priority) */}
              {hasAudio && (
                <AudioPlayer
                  src={episode.source.audioPath!}
                  onReady={(h) => { playerHandleRef.current = h; }}
                  onTimeUpdate={setCurrentTime}
                />
              )}

              {/* Video/tweet embed (fallback if no audio) */}
              {!hasAudio && youtubeId && (
                <YouTubePlayer
                  videoId={youtubeId}
                  thumbnailUrl={episode.source.thumbnailUrl}
                  onReady={(h) => { playerHandleRef.current = h; }}
                  onTimeUpdate={setCurrentTime}
                />
              )}
              {!hasAudio && bilibiliId && <BilibiliPlayer bvid={bilibiliId} />}
              {!hasAudio && tweetId && <XTweetEmbed tweetId={tweetId} url={episode.source.url} />}

              {/* If we have audio AND video/embed, show small preview */}
              {hasAudio && youtubeId && (
                <details className="text-sm">
                  <summary className="text-zinc-400 cursor-pointer hover:text-zinc-600">Show Video</summary>
                  <div className="mt-2">
                    <YouTubePlayer videoId={youtubeId} thumbnailUrl={episode.source.thumbnailUrl} />
                  </div>
                </details>
              )}
              {hasAudio && bilibiliId && (
                <details className="text-sm">
                  <summary className="text-zinc-400 cursor-pointer hover:text-zinc-600">Show Video</summary>
                  <div className="mt-2"><BilibiliPlayer bvid={bilibiliId} /></div>
                </details>
              )}
              {hasAudio && tweetId && (
                <details className="text-sm">
                  <summary className="text-zinc-400 cursor-pointer hover:text-zinc-600">Show Tweet</summary>
                  <div className="mt-2"><XTweetEmbed tweetId={tweetId} url={episode.source.url} /></div>
                </details>
              )}

              {/* Mini chapters under player */}
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
              {activeTab === "Chapters" && <ChaptersTab episode={episode} episodeId={id} currentTime={currentTime} seekTo={seekTo} />}
              {activeTab === "Mindmap" && <MindmapTab episode={episode} />}
              {activeTab === "Transcript" && <TranscriptTab episode={episode} translationLangs={translationLangs} currentTime={currentTime} seekTo={seekTo} />}
              {activeTab === "Keywords" && <KeywordsTab episode={episode} />}
              {activeTab === "Highlights" && <HighlightsTab episode={episode} seekTo={seekTo} />}
              {activeTab === "Q&A" && <QATab episode={episode} />}
              {activeTab === "Chat" && <ChatTab episodeId={id} episode={episode} />}
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
      <div className="p-5 rounded-xl bg-gradient-to-br from-blue-50 to-indigo-50 dark:from-blue-950/50 dark:to-indigo-950/50 border border-blue-100 dark:border-blue-900">
        <p className="text-lg font-semibold text-blue-900 dark:text-blue-100">{s.oneLiner}</p>
      </div>

      <div className="prose dark:prose-invert max-w-none">
        <p className="text-zinc-700 dark:text-zinc-300 leading-relaxed whitespace-pre-wrap">{s.abstract}</p>
      </div>

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

      {s.thinkingQuestions.length > 0 && (
        <section className="bg-white dark:bg-zinc-900 rounded-xl border border-zinc-200 dark:border-zinc-800 p-5">
          <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 mb-3 flex items-center gap-2">
            <span className="w-7 h-7 rounded-lg bg-purple-100 dark:bg-purple-900/30 flex items-center justify-center text-sm">🤔</span>
            Thinking Questions
          </h3>
          <ul className="space-y-2">
            {s.thinkingQuestions.map((q, i) => (
              <li key={i} className="p-3 rounded-lg bg-purple-50 dark:bg-purple-950/30 text-zinc-700 dark:text-zinc-300 text-sm">{q}</li>
            ))}
          </ul>
        </section>
      )}

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

// ---------------------------------------------------------------------------
// Chapters with Deep Read
// ---------------------------------------------------------------------------

function ChaptersTab({
  episode,
  episodeId,
  currentTime,
  seekTo,
}: {
  episode: Episode;
  episodeId: string;
  currentTime: number;
  seekTo: (t: number) => void;
}) {
  const [deepReads, setDeepReads] = useState<Record<number, { loading: boolean; content?: string; error?: string }>>({});

  if (episode.chapters.length === 0) return <EmptyState text="No chapters available" />;
  const totalDuration = episode.source.duration || episode.chapters.at(-1)?.endTime || 1;

  const handleDeepRead = async (chapterIdx: number) => {
    if (deepReads[chapterIdx]?.content) {
      setDeepReads((prev) => {
        const copy = { ...prev };
        delete copy[chapterIdx];
        return copy;
      });
      return;
    }

    setDeepReads((prev) => ({ ...prev, [chapterIdx]: { loading: true } }));
    try {
      const result = await requestChapterDeepRead(episodeId, chapterIdx);
      setDeepReads((prev) => ({ ...prev, [chapterIdx]: { loading: false, content: result.content } }));
    } catch (e: any) {
      setDeepReads((prev) => ({ ...prev, [chapterIdx]: { loading: false, error: e.message } }));
    }
  };

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
          <div className="absolute top-0 w-0.5 h-full bg-red-500 z-20 pointer-events-none" style={{ left: `${(currentTime / totalDuration) * 100}%` }} />
        </div>
      </div>

      {/* Chapter cards with deep read */}
      <div className="space-y-3">
        {episode.chapters.map((ch, i) => {
          const isActive = currentTime >= ch.startTime && currentTime < ch.endTime;
          const dr = deepReads[i];
          return (
            <div key={i} className={`rounded-xl border transition-all ${
              isActive
                ? "bg-blue-50 dark:bg-blue-950/50 border-blue-300 dark:border-blue-800 shadow-sm"
                : "bg-white dark:bg-zinc-900 border-zinc-200 dark:border-zinc-800 hover:border-blue-300 dark:hover:border-blue-700"
            }`}>
              <button
                onClick={() => seekTo(ch.startTime)}
                className="w-full text-left p-4"
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

              {/* Deep Read button */}
              <div className="px-4 pb-3 flex items-center gap-2">
                <button
                  onClick={(e) => { e.stopPropagation(); handleDeepRead(i); }}
                  disabled={dr?.loading}
                  className="px-3 py-1.5 text-xs font-medium rounded-lg bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-400 hover:bg-purple-200 dark:hover:bg-purple-900/50 transition-colors disabled:opacity-50"
                >
                  {dr?.loading ? "⏳ Generating..." : dr?.content ? "📖 Hide Deep Read" : "📖 Deep Read"}
                </button>
              </div>

              {/* Deep read content */}
              {dr?.content && (
                <div className="px-4 pb-4 border-t border-zinc-100 dark:border-zinc-800 pt-3">
                  <div className="prose dark:prose-invert max-w-none text-sm">
                    <div className="whitespace-pre-wrap text-zinc-700 dark:text-zinc-300 leading-relaxed">{dr.content}</div>
                  </div>
                </div>
              )}
              {dr?.error && (
                <div className="px-4 pb-4">
                  <p className="text-sm text-red-500">{dr.error}</p>
                </div>
              )}
            </div>
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

// ---------------------------------------------------------------------------
// Transcript with Search + Language Toggle
// ---------------------------------------------------------------------------

function TranscriptTab({
  episode,
  translationLangs,
  currentTime,
  seekTo,
}: {
  episode: Episode;
  translationLangs: string[];
  currentTime: number;
  seekTo: (t: number) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [searchQuery, setSearchQuery] = useState("");
  const [selectedLang, setSelectedLang] = useState<string | "">("");

  useEffect(() => {
    if (autoScroll && activeRef.current) {
      activeRef.current.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }, [currentTime, autoScroll]);

  if (!episode.transcript) return <EmptyState text="No transcript available" />;

  const translations = episode.transcript.translations;
  const langTranslations = selectedLang ? translations[selectedLang] : undefined;

  const filteredParagraphs = searchQuery.trim()
    ? episode.transcript.paragraphs.filter((para) =>
        para.sentences.some((s) =>
          s.text.toLowerCase().includes(searchQuery.toLowerCase())
        )
      )
    : episode.transcript.paragraphs;

  const highlightText = (text: string) => {
    if (!searchQuery.trim()) return text;
    const regex = new RegExp(`(${searchQuery.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi");
    const parts = text.split(regex);
    return parts.map((part, i) =>
      regex.test(part) ? (
        <mark key={i} className="bg-yellow-200 dark:bg-yellow-800 rounded px-0.5">{part}</mark>
      ) : (
        part
      )
    );
  };

  const langNames: Record<string, string> = {
    zh: "中文", en: "English", ja: "日本語", ko: "한국어", es: "Español",
    fr: "Français", de: "Deutsch", pt: "Português", ru: "Русский", ar: "العربية",
  };

  return (
    <div className="space-y-3">
      {/* Controls */}
      <div className="flex items-center gap-3 flex-wrap">
        {/* Search */}
        <div className="relative flex-1 min-w-[200px]">
          <svg className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><circle cx={11} cy={11} r={8}/><path d="M21 21l-4.35-4.35"/></svg>
          <input
            type="text"
            placeholder="Search transcript..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-9 pr-3 py-2 text-sm rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 focus:ring-2 focus:ring-blue-500 outline-none placeholder:text-zinc-400"
          />
          {searchQuery && (
            <button onClick={() => setSearchQuery("")} className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-zinc-600">
              <svg viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4"><path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"/></svg>
            </button>
          )}
        </div>

        {/* Language toggle */}
        {translationLangs.length > 0 && (
          <select
            value={selectedLang}
            onChange={(e) => setSelectedLang(e.target.value)}
            className="px-3 py-2 text-sm rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 focus:ring-2 focus:ring-blue-500 outline-none"
          >
            <option value="">Original</option>
            {translationLangs.map((lang) => (
              <option key={lang} value={lang}>{langNames[lang] || lang}</option>
            ))}
          </select>
        )}

        {/* Stats + auto-scroll */}
        <span className="text-sm text-zinc-400">
          {searchQuery ? `${filteredParagraphs.length} / ` : ""}{episode.transcript.paragraphs.length} paragraphs
        </span>
        <label className="flex items-center gap-2 text-sm text-zinc-500 cursor-pointer">
          <input type="checkbox" checked={autoScroll} onChange={(e) => setAutoScroll(e.target.checked)} className="rounded" />
          Auto-scroll
        </label>
      </div>

      {/* Transcript content */}
      <div ref={containerRef} className="space-y-1">
        {filteredParagraphs.map((para) => {
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
                <span key={s.id || si} className="sentence-span">{si > 0 ? " " : ""}{highlightText(s.text)}</span>
              ))}
              {/* Translation */}
              {langTranslations && (
                <div className="mt-1 pl-12 text-xs text-zinc-400 dark:text-zinc-500 italic">
                  {para.sentences.map((s) => {
                    const trans = langTranslations[s.id];
                    return trans ? <span key={s.id}>{trans.text} </span> : null;
                  })}
                </div>
              )}
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
            <p className="text-zinc-800 dark:text-zinc-200 font-medium">{h.content}</p>
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

// ---------------------------------------------------------------------------
// AI Chat Tab
// ---------------------------------------------------------------------------

function ChatTab({ episodeId, episode }: { episodeId: string; episode: Episode }) {
  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [loadingHistory, setLoadingHistory] = useState(true);
  const chatEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    fetch(`${process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:3210"}/api/episodes/${episodeId}/chat`, {
      credentials: "include",
    })
      .then((r) => r.json())
      .then((data) => setMessages(data.messages || []))
      .catch(() => {})
      .finally(() => setLoadingHistory(false));
  }, [episodeId]);

  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || sending) return;

    const userMsg: ChatMsg = { id: `temp-${Date.now()}`, episodeId, role: "user", content: text, createdAt: new Date().toISOString() };
    setMessages((prev) => [...prev, userMsg]);
    setInput("");
    setSending(true);

    try {
      const response = await sendChatMessage(episodeId, text);
      setMessages((prev) => {
        const updated = prev.filter((m) => m.id !== userMsg.id);
        return [...updated, response.userMessage, response.assistantMessage];
      });
    } catch (e: any) {
      setMessages((prev) => [
        ...prev,
        { id: `err-${Date.now()}`, episodeId, role: "assistant", content: `Error: ${e.message}`, createdAt: new Date().toISOString() },
      ]);
    } finally {
      setSending(false);
    }
  };

  const suggestedQuestions = [
    `What is the main argument in "${episode.source.title}"?`,
    "Summarize the key points in 3 bullets",
    "What are the controversial claims made?",
    "What questions remain unanswered?",
  ];

  return (
    <div className="flex flex-col h-[600px]">
      {/* Messages */}
      <div className="flex-1 overflow-y-auto space-y-3 mb-4">
        {loadingHistory ? (
          <div className="text-center py-8 text-zinc-400">Loading chat history...</div>
        ) : messages.length === 0 ? (
          <div className="space-y-4">
            <div className="text-center py-8">
              <p className="text-zinc-400 mb-4">Ask AI about this episode</p>
            </div>
            <div className="grid gap-2 sm:grid-cols-2">
              {suggestedQuestions.map((q, i) => (
                <button
                  key={i}
                  onClick={() => setInput(q)}
                  className="text-left p-3 text-sm rounded-lg border border-zinc-200 dark:border-zinc-700 hover:border-blue-400 dark:hover:border-blue-600 text-zinc-600 dark:text-zinc-400 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
                >
                  {q}
                </button>
              ))}
            </div>
          </div>
        ) : (
          messages.map((msg) => (
            <div key={msg.id} className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}>
              <div className={`max-w-[80%] px-4 py-3 rounded-2xl text-sm whitespace-pre-wrap ${
                msg.role === "user"
                  ? "bg-blue-600 text-white rounded-br-md"
                  : "bg-white dark:bg-zinc-800 text-zinc-800 dark:text-zinc-200 border border-zinc-200 dark:border-zinc-700 rounded-bl-md"
              }`}>
                {msg.content}
              </div>
            </div>
          ))
        )}
        {sending && (
          <div className="flex justify-start">
            <div className="px-4 py-3 rounded-2xl rounded-bl-md bg-white dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700">
              <div className="flex gap-1">
                <span className="w-2 h-2 rounded-full bg-zinc-400 animate-bounce" style={{ animationDelay: "0ms" }} />
                <span className="w-2 h-2 rounded-full bg-zinc-400 animate-bounce" style={{ animationDelay: "150ms" }} />
                <span className="w-2 h-2 rounded-full bg-zinc-400 animate-bounce" style={{ animationDelay: "300ms" }} />
              </div>
            </div>
          </div>
        )}
        <div ref={chatEndRef} />
      </div>

      {/* Input */}
      <div className="flex gap-2">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && handleSend()}
          placeholder="Ask about this episode..."
          disabled={sending}
          className="flex-1 px-4 py-3 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 text-sm focus:ring-2 focus:ring-blue-500 outline-none disabled:opacity-50 placeholder:text-zinc-400"
        />
        <button
          onClick={handleSend}
          disabled={!input.trim() || sending}
          className="px-4 py-3 rounded-xl bg-blue-600 hover:bg-blue-700 disabled:bg-zinc-300 dark:disabled:bg-zinc-700 text-white text-sm font-medium transition-colors"
        >
          Send
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Empty state
// ---------------------------------------------------------------------------

function EmptyState({ text }: { text: string }) {
  return <div className="text-center py-16 text-zinc-400"><p>{text}</p></div>;
}
