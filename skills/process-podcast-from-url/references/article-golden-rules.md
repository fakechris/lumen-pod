# Article golden rules — YouMind v54 "Read-and-Done"

This is the article contract the lumen-pod Web UI and the Episode `article.content` field both expect. It is modeled after the YouMind v54 philosophy: a long-form article so thorough the reader does not need to watch the original.

Source of truth: `crates/lumen-pod-api/src/main.rs::article_system_prompt` and the `skills/process-episode/SKILL.md` (lumen-pod repo). The same constraints are enforced both in the prompt and in the system message.

The lumen-pod LLM uses an Anthropic-compatible endpoint (`LLM_BASE_URL` / `LLM_MODEL` env, defaults to `https://api.minimaxi.com/anthropic` + `MiniMax-M3.1-Flash-Preview`). The agent calling this skill generates the article in its own LLM call, then writes the resulting markdown into the Episode JSON. Same shape, same rules.

## 8 golden constraints (apply in order)

### 1. Scene-First Hook (具体画面开场)

Open with a **concrete scene, conflict, counter-intuitive data point, or specific story** from the episode. Within 200 words, the reader should know what they will get and why it is worth reading.

❌ Bad: "This video is about why the speaker believes the AI race is more about data than compute."

✅ Good: "On a Tuesday in March 2024, a small team at a research lab in Zürich watched a 7-billion-parameter model score 92% on a math benchmark the lab had spent two years treating as unsolvable. They had not trained the model. They had only fed it more text. The implications of that one fact are what this episode is actually about."

**Stay 100% faithful to the transcript.** Do not invent. If a speaker played a pre-recorded demo video during a live session for privacy reasons, you write "pre-recorded demo video" — never "they stopped recording."

### 2. Reasoning over assertions (讲清观点背后的思考过程)

Do not reduce the content to bullet-point conclusions. Walk the reader through the **how and why**: what conventional wisdom the speaker challenges, what intermediate steps they take, what trade-offs they weigh. If the speaker says "X is the future," the article should explain the chain of reasoning that led them to that claim, not just the claim.

### 3. 1:1 framework + primary case preservation (1:1 保留讲者框架与主案例细节)

Mirror the speaker's logical progression using clear `##` section headings (one heading per major turn in the argument).

Under every major section, you **must** preserve the primary example, experiment, story, or dataset in full narrative detail. **Never** compress a core story into a single summary sentence. If the speaker spent 4 minutes on an experiment, your article gives the experiment 2-3 paragraphs.

### 4. Term pinning & ASR correction (专有名词核对与原词锚定)

Cross-check person names, company names, books, and technical terms against `source.title`, `source.author`, `source.podcastName`, and `source.description`. Fix ASR homophone errors (e.g. `Toby Lutke` → `Tobi Lütke`, `Codex` → `Codex`).

**Keep product / brand / company / internal system names in their original form, never with a literal dictionary translation in parentheses.** Examples:

- `Cortex` — not `Cortex（皮层）`
- `Notion` — not `Notion（概念）`
- `Linear` — not `Linear（线性）`
- `Slite`, `dbt`, `GBrain`, `Anthropic`, `OpenAI` — all stay as-is

**Only** attach `中文译名（Original Term）` for **domain concepts, methodologies, or technical terms** when they first appear. Examples:

- `认知负荷（Cognitive Load）`
- `上下文图谱（Context Graph）`
- `检索增强生成（Retrieval-Augmented Generation, RAG）`

### 5. Tri-part notable quotes (三段式高规格金句引用)

Include **3–6 of the most powerful verbatim quotes** from the episode. Use the **exact sentence-level timestamp** placed immediately before that quoted sentence in the transcript — not the paragraph's opening timestamp, not a round number.

Format each quote as a three-line blockquote:

```
> 「精炼传神的中文译文（或目标语言译文）」
> *"Exact original quote, word for word, from the speaker"*
> —— Speaker Name [MM:SS]
```

The lumen-pod Web UI and Chrome Extension turn `[MM:SS]` and `[H:MM:SS]` markers into click-to-seek player buttons automatically. **Do not manually hyperlink the timestamp; just write the bare marker.**

If the source language and target language are the same (e.g. English source, English article), you may omit the duplicate line and keep:

```
> "Exact original quote"
> —— Speaker Name [MM:SS]
```

### 6. High-bar visual referrals (高门槛的「回看原片画面」指针)

Only when words genuinely cannot replace the visual — a live UI/code demo, a physical experiment, a complex chart walkthrough, a live security demo — insert:

```
🎬 [MM:SS 建议回看原片画面：具体原因]
```

For all verbal arguments and stories, explain them completely in text. **Most episodes will have 0–2 of these markers; a transcript-only podcast should have 0.**

### 7. Unified timestamp format (统一时间戳格式)

- **Under 1 hour** (`< 60:00`): always `[MM:SS]` with two-digit minutes, e.g. `[05:14]`, `[11:33]`.
- **At or over 1 hour** (`>= 1:00:00`): always `[H:MM:SS]`, e.g. `[1:02:15]`, `[2:10:05]`.
- **Never** prefix with a leading zero hour like `[0:11:33]` or `[00:11:33]`, even if the episode is over an hour long.

The `MM` in `[MM:SS]` is `floor(seconds / 60)` modulo 60. The `H` in `[H:MM:SS]` is `floor(seconds / 3600)`. So `[1:11:33]` is 1h 11m 33s, **not** 1m 11s.

### 8. Structured post-article curations (结构化尾注)

Always end the article with these three sections in this order:

#### `## 💡 值得记住的核心认知`

3–5 sharp cognitive takeaways. Each takeaway is a single sentence (≤ 30 words), and it is the kind of sentence the reader can quote in a meeting tomorrow. Not a recap — an actual cognitive shift.

#### `## 📚 视频提及的书影音与工具`

A structured list of every book, paper, film, product, tool, or notable figure mentioned in the episode. Original names (with original-language author/title for books and papers), and one short sentence of context for each. If the episode mentioned nothing notable, write "本期未提及具体书影音或工具。" (or the language-equivalent).

#### `## 🧭 延伸探索建议`

2–3 genuinely relevant follow-up topics, search queries, or questions for active learning. **Not** "go read more about the same thing" — actually point at adjacent territory the curious reader would want to enter.

## Structural skeleton (do not deviate)

```markdown
# Compelling, faithful article title

[Scene-first hook, 150-250 words]

[##  — Mirror the speaker's first major turn]
[Reasoning + primary case in full narrative detail]
[Tri-part quote with [MM:SS] / [H:MM:SS]]
[🎬 if a visual is irreplaceable]

[##  — Next major turn]
[Same structure]

...

## 💡 值得记住的核心认知
- ...
- ...
- ...

## 📚 视频提及的书影音与工具
- **Original Title** by Author — context
- ...

## 🧭 延伸探索建议
- ...
- ...
```

The title is a single `# Title` line at the very top. No frontmatter, no byline, no abstract. The article body starts immediately after.

## Length and tone

- **Length**: 2,500–6,000 words for a 60–90 minute episode. The 1:1 case preservation rule means you cannot short-cut; if the speaker spent 4 minutes on a story, you give it 200–400 words.
- **Tone**: confident, precise, faithful. The reader is trusting you to be a faithful curator, not a paraphraser. The voice of a senior editor who has watched the whole thing and is now handing the reader the spine.
- **Language**: match the user's preferred language. Default to `zh` for Chinese users, `en` for English. The `article.lang` field records what you wrote; the Web UI may or may not translate on the fly.

## What the lumen-pod server does with this

When the agent calls `pod_publish` with `article.format = "markdown"` and a non-empty `article.content`, the Web UI:

1. Renders the markdown in the Episode detail page (right tab).
2. Wires every `[MM:SS]` / `[H:MM:SS]` marker to seek the audio/video player.
3. Cross-links `## 💡 值得记住的核心认知` items to the corresponding `highlights[].time` (if any).
4. Surfaces the article title as the Episode's "long-form read" entry point.

If `article.content` is empty or `"No response from AI"`, the Web UI shows an "AI summary not yet generated" placeholder and offers a "Generate" button that hits `POST /api/episodes/<id>/article`.
