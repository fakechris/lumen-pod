import { useState, useEffect, useCallback } from "react";
import type { Article, Episode } from "../../types/messages";
import { formatTimestamp, requestEpisodeArticle } from "../../utils/api";
import { ChapterList } from "./ChapterList";
import { TranscriptView } from "./TranscriptView";
import { MarkdownContent } from "./MarkdownContent";

type Tab = "summary" | "article" | "chapters" | "transcript" | "qa";

interface EpisodeViewProps {
  episode: Episode;
}

export function EpisodeView({ episode }: EpisodeViewProps) {
  const [activeTab, setActiveTab] = useState<Tab>("summary");
  const [currentTime, setCurrentTime] = useState(0);
  const [article, setArticle] = useState<Article | undefined>(episode.article);

  useEffect(() => {
    setArticle(episode.article);
  }, [episode.id, episode.article]);

  // Poll YouTube player time
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const [tab] = await chrome.tabs.query({
          active: true,
          currentWindow: true,
        });
        if (!tab?.id) return;

        const results = await chrome.scripting.executeScript({
          target: { tabId: tab.id },
          func: () => {
            const video = document.querySelector("video");
            return video?.currentTime ?? 0;
          },
        });

        if (results?.[0]?.result !== undefined) {
          setCurrentTime(results[0].result as number);
        }
      } catch {
        // tab might not be accessible
      }
    }, 500);

    return () => clearInterval(interval);
  }, []);

  const seekTo = useCallback(async (time: number) => {
    try {
      const [tab] = await chrome.tabs.query({
        active: true,
        currentWindow: true,
      });
      if (!tab?.id) return;

      await chrome.scripting.executeScript({
        target: { tabId: tab.id },
        func: (t: number) => {
          const video = document.querySelector("video");
          if (video) video.currentTime = t;
        },
        args: [time],
      });
    } catch {
      // ignore
    }
  }, []);

  const tabs: { id: Tab; label: string; count?: number }[] = [
    { id: "summary", label: "摘要" },
    { id: "article", label: "文章" },
    { id: "chapters", label: "章节", count: episode.chapters.length },
    {
      id: "transcript",
      label: "字幕",
      count: episode.transcript?.paragraphs.length,
    },
    { id: "qa", label: "问答", count: episode.qas.length },
  ];

  return (
    <div>
      {/* Episode header */}
      <div className="mb-3">
        {episode.source.thumbnailUrl && (
          <img
            src={episode.source.thumbnailUrl}
            alt=""
            className="w-full rounded-lg mb-2"
          />
        )}
        <h2 className="text-sm font-bold text-zinc-900 dark:text-zinc-100 leading-tight">
          {episode.source.title}
        </h2>
        <div className="flex items-center gap-2 mt-1 text-xs text-zinc-500 dark:text-zinc-400">
          <span>{episode.source.author}</span>
          {episode.source.duration > 0 && (
            <>
              <span>•</span>
              <span>{formatTimestamp(episode.source.duration)}</span>
            </>
          )}
        </div>
      </div>

      {/* Tab bar */}
      <div className="flex border-b border-zinc-200 dark:border-zinc-700 mb-3 -mx-3 px-3 overflow-x-auto">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`px-3 py-1.5 text-xs font-medium whitespace-nowrap border-b-2 transition-colors ${
              activeTab === tab.id
                ? "border-blue-500 text-blue-600 dark:text-blue-400"
                : "border-transparent text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
            }`}
          >
            {tab.label}
            {tab.count !== undefined && tab.count > 0 && (
              <span className="ml-1 text-[10px] text-zinc-400">
                {tab.count}
              </span>
            )}
          </button>
        ))}
      </div>

      {/* Tab content */}
      {activeTab === "summary" && (
        <SummaryTab episode={episode} />
      )}
      {activeTab === "article" && (
        <ArticleTab
          episodeId={episode.id}
          article={article}
          onArticleUpdated={setArticle}
          onSeek={seekTo}
        />
      )}
      {activeTab === "chapters" && (
        <ChapterList
          chapters={episode.chapters}
          currentTime={currentTime}
          onSeek={seekTo}
        />
      )}
      {activeTab === "transcript" && episode.transcript && (
        <TranscriptView
          transcript={episode.transcript}
          currentTime={currentTime}
          onSeek={seekTo}
        />
      )}
      {activeTab === "qa" && <QATab episode={episode} />}
    </div>
  );
}

function ArticleTab({
  episodeId,
  article,
  onArticleUpdated,
  onSeek,
}: {
  episodeId: string;
  article?: Article;
  onArticleUpdated: (art: Article) => void;
  onSeek: (seconds: number) => void;
}) {
  const [focus, setFocus] = useState(article?.focus ?? "");
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleGenerate = async (regenerate = false) => {
    setGenerating(true);
    setError(null);
    try {
      const res = await requestEpisodeArticle(episodeId, {
        lang: "zh",
        focus: focus.trim() || undefined,
        regenerate,
      });
      onArticleUpdated(res.article);
    } catch (e: any) {
      setError(e.message || "生成失败");
    } finally {
      setGenerating(false);
    }
  };

  return (
    <div className="space-y-2.5">
      <div className="flex flex-col gap-1.5 bg-zinc-50 dark:bg-zinc-800/60 p-2 rounded-lg border border-zinc-200 dark:border-zinc-700">
        <input
          type="text"
          value={focus}
          onChange={(e) => setFocus(e.target.value)}
          disabled={generating}
          placeholder="可选：输入关注重点（如：AI Agent 架构）"
          className="w-full px-2 py-1 text-xs rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-800 dark:text-zinc-200 outline-none"
        />
        <button
          onClick={() => handleGenerate(!!article)}
          disabled={generating}
          className="w-full py-1.5 text-xs font-medium rounded bg-blue-600 hover:bg-blue-700 text-white transition-colors disabled:opacity-50"
        >
          {generating ? "⏳ 正在撰写深度长文..." : article ? "🔄 重新生成深度长文" : "✨ 生成「读完就够」深度长文"}
        </button>
      </div>

      {error && (
        <p className="text-[11px] text-red-500 bg-red-50 dark:bg-red-950/30 p-2 rounded">
          {error}
        </p>
      )}

      {article ? (
        <div className="space-y-2">
          <h3 className="text-xs font-bold text-zinc-900 dark:text-zinc-100 leading-snug">
            {article.title}
          </h3>
          <MarkdownContent content={article.content} onSeek={onSeek} />
        </div>
      ) : (
        !generating && (
          <p className="text-xs text-zinc-400 py-4 text-center">
            点击上方按钮将本视频转化为独立深度长文，文中时间戳可点击直接跳转视频进度。
          </p>
        )
      )}
    </div>
  );
}

function SummaryTab({ episode }: { episode: Episode }) {
  if (!episode.summary) {
    return (
      <p className="text-xs text-zinc-400 py-4 text-center">暂无摘要</p>
    );
  }

  const s = episode.summary;

  return (
    <div className="space-y-3">
      {/* One-liner */}
      <div className="bg-blue-50 dark:bg-blue-950/30 rounded-lg p-3">
        <p className="text-sm font-medium text-zinc-800 dark:text-zinc-200 leading-relaxed">
          {s.oneLiner}
        </p>
      </div>

      {/* Abstract */}
      <div className="text-xs text-zinc-600 dark:text-zinc-400 leading-relaxed">
        <MarkdownContent content={s.abstract} />
      </div>

      {/* Key takeaways */}
      {s.keyTakeaways.length > 0 && (
        <div>
          <h4 className="text-xs font-semibold text-zinc-700 dark:text-zinc-300 mb-1.5">
            核心要点
          </h4>
          <ul className="space-y-1">
            {s.keyTakeaways.map((t, i) => (
              <li
                key={i}
                className="text-xs text-zinc-600 dark:text-zinc-400 flex gap-1.5"
              >
                <span className="text-blue-500 mt-0.5">•</span>
                <span>{t}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Keywords */}
      {episode.keywords.length > 0 && (
        <div>
          <h4 className="text-xs font-semibold text-zinc-700 dark:text-zinc-300 mb-1.5">
            关键词
          </h4>
          <div className="flex flex-wrap gap-1.5">
            {episode.keywords.map((kw) => (
              <span
                key={kw.key}
                className="px-2 py-0.5 text-[10px] bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 rounded-full"
                title={kw.desc}
              >
                {kw.key}
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Glossary */}
      {s.glossary.length > 0 && (
        <div>
          <h4 className="text-xs font-semibold text-zinc-700 dark:text-zinc-300 mb-1.5">
            术语表
          </h4>
          <dl className="space-y-1">
            {s.glossary.map((g) => (
              <div key={g.term} className="text-xs">
                <dt className="font-medium text-zinc-700 dark:text-zinc-300 inline">
                  {g.term}:
                </dt>
                <dd className="text-zinc-500 dark:text-zinc-400 inline ml-1">
                  {g.definition}
                </dd>
              </div>
            ))}
          </dl>
        </div>
      )}
    </div>
  );
}

function QATab({ episode }: { episode: Episode }) {
  if (episode.qas.length === 0) {
    return (
      <p className="text-xs text-zinc-400 py-4 text-center">暂无问答</p>
    );
  }

  return (
    <div className="space-y-3">
      {episode.qas.map((qa, i) => (
        <div
          key={i}
          className="border border-zinc-200 dark:border-zinc-700 rounded-lg overflow-hidden"
        >
          <div className="bg-zinc-50 dark:bg-zinc-800/50 px-3 py-2">
            <p className="text-xs font-medium text-zinc-800 dark:text-zinc-200">
              Q: {qa.question}
            </p>
          </div>
          <div className="px-3 py-2 text-xs text-zinc-600 dark:text-zinc-400 leading-relaxed">
            {qa.answer}
          </div>
        </div>
      ))}
    </div>
  );
}
