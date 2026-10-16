import { useState, useEffect, useCallback } from "react";
import { extractVideoId } from "../utils/youtube";
import { lookupEpisodeByUrl, getEpisode } from "../utils/api";
import type { Episode } from "../types/messages";
import { EpisodeView } from "./components/EpisodeView";
import { Settings } from "./components/Settings";
import { EmptyState } from "./components/EmptyState";

type View = "main" | "settings";

export function App() {
  const [view, setView] = useState<View>("main");
  const [episode, setEpisode] = useState<Episode | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [currentUrl, setCurrentUrl] = useState<string>("");

  const checkCurrentTab = useCallback(async () => {
    try {
      const [tab] = await chrome.tabs.query({
        active: true,
        currentWindow: true,
      });
      if (!tab?.url) return;
      setCurrentUrl(tab.url);

      const videoId = extractVideoId(tab.url);
      if (!videoId) {
        setEpisode(null);
        setError(null);
        return;
      }

      setLoading(true);
      setError(null);

      const ep = await lookupEpisodeByUrl(tab.url);
      setEpisode(ep);
      if (!ep) {
        setError(null);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void checkCurrentTab();

    // Listen for tab changes
    const onActivated = () => void checkCurrentTab();
    const onUpdated = (_: number, info: chrome.tabs.TabChangeInfo) => {
      if (info.status === "complete") void checkCurrentTab();
    };

    chrome.tabs.onActivated.addListener(onActivated);
    chrome.tabs.onUpdated.addListener(onUpdated);

    // Listen for messages from content script about SPA nav
    const onMessage = (msg: { type: string }) => {
      if (msg.type === "TAB_URL_CHANGED") {
        void checkCurrentTab();
      }
    };
    chrome.runtime.onMessage.addListener(onMessage);

    return () => {
      chrome.tabs.onActivated.removeListener(onActivated);
      chrome.tabs.onUpdated.removeListener(onUpdated);
      chrome.runtime.onMessage.removeListener(onMessage);
    };
  }, [checkCurrentTab]);

  if (view === "settings") {
    return (
      <Settings
        onBack={() => {
          setView("main");
          void checkCurrentTab();
        }}
      />
    );
  }

  return (
    <div className="min-h-screen bg-white dark:bg-slate-900">
      {/* Header */}
      <header className="sticky top-0 z-10 bg-white/80 dark:bg-slate-900/80 backdrop-blur border-b border-zinc-200 dark:border-zinc-700 px-3 py-2 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <svg
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
            className="text-blue-500"
          >
            <circle cx="12" cy="12" r="10" />
            <polygon points="10 8 16 12 10 16 10 8" />
          </svg>
          <span className="font-semibold text-sm text-zinc-900 dark:text-zinc-100">
            Lumen Pod
          </span>
        </div>
        <button
          onClick={() => setView("settings")}
          className="p-1.5 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 rounded-md hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
          title="Settings"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <circle cx="12" cy="12" r="3" />
            <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" />
          </svg>
        </button>
      </header>

      {/* Content */}
      <main className="p-3">
        {loading && (
          <div className="flex items-center justify-center py-12">
            <div className="animate-spin rounded-full h-8 w-8 border-2 border-blue-500 border-t-transparent" />
          </div>
        )}

        {error && (
          <div className="bg-red-50 dark:bg-red-950/30 text-red-700 dark:text-red-300 px-3 py-2 rounded-lg text-sm">
            {error}
          </div>
        )}

        {!loading && !error && episode && <EpisodeView episode={episode} />}

        {!loading && !error && !episode && (
          <EmptyState
            isYouTube={extractVideoId(currentUrl) !== null}
            currentUrl={currentUrl}
            onOpenSettings={() => setView("settings")}
            onSubmitted={() => void checkCurrentTab()}
          />
        )}
      </main>
    </div>
  );
}
