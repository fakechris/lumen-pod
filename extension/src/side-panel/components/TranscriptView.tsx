import { useEffect, useRef, useState } from "react";
import type { Transcript, Paragraph } from "../../types/messages";
import { formatTimestamp } from "../../utils/api";

interface TranscriptViewProps {
  transcript: Transcript;
  currentTime: number;
  onSeek: (time: number) => void;
}

const SPEAKER_COLORS = [
  "text-blue-500",
  "text-emerald-500",
  "text-purple-500",
  "text-orange-500",
  "text-pink-500",
  "text-teal-500",
];

export function TranscriptView({
  transcript,
  currentTime,
  onSeek,
}: TranscriptViewProps) {
  const [autoScroll, setAutoScroll] = useState(true);
  const activeRef = useRef<HTMLDivElement>(null);
  const speakerMap = useRef(new Map<string, string>());

  function speakerColor(speaker: string): string {
    if (!speakerMap.current.has(speaker)) {
      speakerMap.current.set(
        speaker,
        SPEAKER_COLORS[speakerMap.current.size % SPEAKER_COLORS.length]
      );
    }
    return speakerMap.current.get(speaker)!;
  }

  // Auto-scroll to active paragraph
  useEffect(() => {
    if (autoScroll && activeRef.current) {
      activeRef.current.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
    }
  }, [currentTime, autoScroll]);

  const paragraphs = transcript.paragraphs;

  if (paragraphs.length === 0) {
    return (
      <p className="text-xs text-zinc-400 py-4 text-center">暂无字幕</p>
    );
  }

  return (
    <div>
      {/* Auto-scroll toggle */}
      <div className="flex items-center justify-end mb-2">
        <label className="flex items-center gap-1.5 text-[10px] text-zinc-500 dark:text-zinc-400 cursor-pointer select-none">
          <input
            type="checkbox"
            checked={autoScroll}
            onChange={(e) => setAutoScroll(e.target.checked)}
            className="rounded border-zinc-300 text-blue-500 focus:ring-blue-500 h-3 w-3"
          />
          自动滚动
        </label>
      </div>

      {/* Paragraphs */}
      <div className="space-y-0.5">
        {paragraphs.map((para) => {
          const startTime = para.sentences[0]?.words[0]?.start ?? 0;
          const endTime =
            para.sentences.at(-1)?.words.at(-1)?.end ?? 0;
          const isActive =
            currentTime >= startTime && currentTime <= endTime;

          return (
            <div
              key={para.id}
              ref={isActive ? activeRef : undefined}
              onClick={() => onSeek(startTime)}
              className={`px-2 py-1.5 rounded-md transition-all text-[11px] leading-relaxed cursor-pointer ${
                isActive
                  ? "bg-blue-50 dark:bg-blue-950/60 border-l-2 border-blue-500 text-zinc-800 dark:text-zinc-200"
                  : "hover:bg-zinc-50 dark:hover:bg-zinc-800/50 text-zinc-500 dark:text-zinc-400"
              }`}
            >
              <span className="font-mono text-[9px] text-blue-500 mr-1.5">
                {formatTimestamp(startTime)}
              </span>
              {para.speaker && (
                <span
                  className={`text-[10px] font-semibold mr-1 ${speakerColor(para.speaker)}`}
                >
                  [{para.speaker}]
                </span>
              )}
              {para.sentences.map((s, si) => {
                const sStart = s.words[0]?.start ?? 0;
                const sEnd = s.words.at(-1)?.end ?? 0;
                const isSentenceActive =
                  isActive &&
                  currentTime >= sStart &&
                  currentTime <= sEnd;

                return (
                  <span
                    key={s.id || si}
                    onClick={(e) => {
                      e.stopPropagation();
                      onSeek(sStart);
                    }}
                    className={`transition-colors duration-200 ${
                      isSentenceActive
                        ? "bg-blue-200/80 dark:bg-blue-700/50 text-blue-900 dark:text-blue-100 font-medium rounded px-0.5"
                        : ""
                    }`}
                  >
                    {si > 0 ? " " : ""}
                    {s.text}
                  </span>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}
