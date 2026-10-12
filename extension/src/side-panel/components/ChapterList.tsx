import { useState } from "react";
import type { Chapter } from "../../types/messages";
import { formatTimestamp } from "../../utils/api";
import { MarkdownContent } from "./MarkdownContent";

interface ChapterListProps {
  chapters: Chapter[];
  currentTime: number;
  onSeek: (time: number) => void;
}

export function ChapterList({ chapters, currentTime, onSeek }: ChapterListProps) {
  const [expandedIdx, setExpandedIdx] = useState<number | null>(null);

  if (chapters.length === 0) {
    return (
      <p className="text-xs text-zinc-400 py-4 text-center">暂无章节</p>
    );
  }

  return (
    <div className="space-y-1">
      {chapters.map((ch, i) => {
        const isActive =
          currentTime >= ch.startTime && currentTime < ch.endTime;
        const isExpanded = expandedIdx === i;
        const hasDeepRead = !!ch.deepRead;

        return (
          <div
            key={i}
            className={`rounded-lg border transition-all ${
              isActive
                ? "border-blue-300 dark:border-blue-700 bg-blue-50/50 dark:bg-blue-950/30"
                : "border-transparent hover:bg-zinc-50 dark:hover:bg-zinc-800/50"
            }`}
          >
            {/* Chapter header */}
            <div
              className="flex items-start gap-2 px-2.5 py-2 cursor-pointer"
              onClick={() => void onSeek(ch.startTime)}
            >
              <span className="font-mono text-[10px] text-blue-500 mt-0.5 shrink-0">
                {formatTimestamp(ch.startTime)}
              </span>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-1">
                  <h4
                    className={`text-xs font-medium leading-tight ${
                      isActive
                        ? "text-blue-700 dark:text-blue-300"
                        : "text-zinc-800 dark:text-zinc-200"
                    }`}
                  >
                    {ch.title}
                  </h4>
                  {ch.hasAds && (
                    <span className="text-[9px] px-1 py-px bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded">
                      AD
                    </span>
                  )}
                </div>
                <p className="text-[11px] text-zinc-500 dark:text-zinc-400 mt-0.5 line-clamp-2">
                  {ch.summary}
                </p>
              </div>
              {hasDeepRead && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setExpandedIdx(isExpanded ? null : i);
                  }}
                  className={`shrink-0 p-1 rounded transition-colors ${
                    isExpanded
                      ? "text-blue-500 bg-blue-100 dark:bg-blue-900/30"
                      : "text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300"
                  }`}
                  title="查看深度解读"
                >
                  <svg
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                  >
                    <path d="M2 3h6a4 4 0 0 1 4 4v14a3 3 0 0 0-3-3H2z" />
                    <path d="M22 3h-6a4 4 0 0 0-4 4v14a3 3 0 0 1 3-3h7z" />
                  </svg>
                </button>
              )}
            </div>

            {/* Deep read content */}
            {isExpanded && ch.deepRead && (
              <div className="px-2.5 pb-2.5 border-t border-zinc-100 dark:border-zinc-800 mt-1 pt-2">
                <div className="prose prose-xs dark:prose-invert max-w-none text-[11px] leading-relaxed">
                  <MarkdownContent content={ch.deepRead} />
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
