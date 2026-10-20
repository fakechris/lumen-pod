# Transcript & Episode schema for lumen-pod

This is the exact JSON shape lumen-pod's `Store::insert_episode` accepts. All field names are **camelCase**. Optional fields can be omitted (they get a sensible default). Top-level `id` is auto-generated if you pass an empty string.

Source of truth: `crates/lumen-pod-core/src/episode.rs` and `crates/lumen-pod-mcp/src/lib.rs::tools()`.

## Top-level Episode

```json
{
  "id": "",
  "source": { ... },
  "processing": { ... },
  "summary": { ... } | null,
  "chapters": [ ... ],
  "mindmap": { ... } | null,
  "transcript": { ... } | null,
  "keywords": [ ... ],
  "highlights": [ ... ],
  "qas": [ ... ],
  "clips": [],
  "article": { ... } | null,
  "shownotes": "..." | null,
  "isRead": false,
  "tags": ["auto-processed"],
  "collectionId": null,
  "ownerId": null,
  "visibility": "public",
  "createdAt": "2026-10-18T11:00:00Z",
  "updatedAt": "2026-10-18T11:00:00Z"
}
```

The host fills `createdAt` / `updatedAt` on insert — pass them or omit. Same for `ownerId` (only the API-token viewer sets it to `null`; OAuth viewers get their user id).

## `source` (required)

```json
{
  "url": "https://...",
  "platform": "youtube | bilibili | xiaoyuzhou | twitter | apple | spotify | other",
  "title": "...",
  "author": "...",
  "publishedAt": "2024-01-01T00:00:00Z" | null,
  "thumbnailUrl": "https://..." | null,
  "duration": 3600.0,
  "feedUrl": null,
  "description": "..." | null,
  "podcastName": "..." | null,
  "episodeCoverUrl": "..." | null,
  "language": "en | zh | ja | ..." | null,
  "audioPath": null
}
```

`audioPath` is filled by the server after `pod_upload_audio_guide` runs (it becomes `"/audio/<episode_id>.mp3"`).

## `processing` (required)

```json
{
  "status": "pending | processing | done | error",
  "asrEngine": "youtube-subs | sensevoice-small | volcengine-doubao-v2 | ...",
  "llmModel": "claude-4-opus | MiniMax-M3.1-Flash-Preview | ...",
  "processedAt": "2024-01-01T00:00:00Z" | null,
  "agentId": "<your agent id>",
  "error": null
}
```

`status` defaults to `"pending"` if you set nothing; the Web UI will treat a `"pending"` episode as in-flight. Set `"done"` on successful publish, `"error"` on failure (with a message in `error`).

## `summary` (optional but recommended)

```json
{
  "oneLiner": "One sentence thesis of the episode.",
  "abstract": "200-500 word overview in the user's preferred language.",
  "keyTakeaways": ["Take 1", "Take 2", "Take 3"],
  "thinkingQuestions": ["What if...?", "How might...?"],
  "glossary": [
    {"term": "Cognitive Load", "definition": "Total mental effort in working memory."}
  ]
}
```

`abstract` and `oneLiner` are required if you set the field. `thinkingQuestions` and `glossary` default to `[]`.

## `chapters` (optional)

```json
[
  {
    "title": "Introduction",
    "startTime": 0.0,
    "endTime": 120.0,
    "summary": "The host introduces today's topic...",
    "hasAds": false,
    "deepRead": null
  }
]
```

`deepRead` is filled lazily by `POST /api/episodes/<id>/chapters/<idx>/deep-read` — you do not need to write it during `pod_publish`.

## `transcript` (optional but recommended)

The shape is intentionally compatible with **lumen-cut**'s `Doc` (so you can pipe lumen-cut's ASR output directly). If your ASR produces something else, convert it to this shape.

```json
{
  "paragraphs": [
    {
      "id": 1,
      "speaker": "Lex Fridman",
      "sentences": [
        {
          "id": "s1",
          "text": "Welcome to the show.",
          "words": [
            {"id": "w0", "text": "Welcome", "start": 0.0, "end": 0.5}
          ]
        }
      ]
    }
  ],
  "translations": {
    "zh": {
      "s1": {"text": "欢迎收听本期节目。"}
    }
  }
}
```

- `paragraphs[].id` is `u32`. Sequential is fine.
- `paragraphs[].speaker` is `Option<String>`. Use the real name once you have mapped it (see `speaker-mapping.md`); before that, keep whatever the ASR gave you.
- `paragraphs[].sentences[].id` is a `String` (not a number). Use `"s1"`, `"s2"`, ... or any unique string. Other fields (e.g. `highlights[].cueId`, translations) reference these ids.
- `paragraphs[].sentences[].words[].start` / `end` are **seconds** (float, not MM:SS).
- `translations` is a 2-level `BTreeMap` keyed by language code → sentence id → translation text. Skip if you are not generating a translation.

## `keywords` (optional)

```json
[
  {"key": "transformer", "desc": "Neural net architecture behind modern LLMs."},
  {"key": "RLHF", "desc": "Reinforcement learning from human feedback."}
]
```

`desc` is optional.

## `highlights` (optional)

```json
[
  {"time": 342.5, "content": "Key insight: local models will surpass cloud in 2 years.", "cueId": "s42", "note": null}
]
```

- `time` is seconds (float). Web UI turns this into a clickable seek button.
- `cueId` is optional; if you set it, the Web UI scrolls the transcript to that sentence when the highlight is clicked.

## `qas` (optional)

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

`questionSpeaker` / `answerSpeaker` are optional.

## `clips` (optional, user-generated)

You almost never set this from an automated agent. Leave it `[]`.

## `article` (optional but recommended — YouMind v54 Read-and-Done)

```json
{
  "title": "Compelling, faithful article title",
  "content": "Full markdown article...",
  "format": "markdown | blog | newsletter | xiaohongshu | wechat",
  "lang": "zh | en | ja | ...",
  "focus": "user-supplied angle" | null,
  "generatedAt": "2026-10-18T11:00:00Z"
}
```

`focus` lets the user steer the article to a particular angle. Leave `null` for the default.

## `mindmap` (optional)

```json
{
  "markdown": "# Title\n## Topic 1\n### Sub A\n### Sub B\n## Topic 2\n### Sub C",
  "opml": null
}
```

`opml` is for XMind/MindNode export — leave `null` unless you really need it.

## `tags` (optional)

```json
["auto-processed", "ai", "claude", "..."]
```

`"auto-processed"` is the convention. Add a few topical tags (1-5 max).

## `visibility`

`"public"` (default) or `"private"`. Only set `"private"` when the user asked. Private episodes require `can_publish_private` permission (admins have it by default; OAuth users need to be granted).

## MCP tool: `pod_publish` argument shape

```
Call pod_publish with:
  episode_json: <STRING — the full Episode JSON above, single-quoted or in a heredoc>
```

The tool auto-assigns `id` if you pass `""`, auto-fills `createdAt` / `updatedAt`, and rejects duplicates:

```json
{"status": "duplicate", "url": "..."}
```

If you pass a `LUMEN_POD_API_TOKEN` bearer (or your runtime already has one in the env), the API-token viewer takes ownership automatically.

## MCP tool: `pod_update` argument shape

```
Call pod_update with:
  id: <episode uuid>
  updates_json: <STRING — only the fields you want to change>
```

Both camelCase and snake_case are accepted. Example: setting audio path after upload:

```json
{"source": {"audioPath": "/audio/<episode_id>.mp3", "url": "https://...", "title": "...", "...": "..."}}
```

(yes, you have to re-send the full `source` block — partial nested updates are not supported)

## REST: raw audio upload

`POST /api/episodes/<id>/audio` — body is the raw audio bytes. Headers:

```
Authorization: Bearer <LUMEN_POD_API_TOKEN>
Content-Type: application/octet-stream
```

Server response:

```json
{"status": "ok", "audioPath": "/audio/<id>.mp3", "format": "mp3", "sizeBytes": 12345678}
```

The server then `PATCH`-es the episode so `source.audioPath = "/audio/<id>.mp3"`. The Web UI swaps the YouTube/Bilibili iframe for a self-hosted `<audio>` tag automatically.

## Other REST endpoints you might need

| Endpoint | Use |
|---|---|
| `GET /api/episodes?limit=N&offset=M` | list episodes (requires Bearer) |
| `GET /api/episodes/<id>` | full episode (Bearer) |
| `PATCH /api/episodes/<id>` | update fields (Bearer, must be owner/admin) |
| `DELETE /api/episodes/<id>` | delete (Bearer, admin) |
| `GET /api/episodes/lookup?url=<url>` | find episode by source URL (Bearer) |
| `GET /api/search?q=<query>&limit=N` | full-text search across transcripts (Bearer) |
| `GET /api/me` | current viewer info (Bearer) |
| `POST /api/episodes/<id>/chat` | per-episode AI chat (Bearer) |
| `POST /api/episodes/<id>/article?lang=zh&focus=...` | regenerate article (Bearer) |
| `POST /api/episodes/<id>/chapters/<idx>/deep-read?lang=zh` | regenerate chapter deep-read (Bearer) |
| `POST /api/episodes/<id>/deep-read-all?lang=zh` | batch deep-read all chapters (Bearer) |

All of these require either a valid JWT session cookie or the `LUMEN_POD_API_TOKEN` Bearer. The agent uses the Bearer.
