import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

interface MarkdownContentProps {
  content: string;
}

export function MarkdownContent({ content }: MarkdownContentProps) {
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
            {children}
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
        strong: ({ children }) => (
          <strong className="font-semibold text-zinc-700 dark:text-zinc-300">
            {children}
          </strong>
        ),
        blockquote: ({ children }) => (
          <blockquote className="border-l-2 border-blue-300 dark:border-blue-700 pl-2 my-1.5 text-[11px] text-zinc-500 dark:text-zinc-400 italic">
            {children}
          </blockquote>
        ),
        code: ({ children }) => (
          <code className="text-[10px] bg-zinc-100 dark:bg-zinc-800 px-1 py-0.5 rounded">
            {children}
          </code>
        ),
      }}
    >
      {content}
    </ReactMarkdown>
  );
}
