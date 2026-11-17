"use client";

import { useEffect, useRef, useState } from "react";
import qrcode from "qrcode-generator";
import type { Episode } from "@/lib/types";
import { formatTimestamp } from "@/lib/api";

export type PosterLayout = "quote" | "minimal";

export interface PosterData {
  /** The quote / takeaway text rendered large on the card. */
  text: string;
  /** Seconds — where the QR code deep-links to (`episodes/{id}?t=`). */
  time: number;
  /** Optional attribution line (speaker name). */
  speaker?: string;
  /** Small label above the quote, e.g. the clip title. */
  label?: string;
}

const CANVAS_W = 1080;
const CANVAS_H = 1440; // 3:4 — wechat-friendly share card
const FONT_STACK = '"PingFang SC","Hiragino Sans GB","Source Han Sans SC","Noto Sans CJK SC","Microsoft YaHei",system-ui,sans-serif';

/** Wrap mixed CJK/latin text by measuring per character (canvas measureText
 * handles both scripts; break opportunities after CJK chars and spaces). */
function wrapText(ctx: CanvasRenderingContext2D, text: string, maxWidth: number, maxLines: number): string[] {
  const lines: string[] = [];
  let cur = "";
  const chars = Array.from(text);
  for (let i = 0; i < chars.length; i++) {
    const ch = chars[i];
    const next = cur + ch;
    if (ctx.measureText(next).width > maxWidth && cur) {
      // avoid breaking inside a latin word: backtrack to the last space
      let cut = cur.length;
      if (!/[\u2e80-\u9fff\u3000-\u303f\uff00-\uffef]/.test(ch) && !/[\u2e80-\u9fff]/.test(cur[cur.length - 1] ?? "")) {
        const sp = cur.lastIndexOf(" ");
        if (sp > 0 && lines.length + 1 <= maxLines) cut = sp + 1;
      }
      lines.push(cur.slice(0, cut));
      cur = cur.slice(cut) + ch;
    } else {
      cur = next;
    }
    if (lines.length >= maxLines) return lines;
  }
  if (cur && lines.length < maxLines) lines.push(cur);
  return lines;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

function drawQr(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, size: number, dark: string, light: string) {
  const qr = qrcode(0, "M");
  qr.addData(text);
  qr.make();
  const n = qr.getModuleCount();
  const cell = size / n;
  ctx.fillStyle = light;
  ctx.fillRect(x - cell, y - cell, size + cell * 2, size + cell * 2);
  ctx.fillStyle = dark;
  for (let r = 0; r < n; r++) {
    for (let c = 0; c < n; c++) {
      if (qr.isDark(r, c)) ctx.fillRect(x + c * cell, y + r * cell, Math.ceil(cell), Math.ceil(cell));
    }
  }
}

function drawPoster(canvas: HTMLCanvasElement, episode: Episode, data: PosterData, layout: PosterLayout, link: string) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  canvas.width = CANVAS_W;
  canvas.height = CANVAS_H;

  const dark = layout === "minimal";
  // Background
  if (dark) {
    ctx.fillStyle = "#18181b";
    ctx.fillRect(0, 0, CANVAS_W, CANVAS_H);
    ctx.fillStyle = "rgba(245, 158, 11, 0.08)";
    ctx.fillRect(0, 0, CANVAS_W, 8);
  } else {
    const grad = ctx.createLinearGradient(0, 0, CANVAS_W, CANVAS_H);
    grad.addColorStop(0, "#eff6ff");
    grad.addColorStop(0.5, "#f8fafc");
    grad.addColorStop(1, "#eef2ff");
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, CANVAS_W, CANVAS_H);
  }

  const pad = 84;
  const quoteColor = dark ? "#fafafa" : "#18181b";
  const subColor = dark ? "#d4d4d8" : "#52525b";

  // Opening quote mark
  ctx.fillStyle = dark ? "#f59e0b" : "#3b82f6";
  ctx.font = `700 220px ${FONT_STACK}`;
  ctx.fillText("\u201c", pad - 8, 300);

  // Label chip
  let cursorY = 320;
  if (data.label) {
    ctx.font = `600 34px ${FONT_STACK}`;
    const labelW = ctx.measureText(data.label).width;
    ctx.fillStyle = dark ? "rgba(245,158,11,0.15)" : "rgba(59,130,246,0.12)";
    roundRect(ctx, pad, cursorY, labelW + 44, 62, 31);
    ctx.fill();
    ctx.fillStyle = dark ? "#fbbf24" : "#2563eb";
    ctx.fillText(data.label, pad + 22, cursorY + 43);
    cursorY += 110;
  }

  // Quote text — auto-shrink until it fits maxLines
  const maxW = CANVAS_W - pad * 2;
  const maxLines = 9;
  let fontSize = 64;
  let lines: string[] = [];
  for (; fontSize >= 36; fontSize -= 4) {
    ctx.font = `600 ${fontSize}px ${FONT_STACK}`;
    lines = wrapText(ctx, data.text, maxW, maxLines);
    if (lines.length <= maxLines && !isTruncated(lines, data.text)) break;
  }
  ctx.fillStyle = quoteColor;
  ctx.font = `600 ${fontSize}px ${FONT_STACK}`;
  const lineHeight = fontSize * 1.52;
  for (const line of lines) {
    ctx.fillText(line, pad, cursorY + fontSize);
    cursorY += lineHeight;
  }
  cursorY += 26;

  function isTruncated(ls: string[], src: string): boolean {
    return ls.join("").replace(/\s/g, "").length < src.replace(/\s/g, "").length;
  }

  // Attribution + timestamp
  const stamp = formatTimestamp(data.time);
  const attribution = [data.speaker, `\u3010${stamp}\u3011`].filter(Boolean).join(" ");
  if (attribution) {
    ctx.font = `500 38px ${FONT_STACK}`;
    ctx.fillStyle = dark ? "#fbbf24" : "#2563eb";
    ctx.fillText(attribution, pad, cursorY + 40);
    cursorY += 92;
  }

  // Footer card: episode title / author / platform / QR
  const footH = 250;
  const footY = CANVAS_H - footH - 64;
  ctx.fillStyle = dark ? "rgba(255,255,255,0.06)" : "rgba(255,255,255,0.75)";
  roundRect(ctx, 56, footY, CANVAS_W - 112, footH, 36);
  ctx.fill();

  const qrSize = 170;
  const qrX = CANVAS_W - 56 - 36 - qrSize;
  const qrY = footY + (footH - qrSize) / 2;
  drawQr(ctx, link, qrX, qrY, qrSize, dark ? "#fafafa" : "#18181b", dark ? "#27272a" : "#ffffff");

  ctx.font = `700 36px ${FONT_STACK}`;
  ctx.fillStyle = quoteColor;
  const titleLines = wrapText(ctx, episode.source.title, qrX - pad - 60, 2);
  ctx.fillText(titleLines[0] ?? "", pad, footY + 62);
  if (titleLines[1]) {
    ctx.font = `500 32px ${FONT_STACK}`;
    ctx.fillText(titleLines[1], pad, footY + 106);
  }
  ctx.font = `500 30px ${FONT_STACK}`;
  ctx.fillStyle = subColor;
  const meta = [episode.source.author, episode.source.podcastName, episode.source.platform].filter(Boolean).join(" \u00b7 ");
  ctx.fillText(truncateToWidth(ctx, meta, qrX - pad - 60), pad, footY + (titleLines[1] ? 150 : 118));

  // Wordmark
  ctx.font = `700 30px ${FONT_STACK}`;
  ctx.fillStyle = subColor;
  ctx.fillText("Lumen Pod", pad, CANVAS_H - 24);
}

function truncateToWidth(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string {
  if (ctx.measureText(text).width <= maxWidth) return text;
  let t = text;
  while (t.length > 1 && ctx.measureText(t + "\u2026").width > maxWidth) t = t.slice(0, -1);
  return t + "\u2026";
}

export default function PosterDialog({
  episode,
  data,
  onClose,
}: {
  episode: Episode;
  data: PosterData | null;
  onClose: () => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [layout, setLayout] = useState<PosterLayout>("quote");
  const [copied, setCopied] = useState(false);
  const link = data ? `${typeof window !== "undefined" ? window.location.origin : ""}/episodes/${episode.id}?t=${Math.floor(data.time)}` : "";

  useEffect(() => {
    if (!data || !canvasRef.current) return;
    drawPoster(canvasRef.current, episode, data, layout, link);
  }, [data, layout, episode, link]);

  if (!data) return null;

  const download = () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.toBlob((blob) => {
      if (!blob) return;
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = `lumen-pod-${episode.id.slice(0, 8)}-${Math.floor(data.time)}s.png`;
      a.click();
      URL.revokeObjectURL(a.href);
    }, "image/png");
  };

  const copyLink = async () => {
    try {
      await navigator.clipboard.writeText(link);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch { /* clipboard unavailable */ }
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4" onClick={onClose}>
      <div className="bg-white dark:bg-zinc-900 rounded-2xl shadow-2xl max-w-lg w-full p-5 space-y-4" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h3 className="text-base font-bold text-zinc-900 dark:text-zinc-100">金句海报</h3>
          <button onClick={onClose} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 text-xl leading-none">&times;</button>
        </div>
        <canvas ref={canvasRef} className="w-full rounded-xl border border-zinc-200 dark:border-zinc-800" />
        <div className="flex items-center gap-2">
          {(["quote", "minimal"] as const).map((l) => (
            <button
              key={l}
              onClick={() => setLayout(l)}
              className={`px-3 py-1.5 text-sm rounded-lg border transition-colors ${layout === l ? "bg-blue-600 text-white border-blue-600" : "border-zinc-300 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 hover:border-blue-400"}`}
            >
              {l === "quote" ? "清朗蓝" : "极简黑"}
            </button>
          ))}
          <div className="flex-1" />
          <button onClick={copyLink} className="px-3 py-1.5 text-sm rounded-lg border border-zinc-300 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300 hover:border-blue-400">
            {copied ? "已复制" : "复制链接"}
          </button>
          <button onClick={download} className="px-4 py-1.5 text-sm rounded-lg bg-blue-600 text-white hover:bg-blue-700">
            下载 PNG
          </button>
        </div>
        <p className="text-xs text-zinc-400">二维码指向 {link}，扫码直达该时间点。</p>
      </div>
    </div>
  );
}
