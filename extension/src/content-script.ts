import { extractVideoId } from "./utils/youtube";

let currentVideoId: string | null = null;
let indicatorEl: HTMLElement | null = null;

function checkUrl() {
  const videoId = extractVideoId(window.location.href);
  if (videoId === currentVideoId) return;
  currentVideoId = videoId;

  if (!videoId) {
    removeIndicator();
    return;
  }

  // Ask service worker to look up this video
  chrome.runtime.sendMessage(
    { type: "LOOKUP_VIDEO", videoId, url: window.location.href },
    (response) => {
      if (chrome.runtime.lastError) return;
      if (response?.type === "LOOKUP_RESULT" && response.found) {
        showIndicator(response.title ?? "View in Lumen Pod");
      } else {
        removeIndicator();
      }
    }
  );
}

function showIndicator(title: string) {
  removeIndicator();

  indicatorEl = document.createElement("div");
  indicatorEl.id = "lumen-pod-indicator";
  indicatorEl.innerHTML = `
    <div style="
      position: fixed;
      bottom: 24px;
      right: 24px;
      z-index: 99999;
      background: linear-gradient(135deg, #3B82F6, #6366F1);
      color: white;
      padding: 10px 16px;
      border-radius: 12px;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
      font-size: 13px;
      font-weight: 500;
      cursor: pointer;
      box-shadow: 0 4px 20px rgba(59, 130, 246, 0.4);
      display: flex;
      align-items: center;
      gap: 8px;
      transition: transform 0.2s, box-shadow 0.2s;
      max-width: 300px;
    " id="lumen-pod-indicator-inner">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10"></circle>
        <polygon points="10 8 16 12 10 16 10 8"></polygon>
      </svg>
      <span style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">
        ${escapeHtml(title)}
      </span>
    </div>
  `;

  const inner = indicatorEl.querySelector(
    "#lumen-pod-indicator-inner"
  ) as HTMLElement;
  if (inner) {
    inner.addEventListener("mouseenter", () => {
      inner.style.transform = "scale(1.05)";
      inner.style.boxShadow = "0 6px 24px rgba(59, 130, 246, 0.5)";
    });
    inner.addEventListener("mouseleave", () => {
      inner.style.transform = "scale(1)";
      inner.style.boxShadow = "0 4px 20px rgba(59, 130, 246, 0.4)";
    });
    inner.addEventListener("click", () => {
      chrome.runtime.sendMessage({ type: "OPEN_SIDE_PANEL" });
    });
  }

  document.body.appendChild(indicatorEl);

  // Auto-hide after 8 seconds
  setTimeout(() => {
    if (indicatorEl) {
      indicatorEl.style.transition = "opacity 0.5s";
      indicatorEl.style.opacity = "0";
      setTimeout(removeIndicator, 500);
    }
  }, 8000);
}

function removeIndicator() {
  if (indicatorEl) {
    indicatorEl.remove();
    indicatorEl = null;
  }
}

function escapeHtml(text: string): string {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

// Initial check
checkUrl();

// YouTube SPA navigation detection
// 1. Listen for yt-navigate-finish (YouTube's custom event)
document.addEventListener("yt-navigate-finish", () => {
  setTimeout(checkUrl, 300);
});

// 2. Fallback: MutationObserver on the title
const titleObserver = new MutationObserver(() => {
  checkUrl();
});

const titleEl = document.querySelector("title");
if (titleEl) {
  titleObserver.observe(titleEl, { childList: true });
}

// 3. popstate for back/forward
window.addEventListener("popstate", () => {
  setTimeout(checkUrl, 200);
});
