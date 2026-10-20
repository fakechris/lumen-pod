# Speaker mapping — replacing generic labels with real names

ASR engines (SenseVoice, Whisper, Doubao, YouTube auto-captions) emit generic speaker labels:

- `SPEAKER_00`, `SPEAKER_01`, `SPEAKER_02` (pyannote / pyannote-audio, faster-whisper diarization)
- `Speaker A`, `Speaker B` (some hosted APIs)
- `说话人 1`, `说话人 2` (some Chinese engines)
- `Host` / `Guest` (Apple Podcasts CC, sometimes)
- `Interviewer` / `Interviewee` (YouTube auto-translate)

The lumen-pod Web UI displays the `paragraphs[].speaker` field as the speaker label. Generic labels look bad, get in the way of the transcript view, and break the [MM:SS] quote attribution. **Always map to real names before publishing.**

This file is the heuristic ladder. Run them in order. Stop at the first one that yields a confident mapping.

## Tier 0 — Metadata gives it to you

If `source.title` matches patterns like `"Lex Fridman Podcast #424 — Andrej Karpathy"` or `source.description` starts with `"Host: Lex Fridman. Guest: Andrej Karpathy."`, the answer is in your hand. Apply directly.

## Tier 1 — Self-introduction in the first 60 seconds

Scan `transcript.paragraphs[0..3]` for "I'm [Name]", "my name is [Name]", "大家好我是 [Name]", "欢迎收听 [Show Name]", etc. Extract the names, then assign by role:

- First person to introduce themselves by name → **host** (in solo shows, this is the only speaker)
- Second person to introduce themselves → **guest** (in interview shows)
- Multiple people introducing each other → interview show, host + guest(s)

## Tier 2 — Description / shownotes tell you

Even when `source.description` does not have a literal `"Host: X, Guest: Y"` line, podcast descriptions often say things like:

- "In this episode, host [Name] interviews [Name] about ..."
- "Today we're joined by [Name] from [Company] ..."
- "In this conversation between [Name] and [Name] ..."

Extract both names. If one is the channel name, that's the host.

## Tier 3 — Title-based pattern matching

For well-known shows, the title format alone tells you the speakers:

| Title pattern | Inference |
|---|---|
| `"Lex Fridman Podcast #N"` | `Lex Fridman` = host, all others = guest |
| `"The Joe Rogan Experience #N"` | `Joe Rogan` = host |
| `"a16z Podcast — [Topic]"` | `a16z` = show name; the speakers are a16z partners named in the description |
| `"Acquired [Episode Title]"` | `Ben Gilbert` and `David Rosenthal` = co-hosts (always both) |
| `"Stratechery Interview — [Guest]"` | `Ben Thompson` = host, `Guest` = the other |
| `"[X] 访谈 [Y]"` / `"[X] 对话 [Y]"` | `X` = host, `Y` = guest |

This is not a complete list. Build your own per-show database as you go.

## Tier 4 — Self-reference in speech

When speakers refer to each other in the third person:

- "[Name] you mentioned earlier that..." → the **other** speaker is `[Name]`
- "I think what [Name] was saying..." → same
- "正如 [Name] 刚才说的" → same

This works in two-speaker interviews. In multi-speaker panels it is more error-prone.

## Tier 5 — Speech-pattern signature

If you have multi-speaker audio and no metadata, you can use **speech pattern fingerprinting** as a last resort:

1. Run `pyannote-audio` (or your diarizer of choice) for a clean `SPEAKER_00..N` mapping.
2. Count the total words per speaker.
3. In a typical interview / podcast: the host speaks more, the guest speaks more substantively. The host's turns are short and reactive; the guest's turns are long and expository.
4. Match the largest non-host speaker against known guests in `source.description` / `source.podcastName`.

This is **low confidence** and should always be reviewed against the transcript for sanity. If you cannot map confidently, **leave the generic label and tell the user** in the final report — do not guess wrong.

## What to do when the mapping fails

If you cannot produce a confident mapping, do one of:

- **Leave the generic label** (`SPEAKER_00`, `Speaker A`). The Web UI tolerates it. A future "regenerate" run can fix it.
- **Mark the speaker as `null`** (lumen-pod Episode schema allows `speaker: Option<String>`). The Web UI shows "Speaker 1" / "Speaker 2" or just an unnamed turn. Cleaner than a wrong name.
- **Tell the user** in the final report: "could not map speakers; transcript published with original SPEAKER_00 labels. Add a known host name to retry."

Never invent a name.

## What to do for solo shows

If the entire episode is one person (a monologue, a TED talk, a single-host podcast), there is no mapping to do. Set all `paragraphs[].speaker` to the same name (the host) and move on.

## Examples

### Example 1: Lex Fridman Podcast

```
URL: https://youtube.com/watch?v=ABC123
source.title: "Lex Fridman Podcast #424 — Andrej Karpathy"
source.description: "Andrej Karpathy is a founding team member of OpenAI ..."
```

Mapping:
- `SPEAKER_00` (more words, short reactive turns) → `Lex Fridman`
- `SPEAKER_01` (fewer turns, long expository) → `Andrej Karpathy`

### Example 2: 小宇宙中文播客 (no metadata)

```
URL: https://xiaoyuzhoufm.com/episode/abc123
source.title: "E30 与宝玉聊 AI 创业"
source.description: ""
```

In the first 3 paragraphs:
- p1: "大家好，欢迎收听硬地骇客，我是主持人 Jack。"
- p3: "今天我们请到了宝玉老师，宝玉先跟大家打个招呼。"

Mapping:
- `SPEAKER_00` → `Jack`
- `SPEAKER_01` → `宝玉`

## Anti-patterns

- **Never** trust `SPEAKER_00` to always be the host. In some interview shows, the host speaks less (good interview hosts are good listeners).
- **Never** use only word count. Counter-example: some hosts have solo monologues (e.g. Stratechery weekly) where the host is the only speaker.
- **Never** assume the channel name = the host name. `"Acquired"` is a show name; the hosts are `Ben Gilbert` and `David Rosenthal`.
- **Never** map by gender stereotypes. The signal is in the words and turns, not the voice.
