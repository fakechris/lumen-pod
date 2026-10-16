import { useState } from "react";
import { submitEpisodeUrl } from "../../utils/api";

interface EmptyStateProps {
  isYouTube: boolean;
  currentUrl?: string;
  onOpenSettings: () => void;
  onSubmitted?: () => void;
}

export function EmptyState({
  isYouTube,
  currentUrl,
  onOpenSettings,
  onSubmitted,
}: EmptyStateProps) {
  const [submitting, setSubmitting] = useState(false);
  const [submitResult, setSubmitResult] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (!currentUrl || submitting) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const res = await submitEpisodeUrl(currentUrl);
      setSubmitResult(res.message || "已提交至处理队列");
      onSubmitted?.();
    } catch (e: any) {
      setSubmitError(e.message || "提交失败");
    } finally {
      setSubmitting(false);
    }
  };

  if (isYouTube) {
    return (
      <div className="text-center py-12 px-4 space-y-3">
        <div className="text-4xl mb-2">🎧</div>
        <h3 className="text-sm font-semibold text-zinc-700 dark:text-zinc-300">
          尚未收录此视频
        </h3>
        <p className="text-xs text-zinc-500 dark:text-zinc-400 leading-relaxed">
          此 YouTube 视频尚未被 Lumen Pod 处理。
          <br />
          可一键提交到处理队列，由 Agent 自动转写并生成深度长文。
        </p>

        {currentUrl && (
          <div className="pt-2">
            <button
              onClick={handleSubmit}
              disabled={submitting || !!submitResult}
              className="px-4 py-2 text-xs font-medium rounded-lg bg-blue-600 hover:bg-blue-700 text-white transition-colors disabled:opacity-50"
            >
              {submitting
                ? "⏳ 正在提交..."
                : submitResult
                  ? "✓ 已提交队列"
                  : "✨ 一键提交到 Lumen Pod"}
            </button>
          </div>
        )}

        {submitResult && (
          <p className="text-[11px] text-green-600 dark:text-green-400">
            {submitResult}
          </p>
        )}
        {submitError && (
          <p className="text-[11px] text-red-500">{submitError}</p>
        )}
      </div>
    );
  }

  return (
    <div className="text-center py-12 px-4">
      <div className="text-4xl mb-4">📺</div>
      <h3 className="text-sm font-semibold text-zinc-700 dark:text-zinc-300 mb-2">
        打开 YouTube 视频
      </h3>
      <p className="text-xs text-zinc-500 dark:text-zinc-400 leading-relaxed mb-4">
        请打开一个 YouTube 视频页面，
        <br />
        Lumen Pod 会自动检测已收录的内容。
      </p>
      <button
        onClick={onOpenSettings}
        className="text-xs text-blue-500 hover:text-blue-600 underline"
      >
        配置 API 设置
      </button>
    </div>
  );
}
