import { useState, useEffect } from "react";
import { getApiConfig, setApiConfig, DEFAULT_API_BASE_URL } from "../../config";

interface SettingsProps {
  onBack: () => void;
}

export function Settings({ onBack }: SettingsProps) {
  const [baseUrl, setBaseUrl] = useState(DEFAULT_API_BASE_URL);
  const [token, setToken] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    void getApiConfig().then((config) => {
      setBaseUrl(config.baseUrl);
      setToken(config.token);
    });
  }, []);

  const handleSave = async () => {
    await setApiConfig(baseUrl.replace(/\/$/, ""), token.trim());
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  return (
    <div className="min-h-screen bg-white dark:bg-slate-900">
      <header className="sticky top-0 z-10 bg-white/80 dark:bg-slate-900/80 backdrop-blur border-b border-zinc-200 dark:border-zinc-700 px-3 py-2 flex items-center gap-2">
        <button
          onClick={onBack}
          className="p-1 text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M15 18l-6-6 6-6" />
          </svg>
        </button>
        <span className="font-semibold text-sm text-zinc-900 dark:text-zinc-100">
          设置
        </span>
      </header>

      <div className="p-4 space-y-4">
        <div>
          <label className="block text-xs font-medium text-zinc-700 dark:text-zinc-300 mb-1">
            API 地址
          </label>
          <input
            type="url"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="https://lumenpod.example.com"
            className="w-full px-3 py-2 text-sm border border-zinc-300 dark:border-zinc-600 rounded-lg bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:ring-2 focus:ring-blue-500 focus:border-transparent outline-none"
          />
          <p className="text-xs text-zinc-400 mt-1">
            你的 Lumen Pod 服务地址
          </p>
        </div>

        <div>
          <label className="block text-xs font-medium text-zinc-700 dark:text-zinc-300 mb-1">
            API Token
          </label>
          <input
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="lp_xxxxxxxx"
            className="w-full px-3 py-2 text-sm border border-zinc-300 dark:border-zinc-600 rounded-lg bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:ring-2 focus:ring-blue-500 focus:border-transparent outline-none"
          />
          <p className="text-xs text-zinc-400 mt-1">
            在服务端设置 LUMEN_POD_API_TOKEN 环境变量获取
          </p>
        </div>

        <button
          onClick={() => void handleSave()}
          className="w-full py-2 bg-blue-500 hover:bg-blue-600 text-white text-sm font-medium rounded-lg transition-colors"
        >
          {saved ? "✓ 已保存" : "保存"}
        </button>
      </div>
    </div>
  );
}
