import { extractVideoId, isYouTubeVideoUrl, buildYouTubeUrl } from "./utils/youtube";
import { lookupEpisodeByUrl, getEpisode } from "./utils/api";
import type { Episode } from "./types/messages";

// Cache of looked-up episodes by videoId
const episodeCache = new Map<string, Episode | null>();

// Open side panel on icon click
chrome.action.onClicked.addListener((tab) => {
  if (tab.id !== undefined) {
    void chrome.sidePanel.open({ tabId: tab.id });
  }
});

// Handle messages from content scripts and side panel
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "LOOKUP_VIDEO") {
    handleLookup(message.videoId, message.url)
      .then(sendResponse)
      .catch((err) =>
        sendResponse({ type: "ERROR", error: String(err) })
      );
    return true;
  }

  if (message.type === "GET_EPISODE") {
    getEpisode(message.episodeId)
      .then((ep) => sendResponse({ type: "EPISODE_DATA", episode: ep }))
      .catch((err) =>
        sendResponse({ type: "ERROR", error: String(err) })
      );
    return true;
  }

  if (message.type === "OPEN_SIDE_PANEL") {
    const tabId = sender.tab?.id;
    if (tabId !== undefined) {
      void chrome.sidePanel.open({ tabId });
    }
    return false;
  }

  if (message.type === "TAB_URL_CHANGED") {
    // Content script telling us the URL changed (YouTube SPA navigation)
    if (message.videoId) {
      void handleLookup(message.videoId, buildYouTubeUrl(message.videoId));
    }
    return false;
  }

  return false;
});

// Track tab updates for YouTube SPA navigation
chrome.webNavigation.onHistoryStateUpdated.addListener(
  async (details) => {
    if (details.frameId !== 0) return;
    const videoId = extractVideoId(details.url);
    if (videoId) {
      const result = await handleLookup(videoId, details.url);
      // Update badge
      updateBadge(details.tabId, result.found);
    } else {
      updateBadge(details.tabId, false);
    }
  },
  { url: [{ hostContains: "youtube.com" }] }
);

// Also check on normal page load
chrome.tabs.onUpdated.addListener(async (tabId, changeInfo, tab) => {
  if (changeInfo.status !== "complete" || !tab.url) return;
  if (!isYouTubeVideoUrl(tab.url)) {
    updateBadge(tabId, false);
    return;
  }
  const videoId = extractVideoId(tab.url);
  if (videoId) {
    const result = await handleLookup(videoId, tab.url);
    updateBadge(tabId, result.found);
  }
});

async function handleLookup(
  videoId: string,
  url: string
): Promise<{ type: "LOOKUP_RESULT"; found: boolean; episodeId?: string; title?: string }> {
  // Check cache first
  if (episodeCache.has(videoId)) {
    const cached = episodeCache.get(videoId);
    if (cached) {
      return {
        type: "LOOKUP_RESULT",
        found: true,
        episodeId: cached.id,
        title: cached.source.title,
      };
    }
    return { type: "LOOKUP_RESULT", found: false };
  }

  // Query API
  const episode = await lookupEpisodeByUrl(url);
  episodeCache.set(videoId, episode);

  if (episode) {
    return {
      type: "LOOKUP_RESULT",
      found: true,
      episodeId: episode.id,
      title: episode.source.title,
    };
  }

  return { type: "LOOKUP_RESULT", found: false };
}

function updateBadge(tabId: number, hasEpisode: boolean) {
  if (hasEpisode) {
    void chrome.action.setBadgeText({ tabId, text: "✓" });
    void chrome.action.setBadgeBackgroundColor({ tabId, color: "#3B82F6" });
  } else {
    void chrome.action.setBadgeText({ tabId, text: "" });
  }
}
