import React from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

interface MarkdownContentProps {
  content: string;
  onSeek?: (seconds: number) => void;
}

function parseTimestampToSeconds(ts: string): number | null {
  const parts = ts.split(":").map(Number);
  if (parts.some((n) => Number.isNaN(n))) return null;
  if (parts.length === 2) return parts[0] * 60 + parts[1];
  if (parts.length === 3) return parts[0] * 3600 + parts[1] * 60 + parts[2];
  return null;
}

const TIMESTAMP_TOKEN_RE = /(\[\d{1,2}:\d{2}(?::\d{2})?(?:[^\]]*)\])/g;
const TIMESTAMP_INNER_RE = /^\[(\d{1,2}:\d{2}(?::\d{2})?)(?:\s+([^\]]+))?\]$/;

function renderWithTimestamps(
  children: React.ReactNode,
  onSeek?: (seconds: number) => void,
): React.ReactNode {
  if (!onSeek) return children;
  return React.Children.map(children, (child, idx) => {
    if (typeof child === "string") {
      const segments = child.split(TIMESTAMP_TOKEN_RE);
      if (segments.length === 1) return child;
      return segments.map((seg, i) => {
        const m = seg.match(TIMESTAMP_INNER_RE);
        if (!m) return seg;
        const sec = parseTimestampToSeconds(m[1]);
        if (sec === null) return seg;
        const note = m[2];
        return (
          <button
            key={`${idx}-${i}`}
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onSeek(sec);
            }}
            className="inline-flex items-center gap-0.5 mx-0.5 px-1 py-0.5 text-[10px] font-mono rounded bg-blue-100/80 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300 hover:bg-blue-200 dark:hover:bg-blue-800/60 transition-colors cursor-pointer"
            title={`跳转至 ${m[1]}`}
          >
            <span>▶ {m[1]}</span>
            {note && <span className="font-sans">{note}</span>}
          </button>
        );
      });
    }
    return child;
  });
}

export function MarkdownContent({ content, onSeek }: MarkdownContentProps) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        h1: ({ children }) => (
          <h3 className="text-xs font-bold text-zinc-800 dark:text-zinc-200 mt-3 mb-1">
            {children}
          </h3>
        ),
        h2: ({ children }) => (
          <h4 className="text-xs font-semibold text-zinc-800 dark:text-zinc-200 mt-2 mb-1">
            {children}
          </h4>
        ),
        h3: ({ children }) => (
          <h5 className="text-[11px] font-semibold text-zinc-700 dark:text-zinc-300 mt-2 mb-0.5">
            {children}
          </h5>
        ),
        p: ({ children }) => (
          <p className="text-[11px] text-zinc-600 dark:text-zinc-400 leading-relaxed mb-1.5">
            {renderWithTimestamps(children, onSeek)}
          </p>
        ),
        ul: ({ children }) => (
          <ul className="text-[11px] text-zinc-600 dark:text-zinc-400 space-y-0.5 ml-3 list-disc mb-1.5">
            {children}
          </ul>
        ),
        ol: ({ children }) => (
          <ol className="text-[11px] text-zinc-600 dark:text-zinc-400 space-y-0.5 ml-3 list-decimal mb-1.5">
            {children}
          </ol>
        ),
        li: ({ children }) => <li>{renderWithTimestamps(children, onSeek)}</li>,
        strong: ({ children }) => (
          <strong className="font-semibold text-zinc-700 dark:text-zinc-300">
            {renderWithTimestamps(children, onSeek)}
          </strong>
        ),
        blockquote: ({ children }) => (
          <blockquote className="border-l-2 border-blue-300 dark:border-blue-700 pl-2 my-1.5 text-[11px] text-zinc-500 dark:text-zinc-400 italic">
            {children}
          </blockquote>
        ),
        em: ({ children }) => <em>{renderWithTimestamps(children, onSeek)}</em>,
        code: ({ children }) => {
          const text = typeof children === "string" ? children.trim() : "";
          if (TIMESTAMP_INNER_RE.test(text)) {
            return <>{renderWithTimestamps(text, onSeek)}</>;
          }
          return (
            <code className="text-[10px] bg-zinc-100 dark:bg-zinc-800 px-1 py-0.5 rounded">
              {children}
            </code>
          );
        },
      }}
    >
      {content}
    </ReactMarkdown>
  );
}
