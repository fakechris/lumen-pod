# lumen-pod: Process Episode from URL

Process a podcast/video URL into a fully structured Episode with AI-generated content, then publish to lumen-pod via MCP.

## Prerequisites

- `yt-dlp` installed for media download
- `lumen-cut` MCP server connected (for ASR transcription)
- `lumen-pod` MCP server connected (for publishing)
- LLM access for summary/chapter/Q&A generation

## Workflow

### Step 1: Check if already processed

```
Call pod_exists with the source URL.
If exists → report "already processed" and stop.
```

### Step 2: Download audio with yt-dlp

```bash
yt-dlp -x --audio-format wav --audio-quality 0 \
  -o "/tmp/lumen-pod/%(id)s.%(ext)s" \
  --write-info-json \
  "<URL>"
```

Read the `.info.json` file to extract metadata:
- `title`, `uploader` (author), `duration`, `description`
- `thumbnail`, `upload_date`, `webpage_url`
- `channel` (podcast_name), `language`

### Step 3: Transcribe with lumen-cut

Use lumen-cut's MCP tools or CLI to transcribe the audio:

```
Call lumen-cut ASR on the downloaded .wav file.
Result: Doc with paragraphs → sentences → words (with timestamps + speaker diarization).
```

Convert lumen-cut Doc format to lumen-pod Transcript format:
```json
{
  "paragraphs": [
    {
      "id": 1,
      "speaker": "Speaker A",
      "sentences": [
        {
          "id": "s1",
          "text": "Welcome to the show",
          "words": [
            {"id": "w0", "text": "Welcome", "start": 0.0, "end": 0.5},
            ...
          ]
        }
      ]
    }
  ]
}
```

### Step 4: Generate AI content

Using the transcript text and metadata, generate the following with LLM:

#### 4a. Summary
```json
{
  "oneLiner": "One sentence summarizing the episode",
  "abstract": "200-500 word overview of the episode content",
  "keyTakeaways": ["Point 1", "Point 2", ...],
  "thinkingQuestions": ["What if...?", "How might...?"],
  "glossary": [
    {"term": "LLM", "definition": "Large Language Model"}
  ]
}
```

#### 4b. Chapters
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

#### 4c. Keywords
```json
[
  {"key": "artificial intelligence", "desc": "The simulation of human intelligence by machines"},
  {"key": "transformer", "desc": "Neural network architecture behind modern LLMs"}
]
```

#### 4d. Q&A
```json
[
  {
    "question": "What is the main argument?",
    "answer": "The speaker argues that...",
    "questionSpeaker": "Host",
    "answerSpeaker": "Guest"
  }
]
```

#### 4e. Highlights
```json
[
  {
    "time": 342.5,
    "content": "Key insight: local models will surpass cloud in 2 years",
    "cueId": "s42"
  }
]
```

#### 4f. Mindmap (Markmap markdown)
```markdown
# Episode Title
## Main Topic 1
### Sub-point A
### Sub-point B
## Main Topic 2
### Sub-point C
```

#### 4g. Shownotes
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
    "asrEngine": "qwen3-asr",
    "llmModel": "claude-4-opus",
    "agentId": "cursor"
  },
  "summary": { ... },
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
- Episode title and author
- Number of chapters, keywords, Q&A pairs
- One-liner summary
- Link to view on the web UI (if available)

## Prompt Template for LLM Content Generation

```
You are analyzing a podcast/video transcript to extract structured knowledge.

Title: {title}
Author: {author}
Duration: {duration_minutes} minutes

Full transcript:
{transcript_text}

Generate the following in JSON format:

1. **summary**: oneLiner (1 sentence), abstract (200-500 words), keyTakeaways (3-7 points), thinkingQuestions (2-3 deep questions), glossary (key terms with definitions)

2. **chapters**: Array of chapters with title, startTime, endTime (in seconds), summary, hasAds (boolean - detect sponsor/ad segments)

3. **keywords**: Array of {key, desc} - 5-15 important terms with brief descriptions

4. **qas**: Array of {question, answer, questionSpeaker, answerSpeaker} - 3-8 key Q&A pairs from the content

5. **highlights**: Array of {time (seconds), content} - 5-10 most notable moments

6. **mindmap**: Markdown-formatted mindmap (use # headings, max 3 levels deep)

Return valid JSON with these 6 keys.
```

## Error Handling

- If yt-dlp fails → report "download failed" with error
- If ASR fails → publish with `processing.status = "error"` and `processing.error` message
- If LLM generation fails → publish with transcript only (summary/chapters/etc. will be null)
- Network errors → retry up to 3 times with exponential backoff

## Supported Platforms

yt-dlp supports 1000+ platforms including:
- YouTube, Bilibili, Twitter/X, TikTok
- Apple Podcasts, Spotify (with cookies)
- Any direct audio/video URL
