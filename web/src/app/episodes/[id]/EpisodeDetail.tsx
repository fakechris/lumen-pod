"use client";

import { useEffect, useState, useRef, useCallback } from "react";
import Link from "next/link";
import { getEpisode, formatTimestamp, formatDuration } from "@/lib/api";
import type { Episode } from "@/lib/types";

const TABS = [
  "Summary",
  "Mindmap",
  "Transcript",
  "Clips",
  "Keywords",
  "Highlights",
  "Q&A",
  "Shownotes",
] as const;
type TabName = (typeof TABS)[number];

export default function EpisodeDetail({ id }: { id: string }) {
  const [episode, setEpisode] = useState<Episode | null>(null);
  const [activeTab, setActiveTab] = useState<TabName>("Summary");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const audioRef = useRef<HTMLAudioElement>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);

  useEffect(() => {
    if (!id) return;
    getEpisode(id)
      .then(setEpisode)
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false));
  }, [id]);

  const seekTo = useCallback((time: number) => {
    if (audioRef.current) {
      audioRef.current.currentTime = time;
      audioRef.current.play();
      setIsPlaying(true);
    }
  }, []);

  if (loading) return <div className="min-h-screen flex items-center justify-center text-zinc-400">Loading...</div>;
  if (error) return <div className="min-h-screen flex items-center justify-center text-red-500">{error}</div>;
  if (!episode) return null;

  return (
    <div className="min-h-screen bg-zinc-50 dark:bg-zinc-950 pb-24">
      <header className="border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
        <div className="max-w-5xl mx-auto px-4 py-4">
          <Link href="/" className="text-sm text-blue-600 hover:underline mb-2 inline-block">← Back</Link>
          <div className="flex gap-4 mt-2">
            {episode.source.thumbnailUrl && (
              <img src={episode.source.thumbnailUrl} alt="" className="w-32 h-24 rounded-lg object-cover shrink-0" />
            )}
            <div>
              <h1 className="text-xl font-bold text-zinc-900 dark:text-zinc-100">{episode.source.title}</h1>
              <div className="flex items-center gap-2 mt-1 text-sm text-zinc-500">
                <span>{episode.source.author}</span>
                {episode.source.podcastName && (<><span>·</span><span>{episode.source.podcastName}</span></>)}
                <span>·</span>
                <span>{formatDuration(episode.source.duration)}</span>
                {episode.processing.llmModel && (<><span>·</span><span>🤖 {episode.processing.llmModel}</span></>)}
              </div>
            </div>
          </div>
        </div>
      </header>

      <div className="border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 sticky top-0 z-10">
        <div className="max-w-5xl mx-auto px-4">
          <div className="flex gap-1 overflow-x-auto">
            {TABS.map((tab) => (
              <button
                key={tab}
                onClick={() => setActiveTab(tab)}
                className={`px-4 py-3 text-sm font-medium whitespace-nowrap border-b-2 transition-colors ${
                  activeTab === tab
                    ? "border-blue-600 text-blue-600"
                    : "border-transparent text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
                }`}
              >
                {tab}
              </button>
            ))}
          </div>
        </div>
      </div>

      <main className="max-w-5xl mx-auto px-4 py-6">
        {activeTab === "Summary" && <SummaryTab episode={episode} />}
        {activeTab === "Mindmap" && <MindmapTab episode={episode} />}
        {activeTab === "Transcript" && <TranscriptTab episode={episode} currentTime={currentTime} seekTo={seekTo} />}
        {activeTab === "Clips" && <ClipsTab episode={episode} seekTo={seekTo} />}
        {activeTab === "Keywords" && <KeywordsTab episode={episode} />}
        {activeTab === "Highlights" && <HighlightsTab episode={episode} seekTo={seekTo} />}
        {activeTab === "Q&A" && <QATab episode={episode} />}
        {activeTab === "Shownotes" && <ShownotesTab episode={episode} />}
      </main>

      {episode.source.audioPath && (
        <div className="fixed bottom-0 left-0 right-0 bg-white dark:bg-zinc-900 border-t border-zinc-200 dark:border-zinc-800 px-4 py-3">
          <div className="max-w-5xl mx-auto flex items-center gap-4">
            <button
              onClick={() => {
                if (audioRef.current) {
                  isPlaying ? audioRef.current.pause() : audioRef.current.play();
                  setIsPlaying(!isPlaying);
                }
              }}
              className="w-10 h-10 rounded-full bg-blue-600 text-white flex items-center justify-center hover:bg-blue-700"
            >
              {isPlaying ? "⏸" : "▶"}
            </button>
            <div className="flex-1">
              <div className="text-sm font-medium text-zinc-900 dark:text-zinc-100 truncate">
                {episode.source.title}
              </div>
              <div className="text-xs text-zinc-500">
                {formatTimestamp(currentTime)} / {formatDuration(episode.source.duration)}
              </div>
            </div>
            <audio
              ref={audioRef}
              src={episode.source.audioPath}
              onTimeUpdate={(e) => setCurrentTime(e.currentTarget.currentTime)}
              onPlay={() => setIsPlaying(true)}
              onPause={() => setIsPlaying(false)}
            />
          </div>
        </div>
      )}
    </div>
  );
}

function SummaryTab({ episode }: { episode: Episode }) {
  if (!episode.summary) return <EmptyState text="No summary available" />;
  const s = episode.summary;
  return (
    <div className="space-y-6">
      <section>
        <h2 className="text-lg font-semibold text-zinc-900 dark:text-zinc-100">{s.oneLiner}</h2>
        <p className="mt-3 text-zinc-700 dark:text-zinc-300 leading-relaxed whitespace-pre-wrap">{s.abstract}</p>
      </section>
      {s.keyTakeaways.length > 0 && (
        <section>
          <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 mb-2">💡 Key Takeaways</h3>
          <ul className="space-y-2">
            {s.keyTakeaways.map((t, i) => (
              <li key={i} className="flex gap-2 text-zinc-700 dark:text-zinc-300">
                <span className="text-blue-500 shrink-0">•</span>
                <span>{t}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {s.thinkingQuestions.length > 0 && (
        <section>
          <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 mb-2">🤔 Thinking Questions</h3>
          <ul className="space-y-2">
            {s.thinkingQuestions.map((q, i) => (
              <li key={i} className="text-zinc-600 dark:text-zinc-400 italic">❓ {q}</li>
            ))}
          </ul>
        </section>
      )}
      {s.glossary.length > 0 && (
        <section>
          <h3 className="font-semibold text-zinc-900 dark:text-zinc-100 mb-2">📖 Glossary</h3>
          <div className="grid gap-2">
            {s.glossary.map((g) => (
              <div key={g.term} className="p-3 rounded-lg bg-zinc-100 dark:bg-zinc-800">
                <span className="font-medium text-zinc-900 dark:text-zinc-100">{g.term}</span>
                <span className="mx-2 text-zinc-400">—</span>
                <span className="text-zinc-600 dark:text-zinc-400">{g.definition}</span>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function MindmapTab({ episode }: { episode: Episode }) {
  if (!episode.mindmap) return <EmptyState text="No mindmap available" />;
  return (
    <div className="prose dark:prose-invert max-w-none">
      <pre className="whitespace-pre-wrap text-sm bg-zinc-100 dark:bg-zinc-800 p-4 rounded-lg">
        {episode.mindmap.markdown}
      </pre>
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
  if (!episode.transcript) return <EmptyState text="No transcript available" />;
  return (
    <div className="space-y-4">
      {episode.transcript.paragraphs.map((para) => {
        const startTime = para.sentences[0]?.words[0]?.start ?? 0;
        const endTime = para.sentences.at(-1)?.words.at(-1)?.end ?? 0;
        const isActive = currentTime >= startTime && currentTime <= endTime;
        return (
          <div
            key={para.id}
            className={`p-3 rounded-lg transition-colors ${
              isActive ? "bg-blue-50 dark:bg-blue-950 border border-blue-200 dark:border-blue-800" : ""
            }`}
          >
            <div className="flex items-center gap-2 mb-1">
              {para.speaker && (
                <span className="text-xs font-semibold px-2 py-0.5 rounded-full bg-zinc-200 dark:bg-zinc-700 text-zinc-700 dark:text-zinc-300">
                  {para.speaker}
                </span>
              )}
              <button onClick={() => seekTo(startTime)} className="text-xs text-blue-500 hover:underline font-mono">
                {formatTimestamp(startTime)}
              </button>
            </div>
            <p className="text-zinc-800 dark:text-zinc-200 leading-relaxed">
              {para.sentences.map((s) => s.text).join(" ")}
            </p>
          </div>
        );
      })}
    </div>
  );
}

function ClipsTab({ episode, seekTo }: { episode: Episode; seekTo: (t: number) => void }) {
  if (episode.clips.length === 0) return <EmptyState text="No clips yet" />;
  return (
    <div className="space-y-4">
      {episode.clips.map((clip) => (
        <div key={clip.id} className="p-4 rounded-lg border border-zinc-200 dark:border-zinc-800">
          <div className="flex items-start justify-between">
            <h3 className="font-semibold text-zinc-900 dark:text-zinc-100">{clip.title}</h3>
            <button onClick={() => seekTo(clip.point)} className="text-xs text-blue-500 hover:underline font-mono">
              {formatTimestamp(clip.point)}
            </button>
          </div>
          <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-400">{clip.content}</p>
          {clip.takeaways.length > 0 && (
            <ul className="mt-2 space-y-1">
              {clip.takeaways.map((t, i) => (
                <li key={i} className="text-sm text-zinc-500">• {t}</li>
              ))}
            </ul>
          )}
        </div>
      ))}
    </div>
  );
}

function KeywordsTab({ episode }: { episode: Episode }) {
  if (episode.keywords.length === 0) return <EmptyState text="No keywords available" />;
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {episode.keywords.map((kw) => (
        <div key={kw.key} className="p-4 rounded-lg border border-zinc-200 dark:border-zinc-800">
          <span className="text-lg font-semibold text-zinc-900 dark:text-zinc-100">{kw.key}</span>
          {kw.desc && <p className="mt-1 text-sm text-zinc-500">{kw.desc}</p>}
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
        <div key={i} className="flex gap-3 p-3 rounded-lg border border-zinc-200 dark:border-zinc-800">
          <button
            onClick={() => seekTo(h.time)}
            className="text-sm text-blue-500 hover:underline font-mono shrink-0 w-14 text-right"
          >
            {formatTimestamp(h.time)}
          </button>
          <div>
            <p className="text-zinc-800 dark:text-zinc-200">{h.content}</p>
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
        <div key={i} className="p-4 rounded-lg border border-zinc-200 dark:border-zinc-800">
          <div className="flex items-start gap-2">
            <span className="text-blue-500 font-bold shrink-0">Q</span>
            <div>
              <p className="font-medium text-zinc-900 dark:text-zinc-100">{qa.question}</p>
              {qa.questionSpeaker && <span className="text-xs text-zinc-400">— {qa.questionSpeaker}</span>}
            </div>
          </div>
          <div className="flex items-start gap-2 mt-3">
            <span className="text-green-500 font-bold shrink-0">A</span>
            <div>
              <p className="text-zinc-700 dark:text-zinc-300">{qa.answer}</p>
              {qa.answerSpeaker && <span className="text-xs text-zinc-400">— {qa.answerSpeaker}</span>}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}

function ShownotesTab({ episode }: { episode: Episode }) {
  if (!episode.shownotes) return <EmptyState text="No shownotes available" />;
  return (
    <div className="prose dark:prose-invert max-w-none">
      <div className="whitespace-pre-wrap text-zinc-700 dark:text-zinc-300">{episode.shownotes}</div>
    </div>
  );
}

function EmptyState({ text }: { text: string }) {
  return <div className="text-center py-16 text-zinc-400"><p>{text}</p></div>;
}
