interface EmptyStateProps {
  isYouTube: boolean;
  onOpenSettings: () => void;
}

export function EmptyState({ isYouTube, onOpenSettings }: EmptyStateProps) {
  if (isYouTube) {
    return (
      <div className="text-center py-12 px-4">
        <div className="text-4xl mb-4">🎧</div>
        <h3 className="text-sm font-semibold text-zinc-700 dark:text-zinc-300 mb-2">
          尚未收录此视频
        </h3>
        <p className="text-xs text-zinc-500 dark:text-zinc-400 leading-relaxed">
          此 YouTube 视频尚未被 Lumen Pod 处理。
          <br />
          可以在 Lumen Pod 网页端提交此视频链接进行处理。
        </p>
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
