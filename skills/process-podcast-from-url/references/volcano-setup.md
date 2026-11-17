# Volcengine Doubao ASR — when and how to use it

This is the **fallback** path. The local SenseVoice path in Step 3a of `SKILL.md` is the default — it is free, fast (RTF ~0.4 on the box's 8-core Xeon), and needs no API key. **Only** use Volcengine when the local path fails or the user explicitly asked for cloud ASR.

This file is a trimmed extract of the original download-expert skill (`skill-1788317010220` in `/home/box/sand-data/workflows/`). It is kept here so the new agent that runs `process-podcast-from-url` does not need to read the original skill to fall back.

## When to fall back

Move to Volcengine if **any** of:

1. Local SenseVoice crashed mid-run (exit code non-zero, Python traceback in stderr)
2. Output transcript is empty (paragraphs list is `[]`)
3. Output transcript is non-empty but obviously garbage (random characters, all `???`, more than 30% of the same token repeated — a sign the int8 model collapsed)
4. The user explicitly asked for cloud ASR
5. The audio is over 3 hours (SenseVoice's VAD was tuned for 25-second windows; very long files can lose speaker boundaries)

In every other case, ship the local transcript.

## Before you start

You need a Volcengine account and a 豆包语音 (Doubao ASR) API key. Two consoles, two token shapes:

- **New console (火山引擎控制台 → 语音技术 → 录音文件识别)**: gives you a single `VOLCENGINE_ASR_API_KEY`. Send it in the `X-Api-Key` request header. Use this one if you can.
- **Old console (旧版语音控制台)**: gives you a `VOLCENGINE_ASR_APP_ID` + `VOLCENGINE_ASR_ACCESS_TOKEN`. Send both in the `X-Api-App-Key` (or query params, depending on the SDK version).

**Never** use the 方舟 Ark Key (`ARK_API_KEY`) — that is for LLM inference, not ASR. They are different products in the same console.

**Never** ask the user to paste the key in chat. The key lives in the agent's secret store as `VOLCENGINE_ASR_API_KEY` (or `VOLCENGINE_ASR_APP_ID` + `VOLCENGINE_ASR_ACCESS_TOKEN`).

## Submit a transcription job

The endpoint is:

```
POST https://openspeech.bytedance.com/api/v2/asr
```

This is the **submit** endpoint. ASR is async — you submit, get a task id, poll until done.

Request body (JSON, not form-encoded):

```json
{
  "app": {
    "appid": "<VOLCENGINE_ASR_APP_ID>",
    "token": "<VOLCENGINE_ASR_ACCESS_TOKEN>",
    "cluster": "volc.seedasr.auc"
  },
  "user": {"uid": "process-podcast-from-url-<your-agent-id>"},
  "audio": {
    "format": "wav",
    "rate": 16000,
    "bits": 16,
    "channel": 1,
    "url": "<publicly fetchable URL to your audio file>"
  },
  "additions": {
    "use_itn": true,
    "use_ddc": true,
    "use_speaker_info": true,
    "diarization": true,
    "speaker_count": 2
  }
}
```

The `audio.url` must be a URL the Volcengine server can fetch. If your audio is on the local box and not publicly hosted, you have two options:

- **Option A**: Use a `POST /api/v2/asr/upload` upload endpoint (separate from the submit endpoint) to get a Volcengine-hosted URL, then submit with that URL.
- **Option B**: Use the `audio.data` field (base64-encoded bytes) — but the body gets very large for a 78-minute episode. **Not recommended.**

If your audio is local and the user has not provided a hosted URL, fall back to **recording file recognition via OSS upload**: push the file to a Volcengine OSS bucket, then submit with the OSS URL. Ask the user for an `OSS_ACCESS_KEY_ID` / `OSS_ACCESS_KEY_SECRET` if not in the env.

For the 80% case where you have a 1-hour YouTube audio in `.wav` and you just want the transcript fast, **use the upload-via-`POST /api/v2/asr/upload` route** if your local network can reach `openspeech.bytedance.com` (most do).

## ASR 上下文（`request.corpus`：背景 + 热词，专有名词纠偏）

> 策略思路源自 SubCast `asr_context.rs`（AGPL-3.0）——仅借鉴策略，规则表自研重写，未复制代码。
> 其真实案例：嘉宾「曾鸣」，无背景时 Whisper 写「曾敏」、Qwen3 写「曾明」，注入 show notes 后全部写对。

提交任务前，从 episode 元数据构造背景并放进 `request.corpus`。豆包用它做**软引导**，专有名词命中率显著提升：

```json
"request": {
  "corpus": {
    "context": "{\"context_type\":\"dialog_ctx\",\"context_data\":[{\"text\":\"<背景文本>\"}],\"hotwords\":[{\"word\":\"曾鸣\"},{\"word\":\"Anthropic\"}]}"
  }
}
```

注意 `corpus.context` 的值是**序列化后的 JSON 字符串**（不是嵌套对象）。

### 背景构造（预算 700 字符）

1. 取材顺序：`标题` + `节目名`（"谁和什么"放最前）→ 原生章节标题（≤40 个）→ 简介按行；
2. 每行清洗：去 HTML 标签 / URL / 邮箱 / emoji / `MM:SS` 时间戳 / `#话题` 的井号；HTML 实体还原；压缩空白；
3. **锅炉房行整行丢弃**（对"说了什么"零信息量）：关注 / 订阅 / 转发 / 点赞 / 三连 / 投币 / 收藏 / 商务合作 / 合作联系 / 联系方式 / 微信 / 公众号 / 小红书 / 微博 / 抖音 / 赞助 / 广告 / 优惠 / 折扣 / 下载 / APP / 扫码 / 二维码 / 加群 / 入群 / 社群 / 版权 / 转载 / 免责 / follow / subscribe / sponsor / patreon / promo code / affiliate / business inquir / copyright / all rights / link in / instagram / twitter / facebook / tiktok / discord / spotify / apple podcasts（行内命中任一即丢）；
4. 行内去重（按字母数字小写化比对）；纯符号/数字行丢弃；
5. 拼装后超 700 字符 → 按"标题节目名 → 章节 → 简介行"的顺序截断。

### 语言匹配（不匹配就不传）

- 背景文字 CJK 占比 >15% 视为中文背景，否则英文背景；
- **中文背景不配英文音频、英文背景不配中文音频**——错配会把识别往错误方向拉，此时**整体不传 `corpus`**（调用行为与从前完全一致）；音频语言不明时按标题语言。

### hotwords（≤100 个）

从背景里挑确实出现且易写错的人名 / 公司 / 产品 / 术语；长度限制：中文 2–10 字，其他 2–30 字。格式 `[{"word": "…"}]`。

### 回声过滤（转写结果后处理）

引擎在静音/音乐段可能**复读背景**。转写返回后：某行（≥8 个字母数字字符）与背景文本的字符 4-gram 重叠 ≥60% → 判为回声，删掉该行。

## Poll for the result

```
POST https://openspeech.bytedance.com/api/v2/query
```

Body:

```json
{
  "app": {"appid": "...", "token": "...", "cluster": "volc.seedasr.auc"},
  "user": {"uid": "..."},
  "task_id": "<id from submit response>"
}
```

Poll every 10 seconds. The response is one of:

```json
{"code": 0, "message": "ok", "result": {"status": "running"}}
{"code": 0, "message": "ok", "result": {"status": "done", "text": "..."}}
{"code": 0, "message": "ok", "result": {"status": "failed", "error": "..."}}
```

`status == "done"` returns the full transcript as a single text string (no sentence-level timestamps, no speaker info unless you enabled `diarization: true`).

## Convert to lumen-pod Transcript

The Volcengine response is plain text. To get a lumen-pod `Transcript` shape:

1. Split on punctuation (`.`, `?`, `!`, `。`, `？`, `!`, `!`) to recover sentence boundaries.
2. If you enabled `diarization: true` AND asked for `speaker_count: 2`, the response includes `["Speaker 0:", "Speaker 1:"]` style markers. Use those to populate `paragraphs[].speaker`. Otherwise, set `paragraphs[].speaker = null` and run speaker-mapping.md after.
3. You do **not** get word-level timestamps from this endpoint. The lumen-pod `transcript.sentences[].words[]` array has to be empty, OR you reconstruct approximate timestamps by assuming each sentence occupies its proportional share of the total audio duration (`sentences[i].start = total_duration * sum(prev_lengths) / total_length`). Mark these as approximate.

This is worse than the local SenseVoice output. **It is the reason local SenseVoice is the default.**

## Cost & quota

- 录音文件识别 2.0 (the new "大模型" endpoint) is ~¥0.0001 per second of audio. A 78-minute episode is ~¥0.47.
- The old 录音文件识别 endpoint is ~¥0.00008 per second.
- 豆包语音 实时流 (real-time streaming) is **not** what you want for podcast transcription; that is for live captions.

Check the user's billing in the Volcengine console if cost is a concern. The default for the agent is: do not check, just run.

## Error responses

| Code | Meaning | Action |
|---|---|---|
| `1001` | Invalid token | Tell the user the token is wrong. Stop. |
| `1002` | Audio URL unreachable | Re-upload or use OSS route. |
| `1003` | Audio format not supported | Re-encode to 16 kHz mono PCM WAV with `ffmpeg`. |
| `2001` | Rate limit | Wait 60 seconds, retry once. |
| `2002` | Internal server error | Retry once after 30s. If still failing, stop and tell the user. |
| `3001` | Audio too long (> 5h) | Reject — the episode is too long. Suggest the user split it. |

## Fallback ladder

If Volcengine Doubao also fails, the next options in order are:

1. **Aliyun 百炼 DashScope Paraformer-v2** — **NEVER** use. The original download-expert's user explicitly disabled it on 2026-09-02.
2. **Whisper** (large-v3 or distil-large-v3) — **NEVER** use. The user banned it. The ban is in `memory/log/2026-09.md`.
3. **OpenAI Whisper API** — **NEVER** use. Same ban.
4. **Google Speech-to-Text v2** — possible if the user has GCP credentials, but not in the default path.

If all options fail, publish the episode with `processing.status = "error"`, `processing.error = "<error message>"`, `transcript = null`, `summary = null`, `chapters = []`, `article = null`. The user gets a placeholder episode and decides whether to retry.

## What goes in `processing.asrEngine`

After a successful Volcengine run, set:

```json
{"processing": {"asrEngine": "volcengine-doubao-v2", "status": "done", ...}}
```

The Web UI uses this string to display "转写引擎: 火山引擎豆包语音 2.0" (or the language-equivalent) in the Episode header.
