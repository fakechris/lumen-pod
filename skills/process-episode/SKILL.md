# lumen-pod: Process Episode from URL (YouMind v54 "Read-and-Done" Enhanced)

Process a podcast/video URL into a fully structured Episode with AI-generated content — including a standalone "Read-and-Done" long-form `article` — then publish to `lumen-pod` via MCP.

## Prerequisites

- `yt-dlp` installed for metadata, subtitles, and media download
- `lumen-cut` MCP server connected (for fallback ASR transcription when subtitles are unavailable)
- `lumen-pod` MCP server connected (for publishing)
- LLM access for summary / chapters / Q&A / Read-and-Done article generation

## Workflow

### Step 1: Check if already processed

```
Call pod_exists with the source URL.
If exists → report "already processed" and stop.
```

### Step 2: Subtitle-First Fast Track (or Audio ASR Fallback)

#### 2a. Try Fast Subtitle & Metadata Extraction First (Recommended for YouTube/Bilibili)

Before downloading full audio, extract `.info.json` and check if human or high-quality auto-captions exist:

```bash
yt-dlp --write-info-json \
  --write-subs --write-auto-subs \
  --sub-langs "en.*,zh.*,ja.*" \
  --sub-format "json3/vtt/srt" \
  --skip-download \
  -o "/tmp/lumen-pod/%(id)s.%(ext)s" \
  "<URL>"
```

Read the `.info.json` file to extract metadata:
- `title`, `uploader` (author), `duration`, `description`
- `thumbnail`, `upload_date`, `webpage_url`
- `channel` (`podcastName`), `language`, and native `chapters` (if present in `.info.json`)

If a subtitle file (`.json3`, `.vtt`, or `.srt`) is downloaded:
- Parse the subtitle cues directly into `lumen-pod` `Transcript` format (`paragraphs → sentences → words` with `start`/`end` timestamps) and set `processing.asrEngine = "youtube-subs"`.
- Skip Step 2b/Step 3 ASR unless you specifically need multi-speaker diarization on raw audio.

#### 2b. Fallback: Download Audio & Transcribe with `lumen-cut`

If no usable subtitles exist, download the audio and run ASR:

```bash
yt-dlp -x --audio-format wav --audio-quality 0 \
  -o "/tmp/lumen-pod/%(id)s.%(ext)s" \
  --write-info-json \
  "<URL>"
```

### Step 3: Transcribe & Map Real Speaker Names (Metadata-First Speaker Identification)

1. If using `lumen-cut`, call `lumen-cut` ASR on the downloaded `.wav` file to obtain `Doc` (`paragraphs → sentences → words`).
2. **Metadata-First Speaker Mapping**:
   - Inspect `source.title`, `source.author`, `source.podcastName`, and `source.description` (e.g. `"Host: Lex Fridman, Guest: Andrej Karpathy"`).
   - Replace generic labels (`Speaker A`, `Speaker B`, `SPEAKER_00`) with the real speaker names whenever identifiable from context and self-introductions.
3. **ASR Proper Noun & Term Verification**:
   - Cross-check person names, book titles, company names, and technical terms against `title` and `description` to fix ASR homophone typos.

Convert to `lumen-pod` `Transcript` format:
```json
{
  "paragraphs": [
    {
      "id": 1,
      "speaker": "Andrej Karpathy",
      "sentences": [
        {
          "id": "s1",
          "text": "Welcome to the show",
          "words": [
            {"id": "w0", "text": "Welcome", "start": 0.0, "end": 0.5}
          ]
        }
      ]
    }
  ],
  "translations": {}
}
```

### Step 4: Generate AI Content (including YouMind v54 "Read-and-Done" Article)

Using the transcript text and metadata, generate the following sections:

#### 4a. Summary
```json
{
  "oneLiner": "One sentence summarizing the core thesis of the episode",
  "abstract": "200-500 word overview of the episode content",
  "keyTakeaways": ["Point 1", "Point 2"],
  "thinkingQuestions": ["What if...?", "How might...?"],
  "glossary": [
    {"term": "Cognitive Load", "definition": "The total amount of mental effort being used in working memory"}
  ]
}
```

#### 4b. Chapters
If `.info.json` contains native creator chapters, preserve their boundaries and enrich each chapter with a detailed `summary` (and optional `deepRead`). Otherwise, segment the episode logically:
```json
[
  {
    "title": "Introduction",
    "startTime": 0.0,
    "endTime": 120.0,
    "summary": "The host introduces today's topic...",
    "hasAds": false
  }
]
```

#### 4c. Standalone "Read-and-Done" Article (`article`) — YouMind v54 Golden Rules
Generate a complete, standalone long-form article so thorough that the reader does not need to watch the original video:
```json
{
  "title": "Compelling, faithful article title",
  "content": "Full Markdown article following the 8 golden constraints below...",
  "format": "markdown",
  "lang": "zh"
}
```

**8 Golden Constraints for `article.content`**:
1. **Scene-First Hook (具体画面开场)**: Never open with "This video talks about..." or "In this podcast...". Start directly with a concrete scene, conflict, counter-intuitive statistic, or specific story from the episode so the reader can judge within 200 words why it is worth reading.
2. **Reasoning over Assertions (重推理轻结论)**: Do not just list conclusions. Reconstruct HOW and WHY the speaker arrives at each insight — what conventional wisdom they challenge and how they deduce the conclusion.
3. **1:1 Framework + Primary Case Preservation (1:1 保留讲者框架与主案例细节)**: Mirror the speaker's logical structure using `##` headings. Under every major branch, preserve the primary example, experiment, or dataset in full narrative detail — never compress a core story into one sentence.
4. **Term Pinning & ASR Correction (专有名词核对与原词锚定)**: Fix ASR homophone errors using metadata. When a key concept or technical term first appears, include the original term in parentheses: `认知负荷（Cognitive Load）`.
5. **Tri-part Notable Quotes (三段式高规格金句引用)**: Include 3–6 verbatim quotes formatted as a three-line blockquote (use the exact sentence-level timestamp `[MM:SS]` or `[H:MM:SS]`, NOT the paragraph opening timestamp):
   > 「精炼传神的中文译文」
   > *"Exact original wording from the speaker"*
   > —— Speaker Name `[MM:SS]` (or `[H:MM:SS]` when >= 1 hour)
6. **High-Bar Visual Referrals (高门槛的「回看原片画面」指针)**: Only when words genuinely cannot replace the visual (live UI/code demo, physical experiment, complex chart), insert `🎬 [MM:SS 建议回看原片画面：具体原因]` (or `🎬 [H:MM:SS 建议回看原片画面：具体原因]` when >= 1 hour).
7. **Structured Post-Article Curations (结构化尾注)**: Always end the article with:
   - `## 💡 值得记住的核心认知` (3–5 sharp cognitive takeaways)
   - `## 📚 视频提及的书影音与工具` (All books, papers, films, products, tools, or people mentioned, with original names and context)
   - `## 🧭 延伸探索建议` (2–3 genuinely relevant follow-up topics or search queries)
8. **Interactive Timestamps**: For timestamps under 1 hour (`< 60:00`), ALWAYS use `[MM:SS]` with two-digit minutes (e.g. `[05:14]`, `[11:33]`) and NEVER prefix with `0:` like `[0:11:33]`. For timestamps at or over 1 hour (`>= 1:00:00`), use `[H:MM:SS]` (e.g. `[1:02:15]`). The `lumen-pod` Web UI and Chrome Extension automatically turn them into click-to-seek player buttons.

#### 4d. Keywords
```json
[
  {"key": "artificial intelligence", "desc": "The simulation of human intelligence by machines"},
  {"key": "transformer", "desc": "Neural network architecture behind modern LLMs"}
]
```

#### 4e. Q&A
```json
[
  {
    "question": "What is the main argument?",
    "answer": "The speaker argues that...",
    "questionSpeaker": "Lex Fridman",
    "answerSpeaker": "Andrej Karpathy"
  }
]
```

#### 4f. Highlights
```json
[
  {
    "time": 342.5,
    "content": "Key insight: local models will surpass cloud in 2 years",
    "cueId": "s42"
  }
]
```

#### 4g. Mindmap (Markmap markdown)
```markdown
# Episode Title
## Main Topic 1
### Sub-point A
### Sub-point B
## Main Topic 2
### Sub-point C
```

#### 4h. Shownotes
Use the original description from the source platform.

### Step 5: Assemble and publish

Assemble the complete Episode JSON:

```json
{
  "id": "",
  "source": {
    "url": "<original URL>",
    "platform": "youtube",
    "title": "...",
    "author": "...",
    "publishedAt": "2024-01-01T00:00:00Z",
    "thumbnailUrl": "...",
    "duration": 3600.0,
    "description": "...",
    "podcastName": "...",
    "language": "en"
  },
  "processing": {
    "status": "done",
    "asrEngine": "youtube-subs",
    "llmModel": "claude-4-opus",
    "agentId": "cursor"
  },
  "summary": { ... },
  "article": {
    "title": "...",
    "content": "...",
    "format": "markdown",
    "lang": "zh"
  },
  "chapters": [ ... ],
  "mindmap": { "markdown": "..." },
  "transcript": { ... },
  "keywords": [ ... ],
  "highlights": [ ... ],
  "qas": [ ... ],
  "clips": [],
  "shownotes": "...",
  "tags": ["auto-processed"],
  "isRead": false
}
```

Publish via MCP:
```
Call pod_publish with episode_json = <JSON string of the above>
```

### Step 6: Report result

Report to the user:
- Episode title and author (with identified speaker names)
- Article title and a preview of the Scene-First opening
- Number of chapters, keywords, Q&A pairs, and mentioned books/tools
- Link to view on the web UI (if available)

## Error Handling

- If subtitle extraction finds no subtitles → automatically fall back to audio download + `lumen-cut` ASR
- If `yt-dlp` fails → report "download failed" with error
- If ASR fails → publish with `processing.status = "error"` and `processing.error` message
- If LLM generation fails → publish with transcript only (summary/chapters/article will be null)
- Network errors → retry up to 3 times with exponential backoff
