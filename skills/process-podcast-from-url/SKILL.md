---
name: process-podcast-from-url
description: Use when Chris hands you a podcast/video URL (YouTube, B 站, 小宇宙, x.com, or a direct audio file) and wants it transcribed, structured, and saved as a searchable Lumen Pod episode. Also use when a paper PDF URL is given and the user wants it downloaded as a clean PDF. The agent that runs this skill takes one URL at a time and publishes one Episode to Lumen Pod. Not for one-off transcripts the user only wants to read in chat — for those, run the local SenseVoice path and stop.
description_zh: 用户丢过来一条 YouTube / B 站 / 小宇宙 / X / 公开论文 PDF 链接，agent 下载媒体（带平台降级链与登录态回退）、过 6 道质量门（标题匹配、封面黑帧跳过、转写完整性、说话人、时间戳、媒体完整性），再转成结构化稿、总结、章节、关键词，发布成 Lumen Pod 上的一条 Episode（可全文检索），音频分片上传、封面自动替换。**不**用于"只要把稿子给我看一下、不入库"的一次性转写。
description_en: Turn a video / podcast / paper URL into a fully processed Lumen Pod Episode: hardened download (platform fallback chain, cookies), 6 pre-publish quality gates (title match, thumbnail black-frame skip, transcript sanity, speakers, timestamps, media integrity), transcript + speaker map + summary + chapters + keywords + highlights + mindmap + Read-and-Done article, chunked audio upload and thumbnail upload to Lumen Pod. Idempotent (skips if URL already published) and resumable (writes failure to a sidecar).
scope: global
authored_by: Bot Boss (dae91cf8-ab3f-4105-80e5-b8772b25dc70)
because: Chris asked 2026-10-18 — download expert was retired, but the four jobs (download / transcribe / paper PDF / publish to Lumen Pod) keep coming back. Download hardening and quality gates absorbed from the qiaomu-clipper study (2026-10-20).
---

# process-podcast-from-url

You are an agent that takes one URL and turns it into one published Lumen Pod Episode. Follow the workflow end-to-end. Do not ask the user for clarifications that the tools can answer. Stop and report on the first error you cannot recover from.

## What you must have before you start

1. **yt-dlp** and **ffmpeg/ffprobe** on the box. Test: `yt-dlp --version && ffmpeg -version 2>&1 | head -1`.
2. **Lumen Pod MCP server connected** (see Step 0). The agent runtime injects it as a tool. If the tool is missing, stop and tell the person: "lumen-pod MCP is not connected — add it to `.pi/agent/mcp.json` or the equivalent client config; required command: `lumen-pod mcp` over stdio."
3. **An API token for Lumen Pod** in the env as `LUMEN_POD_API_TOKEN`. If missing, stop and tell the person to paste a token; do **not** ask the user to send the token in chat — they save it into the agent's secret store.
4. **Local SenseVoice fallback path** available: `/home/box/tools/sensevoice/.venv/bin/python /home/box/tools/sensevoice/transcribe.py INPUT -o PREFIX --language auto`, with model `/home/box/models/sensevoice/model.int8.onnx` and VAD `/home/box/models/silero_vad.onnx`. This is the default; you only need the Volcengine fallback path when local fails.
5. **Volcengine Doubao ASR credentials** in the env: `VOLCENGINE_ASR_API_KEY` (or, for the old console, `VOLCENGINE_ASR_APP_ID` + `VOLCENGINE_ASR_ACCESS_TOKEN`). Missing? Run the volcano setup guide (`references/volcano-setup.md`).
6. This skill's own scripts — `scripts/thumbnail.py` needs only ffmpeg/ffprobe.

If any of (1)–(4) is missing, report which one and stop. Do not partially run.

## Step 0: Idempotency check

Call `pod_exists` with the source URL. If the response is `{"exists": true}`, stop and report:
- the existing episode id
- title and author (from `pod_list` or `pod_get`)
- a link to view it

Do not re-process. Do not ask the user to confirm.

## Step 1: Extract metadata and try subtitle fast track

```bash
mkdir -p /tmp/lumen-pod
yt-dlp --write-info-json \
  --write-subs --write-auto-subs \
  --sub-langs "en.*,zh.*,ja.*" \
  --sub-format "json3/vtt/srt" \
  --skip-download \
  --ignore-config --no-playlist --socket-timeout 30 --retries 3 \
  -o "/tmp/lumen-pod/%(id)s.%(ext)s" \
  -- "<URL>"
```

Read the `.info.json` (path printed by yt-dlp; usually `/tmp/lumen-pod/<VIDEO_ID>.info.json`). Pull:
- `title` → `source.title`
- `uploader` (or `channel`) → `source.author`
- `duration` (seconds, float) → `source.duration`
- `thumbnail` (or first entry in `thumbnails[]`) → `source.thumbnailUrl`
- `upload_date` (YYYYMMDD → YYYY-MM-DDT00:00:00Z) → `source.publishedAt`
- `description` → `source.description`
- `channel` → `source.podcastName`
- `language` (or detect from auto-subs) → `source.language`
- native `chapters[]` (if present) → candidate `chapters[]`

Detect platform from the URL into `source.platform`:
- `youtube.com`, `youtu.be` → `"youtube"`
- `bilibili.com`, `b23.tv` → `"bilibili"`
- `xiaoyuzhoufm.com`, `xyzfm` → `"xiaoyuzhou"`
- `twitter.com`, `x.com` → `"twitter"`
- `podcasts.apple.com`, `apple.com/podcast` → `"apple"`
- `spotify.com` → `"spotify"`
- else `"other"`

If a subtitle file was written (`.json3`/`.vtt`/`.srt`), parse it into the lumen-pod `Transcript` shape (see `references/transcript-schema.md`) and set `processing.asrEngine = "youtube-subs"`. Skip Steps 2–3.

If no usable subtitle, continue to Step 2.

## Step 2: Download media — follow the platform fallback chain

Read `references/download-playbook.md` and apply it. The short version:

- Always run yt-dlp with the hardened baseline: `--ignore-config --no-playlist --match-filter '!is_live & !has_drm' --socket-timeout 30 --retries 3` and the H.264+AAC-first format chain. `does not pass filter` = 直播/DRM，报告后停止，不重试。
- **B 站**：yt-dlp 裸试 → 带 cookies 重试（`--cookies-from-browser` 或 cookies.txt）→ `bili audio "<BV>"` → 写 error sidecar 停止。HTTP 412 = 风控，直接进入下一步降级。
- **YouTube**：先匿名试（匿名播放器常给直链），遇到 "Sign in to confirm" 再带 cookies 重试一次。
- **小宇宙等播客直链**：直接 `curl -L`。
- 抖音/视频号/小红书需要页面签名直链，无浏览器环境不支持——明确报告，不要硬啃。

If the ASR input file is over 200 MB, transcode to 16 kHz mono first (publishing upload still uses the original-quality mp3):
```bash
ffmpeg -i INPUT -ac 1 -ar 16000 -y OUTPUT.wav
```

Keep the `.info.json` — every later step uses it.

## Step 3: Transcribe (local-first, cloud-fallback)

### 3a. Local SenseVoice (default)

```bash
/home/box/tools/sensevoice/.venv/bin/python \
  /home/box/tools/sensevoice/transcribe.py \
  /tmp/lumen-pod/<ID>.wav \
  -o /tmp/lumen-pod/<ID> \
  --language auto
```

This produces `/tmp/lumen-pod/<ID>.json` (lumen-cut `Doc` format with `paragraphs` / `sentences` / `words` + start/end timestamps) and `/tmp/lumen-pod/<ID>.txt` (plain text). Set `processing.asrEngine = "sensevoice-small"`. Move to Step 3c.

### 3b. Volcengine Doubao ASR (only if 3a failed or user asked for cloud)

Follow `references/volcano-setup.md` to call the Doubao 录音文件识别 2.0 endpoint (`volc.seedasr.auc`). Convert the response to lumen-pod `Transcript`. Set `processing.asrEngine = "volcengine-doubao-v2"`.

### 3c. Map real speaker names

After ASR, replace generic labels (`SPEAKER_00`, `Speaker A`) with real names where you can deduce them from `source.title`, `source.author`, `source.podcastName`, and `source.description`. Cross-check proper nouns against those same fields to fix ASR homophone typos (e.g. `Toby Lutke` → `Tobi Lütke`, `Codex` → `Codex`). See `references/speaker-mapping.md` for heuristics.

Build the final `transcript` per the schema in `references/transcript-schema.md`.

## Step 4: Quality gates — fix BEFORE generating content

Run all six gates from `references/quality-gates.md`. Summary (details and fix procedures live there):

- **G1 标题匹配**：`source.title` 必须概括转写内容；营销号式/答非所问的标题要重写（≤60 字），原标题以 `> 原标题：...` 前置进 shownotes。
- **G2 封面**：`python3 scripts/thumbnail.py check "<thumbnailUrl>"`。判黑或缺图且有视频文件时，`extract` 跳过片头黑帧抽一帧备用（发布后上传，见 Step 7）。
- **G3 转写完整**：转写覆盖时长 vs `duration` 偏差 >10%，或文本量 sanity 不达标 → 重跑转写。
- **G4 说话人**：无 `SPEAKER_00` 残留。
- **G5 时间戳**：`[MM:SS]` / `[H:MM:SS]` 规范，无前导零小时；highlights 用句级时间。
- **G6 媒体完整**：文件非 0 字节，`ffprobe` 时长与 `duration` 偏差 <5%。

任何一道修不了 → 写 `/tmp/lumen-pod/<ID>.error.json` 并停止。不要带病发布。

## Step 5: Generate AI content sections

Use the LLM of your host (claude, MiniMax, etc.). Pass the full transcript + metadata as context. Generate these sections in this order — they depend on each other:

### 5a. `summary`
```json
{
  "oneLiner": "<one-sentence thesis>",
  "abstract": "<200-500 word overview, in user's preferred language>",
  "keyTakeaways": ["...", "..."],
  "thinkingQuestions": ["What if...?", "How might...?"],
  "glossary": [{"term": "...", "definition": "..."}]
}
```

### 5b. `chapters`
If `.info.json` had native chapters, preserve their `startTime`/`endTime` and write a detailed `summary` for each. Otherwise, segment by topic change; each chapter needs `title`, `startTime`, `endTime`, `summary`, `hasAds: false`, and an optional `deepRead` (write `null`; it is generated later on demand).

### 5c. `article` — YouMind v54 Read-and-Done

Generate a single markdown article that is so thorough the reader does not need to watch the original. Follow the 8 golden rules in `references/article-golden-rules.md`. The short version:

1. **Scene-First Hook**: open with a concrete scene, conflict, or counter-intuitive fact — never "In this podcast..."
2. **Reasoning over assertions**: walk through HOW and WHY, not just conclusions.
3. **1:1 framework + primary case preservation**: keep every major example in full narrative detail.
4. **Term pinning**: keep product/brand names as-is (e.g. `Cortex`, **not** `Cortex（皮层）`); only annotate domain concepts like `认知负荷（Cognitive Load）`.
5. **Tri-part notable quotes**: 3–6 verbatim quotes, formatted as:
   ```
   > 「精炼传神的中文译文」
   > *"Original verbatim"*
   > —— Speaker Name [MM:SS]
   ```
   (use `[H:MM:SS]` for ≥ 1 hour; the lumen-pod Web UI turns these into clickable player buttons).
6. **High-bar visual referrals**: only when words cannot replace the visual — `🎬 [MM:SS 建议回看原片画面：具体原因]`.
7. **Structured post-article curations** at the end:
   - `## 💡 值得记住的核心认知` (3–5 takeaways)
   - `## 📚 视频提及的书影音与工具`
   - `## 🧭 延伸探索建议` (2–3 follow-up topics)
8. **Timestamps**: `[MM:SS]` under 1 hour, `[H:MM:SS]` at or over 1 hour. Never `[0:11:33]`.

Set `article.format = "markdown"`, `article.lang = "zh"` (or whatever the user prefers), `article.generatedAt = <ISO-8601 now>`.

### 5d. `keywords`
```json
[{"key": "transformer", "desc": "Neural net architecture behind LLMs"}, ...]
```
3–7 entries, each with a one-line description.

### 5e. `highlights`
```json
[{"time": 342.5, "content": "Key insight text", "cueId": "s42"}, ...]
```
Use exact sentence-level timestamps from the transcript, not paragraph opens. 5–10 entries.

### 5f. `qas`
```json
[{"question": "...", "answer": "...", "questionSpeaker": "Host", "answerSpeaker": "Guest"}, ...]
```
3–6 Q&A pairs; only include `*Speaker` if known.

### 5g. `mindmap`
A markmap-flavoured markdown outline (3–5 `##` sections under `# Title`, with `###` leaves):
```markdown
# Episode Title
## Main Topic 1
### Sub-point A
### Sub-point B
## Main Topic 2
### Sub-point C
```

### 5h. `shownotes`
The original platform description (verbatim or lightly cleaned), if it has substance. Otherwise `null`. If G1 rewrote the title, prepend `> 原标题：<original>`.

### 5i. `tags`
Add `["auto-processed"]` plus any topical tags the LLM suggests. Never invent user-facing tags like `"podcast"` — those are platform-managed in the Web UI.

## Step 6: Assemble and publish the episode

Build the full `Episode` JSON per `references/transcript-schema.md`. Set:
- `id = ""` (lumen-pod mcp will assign a UUID)
- `source.url = <original URL>`
- `processing.status = "done"`
- `processing.llmModel = "<your model id>"`
- `processing.agentId = "<your agent id>"` (e.g. `"download-expert-v2"`)
- `processing.processedAt = <ISO-8601 now>`
- `isRead = false`
- `visibility = "public"` (or `"private"` if the user asked)

Publish via MCP:
```
Call pod_publish with episode_json = <the full JSON above as a single string>
```

Expected response:
```json
{"status": "ok", "id": "<uuid>", "message": "Episode published. To add audio, use pod_upload_audio_guide to get the upload instructions."}
```

Save the `id` — you need it for Step 7.

## Step 7: Upload audio + thumbnail

### 7a. Audio (optional but recommended)

```
Call pod_upload_audio_guide with episode_id = <uuid from Step 6>
```

The tool returns up-to-date curl templates. The size rule: **any single HTTP request must stay under 90 MB** — Cloudflare rejects bodies over 100 MB (413/502) before they reach the API.

**File ≤ 90 MB** — single upload:
```bash
ffmpeg -i /tmp/lumen-pod/<ID>.wav -codec:a libmp3lame -qscale:a 2 -y /tmp/lumen-pod/<ID>.mp3
curl -X POST "https://pod.lumenopen.com/api/episodes/<ID>/audio" \
  -H "Authorization: Bearer $LUMEN_POD_API_TOKEN" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @/tmp/lumen-pod/<ID>.mp3
```

**File > 90 MB** — split losslessly and upload parts in order (`?part=N&total=M`, 1-based):
```bash
# N = ceil(size_mb / 85); duration_secs from the .info.json
ffmpeg -i /tmp/lumen-pod/<ID>.mp3 -c copy -f segment \
  -segment_time $((duration_secs / N)) /tmp/lumen-pod/<ID>.part%03d.mp3
for i in 001 002 003; do
  n=$((10#$i))
  curl -X POST "https://pod.lumenopen.com/api/episodes/<ID>/audio?part=$n&total=$N" \
    -H "Authorization: Bearer $LUMEN_POD_API_TOKEN" \
    -H "Content-Type: application/octet-stream" \
    --data-binary @/tmp/lumen-pod/<ID>.part$i.mp3
done
```

Each part replies `{"status":"ok","part":n,"total":N,"complete":false}`; the **final** part must reply `"complete": true` with `"audioPaths"` (length = total). If it does not, re-upload the missing part — do not restart from scratch (re-uploading `part=1` wipes the previous set). The Web player plays parts back-to-back on one continuous timeline.

### 7b. Thumbnail (only when G2 produced a replacement)

```bash
curl -X POST "https://pod.lumenopen.com/api/episodes/<ID>/thumbnail" \
  -H "Authorization: Bearer $LUMEN_POD_API_TOKEN" \
  -H "Content-Type: image/jpeg" \
  --data-binary @/tmp/lumen-pod/<ID>-thumb.jpg
```

Raw image bytes (jpg/png/webp, ≤10MB) — same style as the audio upload. Success sets `source.thumbnailUrl = "/static/thumbs/<ID>.jpg"`. Verify with `GET /api/episodes/<ID>`.

## Step 8: Paper PDF variant

If the URL points to a paper / report PDF instead of audio/video:
- `https://arxiv.org/pdf/<id>.pdf` → download directly with `curl -L -o /tmp/lumen-pod/<id>.pdf`
- Other public PDF host: same, `curl -L -o /tmp/lumen-pod/<NAME>.pdf`
- Publisher page with no public PDF: **stop** and report the paywall, do not look for mirrors

Then run `pdfinfo /tmp/lumen-pod/<FILE>.pdf` and report:
- title (from metadata)
- author
- page count
- file size

The PDF is the deliverable. Do not summarize it. Do not publish it as a lumen-pod Episode. (That is a future skill.)

## Step 9: Report

End with a short message to the user (Chinese + 1-2 English keywords is fine, match the user):

```
完成了。
- 标题: <title>（G1 通过 / 已改写，原标题: <original>）
- 作者: <author>
- 时长: <HH:MM:SS>
- Episode ID: <uuid>
- 链接: https://pod.lumenopen.com/episodes/<uuid>  (or: `pod_get` id=<uuid>)
- 封面: 原图 / 跳黑帧重抽 @<seconds>
- 章节: N 段
- 关键词: M 个
- 金句: K 段
- 音频: 已上传（单片 / N 分片）/ 未上传
- 质量门: G1-G6 全过 / 哪道修了什么
- 文件本地: /tmp/lumen-pod/<ID>.{wav,mp3,info.json}
```

If anything failed mid-way, report which step and the exact error. Do not apologize, do not retry silently — the user decides whether to retry.

## What you never do

- **Never** ship a partial Episode (missing `summary` or `chapters` is fine; missing `transcript` is not). If transcript failed, the episode is `processing.status = "error"`, you tell the user, you stop.
- **Never** publish with a title that fails G1, or a black/placeholder cover that fails G2 — fix or stop.
- **Never** rewrite the user's transcript into a polished narrative unless the user asked. The transcript is the source of truth; the `article` is the reader-friendly version.
- **Never** append a literal dictionary translation in parentheses to product/brand names (`Cortex` not `Cortex（皮层）`).
- **Never** use a leading zero hour in timestamps (`[05:14]` not `[0:05:14]` or `[00:05:14]`).
- **Never** ask the user to paste a token in chat. The token lives in the agent's secret store as `LUMEN_POD_API_TOKEN`.
- **Never** call the cloud ASR unless the local SenseVoice path failed or the user explicitly asked. The whole point of the local-first chain is to avoid the API key.
- **Never** push to Lumen Pod as `"private"` unless the user asked — privacy is admin-gated, default is `"public"`.
- **Never** call `pod_delete` unless the user asked. The local audio file is the only thing you delete yourself (after the upload step).
- **Never** use a SenseVoice path that pulls Whisper or Paraformer — they are explicitly banned. The audit trail is in the original download-expert's `memory/log/2026-09.md`.
- **Never** retry a download whose error says `does not pass filter`（直播/DRM）or `requested format is not available` — report and stop.

## Failure handling

- **No subtitle and SenseVoice fails**: try Volcengine Doubao once. If that also fails, write a sidecar `/tmp/lumen-pod/<ID>.error.json` with the error and stop. Do not publish an episode without a transcript.
- **`pod_publish` returns `{"status": "duplicate", "url": ...}`**: someone already published it. Look up the existing episode with `pod_get` or `pod_search`, report the existing id, and stop.
- **API token rejected (401/403)**: stop. Do not retry. Tell the user: "Lumen Pod rejected the token; check `LUMEN_POD_API_TOKEN` in the agent's secret store." Validate with `GET /api/episodes?limit=1` (200 = token OK) — never diagnose from a single 401 on a JWT-only endpoint like `/api/me`.
- **HTTP 403 with Cloudflare error 1010/1020 (HTML block page, `server: cloudflare`)**: the edge blocked the agent's egress IP before the request reached Lumen Pod. This says nothing about the token — do not report a token problem and do not retry. Report the Cloudflare error code and Ray ID; the zone owner fixes it with a WAF skip rule for `/api/*`.
- **Quality gate unfixable (G1–G6)**: write `/tmp/lumen-pod/<ID>.error.json` naming the gate, the evidence, and stop.
- **LLM returns no text (thinking_tokens exhausted)**: retry once with a tighter `max_tokens` (24,000 → 16,000). If still empty, publish the episode with `processing.status = "partial"` and `summary = null`, `chapters = []`, `article = null`. The transcript is the spine; the rest can be regenerated later.
- **Audio upload > 200 MB**: transcode to `.mp3` first with `ffmpeg -i INPUT -codec:a libmp3lame -qscale:a 2 -y OUTPUT.mp3`. Reject 0-byte files.

## Output contract

The user receives:
- the published episode id (UUID) and a URL to view it on `https://pod.lumenopen.com/`
- a one-paragraph Chinese summary of what the episode is about
- the six gate results (which passed, what was fixed)
- confirmation of which steps succeeded and which failed
- the local file path(s) for any downloaded audio / paper PDF

A run that does not produce an episode id is a failure. Report the failure step, the error, and what would unblock it.

## Files in this folder

- `SKILL.md` (this file) — workflow + rules
- `references/download-playbook.md` — hardened yt-dlp baseline, platform fallback chains, error triage
- `references/quality-gates.md` — the six pre-publish gates (title / cover / transcript / speakers / timestamps / media)
- `references/transcript-schema.md` — the lumen-pod `Transcript` / `Episode` JSON shape
- `references/article-golden-rules.md` — the YouMind v54 8-rule article contract
- `references/speaker-mapping.md` — heuristics for replacing generic speaker labels
- `references/volcano-setup.md` — when and how to call Doubao ASR (only if local SenseVoice failed)
- `scripts/thumbnail.py` — thumbnail black-check + black-frame-skipping extractor
- `examples/` — example `Episode` JSON for YouTube with subtitles / podcast with local ASR
