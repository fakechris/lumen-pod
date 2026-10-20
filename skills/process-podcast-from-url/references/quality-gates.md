# Quality Gates — 发布前必过的 6 道检查

在生成内容之前（Step 4）执行；**任何一道失败都先修复再继续**，修不了就停（写 error sidecar / 向用户报告），绝不带病发布。

| 门 | 检查 | 不合格的处置 |
|---|---|---|
| G1 | 标题与内容匹配 | 重写标题（原标题存入 shownotes） |
| G2 | 封面非黑、非缺失 | 跳黑帧抽帧替换（`scripts/thumbnail.py`） |
| G3 | 转写时长与源时长一致 | 排查 ASR 截断，必要时重转 |
| G4 | 说话人标签已还原 | 按 speaker-mapping 规则修复 |
| G5 | 时间戳格式合规 | 按 article-golden-rules 规则修复 |
| G6 | 媒体文件完整性 | 重下/重传，确认 `complete: true` |

## G1 标题与内容匹配

平台标题经常是营销号式断章取义（"震惊！""背后真相太让人意外"）或与正片无关的节目名。用你的 LLM 判断：

1. 拿 `source.title` 和**转写全文**对照，问：一个读完转写的人，会认为这个标题概括了内容吗？
2. 不匹配的信号：标题实体在转写中不出现/只出现一次；标题断言转写未支持；纯情绪词无信息量；标题是频道名不是单集主题。
3. 处置：重写为一个具体、信息量充分的标题（≤60 字，含核心人名/主题，不用冒号堆砌）。把 `source.title` 换成新标题，**原标题以一行 `> 原标题：<...>` 前置进 `shownotes`** 保留溯源。
4. 匹配则不动。宁可平实，不可失真。

## G2 封面检查与黑帧跳过

视频类来源的封面常取自片头——而片头经常是黑屏。规则：

```bash
# 1) 平台给的缩略图是否可用（黑图/纯色/极暗都会被判 black）
python3 scripts/thumbnail.py check "<thumbnailUrl 或本地文件>"
#    -> {"black": true, "meanLuma": 0.0, "spread": 0.0}

# 2) 判黑（或平台没有缩略图）→ 从视频跳过片头黑帧抽一帧
python3 scripts/thumbnail.py extract /tmp/lumen-pod/<ID>.mp4 -o /tmp/lumen-pod/<ID>-thumb.jpg
#    -> {"path": "...", "at": 3.5, "meanLuma": 127.3}   # at = 跳过黑帧后的取帧时刻
```

- 判黑标准：平均亮度 < 16/255 且明暗跨度 < 32/255（真实画面几乎不可能同时这么暗且这么平）。
- 抽帧顺序：先用 blackdetect 跳过片头黑段；失败则按 5%→10%→15%→25%→40%→60% 时长点扫描，取第一张非黑帧。
- 抽到的帧在 **Step 7 发布后**上传：`POST /api/episodes/<id>/thumbnail`（Bearer 认证，裸字节流 jpg/png/webp ≤10MB，同音频上传风格），成功后 `source.thumbnailUrl` 自动指向 `/static/thumbs/<id>.jpg`。
- 播客纯音频（无视频）：无封面可抽是正常的，保持平台缩略图或空，不算 G2 失败。

## G3 转写完整性

- `max(sentence.end)`（转写覆盖时长）与 info.json `duration` 偏差 > 10% → ASR 被截断，重跑转写；
- 文本量 sanity：中文 ≥ 120 字/分钟、英文 ≥ 90 词/分钟，明显低于此值大概率截断或空转；
- 0 转写 = 硬失败，走 skill 的失败处理（不发布）。

## G4 说话人标签

转写里残留 `SPEAKER_00`/`Speaker A` 时，按 `references/speaker-mapping.md` 从标题/作者/简介还原真名；无法还原且只有单说话人时统一为 `Host`，多说话人无法还原才保留编号。见 speaker-mapping.md。

## G5 时间戳格式

- `< 1h`：`[MM:SS]`；`≥ 1h`：`[H:MM:SS]`；**禁止** `[0:05:14]`、`[00:05:14]` 这类前导零小时格式；
- highlights 的 `time` 必须取**句级**时间戳（sentence.words[0].start），不是段落开头；
- 金句三段式引用里的时间戳与 highlights 同源。

## G6 媒体完整性

- 音频上传：单片响应 `complete` 必须为 true；分片时最后一片的响应必须带 `audioPaths` 且长度 = total；
- `ffprobe` 时长与 info.json `duration` 偏差 < 5%；
- 上传后 `GET /api/episodes/<id>` 确认 `source.audioPaths`（或 `audioPath`）与 `thumbnailUrl` 已落库。
