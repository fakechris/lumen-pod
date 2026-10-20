# Download Playbook — yt-dlp 加固与平台降级链

研究自乔木剪藏（qiaomu-clipper）的下载实现 + lumen-pod 实战经验。目标：在无头环境把一条 URL 可靠地变成音频/视频文件，并明确知道**为什么失败**。

## 1. yt-dlp 加固基线（所有平台通用）

```bash
yt-dlp \
  --ignore-config \          # 用户/系统的 yt-dlp.conf 会有意外行为（乔木特意加的）
  --no-playlist \            # URL 带列表参数时只取单集
  --no-warnings --newline \
  --abort-on-unavailable-fragments \
  --socket-timeout 30 --retries 3 --fragment-retries 3 \
  --match-filter '!is_live & !has_drm' \   # 直播和 DRM 版权视频明确不可下载，快速失败
  --ffmpeg-location "$(command -v ffmpeg)" \
  -f '<格式链>' \
  -o '/tmp/lumen-pod/%(id)s.%(ext)s' \
  -- '<URL>'
```

要点（来自 qiaomu `media_download.py`）：
- **格式链优先 H.264+AAC**，保证任何播放器可放、后续 ffmpeg 抽帧不踩编解码坑：
  `bv*[vcodec^=avc1]+ba[acodec^=mp4a]/bv*[ext=mp4]+ba[ext=m4a]/bv*+ba/b`
  （只要音频时用 `ba/b`；合并产物落不了 mp4 时允许 `--merge-output-format mp4/mkv`）
- `--match-filter` 拒直播/DRM：错误信息会含 `does not pass filter`，直接向用户报"直播和受版权保护的视频不能下载"，**不要重试**。
- `ERROR: requested format is not available` → 该视频被平台额外限制（常见于 YouTube），转写/学习不受影响，报告后停止。

## 2. 平台降级链

### B 站（bilibili.com / b23.tv）
yt-dlp 对 B 站经常被 412（风控）。按序尝试：
1. `yt-dlp <加固基线>` 裸试（未登录有时可行，尤其是低清晰度）；
2. 带登录态重试：`--cookies-from-browser safari`（或 chrome/firefox，仅限本机有浏览器时）或 `--cookies /path/to/cookies.txt`（Netscape 格式，可从浏览器插件导出）；
3. `bili audio "<BV>" -o /tmp/lumen-pod/<BV>.wav`（仓库既有工具，自带登录态）；
4. 全部失败：写 `<ID>.error.json`（含每步的 stderr 尾行），停止。不要发布无转写的 episode。

### YouTube
- 匿名优先：YouTube 对登录会话只发需要 token 的 web 播放器，**匿名播放器反而常给直链**（乔木经验）。所以先裸试，遇到 "Sign in to confirm you're not a bot" 再带 cookies 重试。
- 字幕直连：`--write-subs --write-auto-subs --sub-langs "en.*,zh.*,ja.*" --sub-format "json3/vtt/srt" --skip-download` 命中字幕时跳过音频下载与 ASR（最快路径）。

### 小宇宙 / 播客 RSS
- 页面或 RSS 里的 `mediaUrl`（如 `media.xyzcdn.net/*.m4a`）是直链，直接 `curl -L` 或 yt-dlp 均可，无需登录。

### 抖音 / 视频号 / 小红书 / 小鹅通（仅当有浏览器环境时）
这类平台的 CDN 直链带签名和 Referer 校验，无头环境不要硬啃；有浏览器时可参考乔木的思路——从播放器 DOM 的 `video.currentSrc` 或 Performance Resource Timing 里捞已签名直链，交给 helper 用 `Referer: <页面URL> + 浏览器 UA` 抓取。lumen-pod 的 agent 场景默认不做，遇到就明确报告"该平台需要浏览器环境"。

## 3. 下载后的完整性检查

- 文件非 0 字节；`ffprobe -v error -show_entries format=duration -of csv=p=0 <file>` 输出的时长与 info.json 的 `duration` 偏差 < 5%；
- 音频超过 200MB：先 `ffmpeg -i in -ac 1 -ar 16000 -y out.wav` 降采样再喂 ASR（只影响转写输入，发布上传仍用原质量 mp3）；
- **保留 info.json**——`title/ uploader/ duration/ thumbnail/ chapters[]` 后面所有步骤都要用。

## 4. 错误分诊速查

| 信号 | 含义 | 动作 |
|---|---|---|
| `does not pass filter` | 直播 / DRM | 报告，停止 |
| HTTP 412（B站） | 风控 | 降级链 2→3 |
| `Sign in to confirm` | 需要登录 | 带 cookies 重试一次 |
| `requested format is not available` | 平台限制该视频 | 报告，停止 |
| `video unavailable` / `private` | 视频不存在或私享 | 报告，停止 |
| 下载文件 0 字节 / duration 缺失 | 下载损坏 | 重试一次，再失败写 error sidecar |
