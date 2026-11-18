# Episode 多来源支持 + 嵌入渲染偏好（X 优先）

## Context

同一内容常在 X 和 YouTube 各发一份（如 PG × Amjad Masad《How To Build The Future》）：X 版可嵌入播放器但元数据贫瘠（304 字简介、无章节、无字幕），YouTube 版元数据完整（正式标题、1911 字简介、18 段原生章节、json3 英文字幕）。当前 `Episode.source` 是单来源对象，`source_url` 列是唯一去重键，前端只从这一个 URL 提取嵌入 ID——无法登记第二来源，合并只能发布前手动拼接 JSON。

目标：episode 支持登记多个来源（`altSources`）+ 嵌入渲染偏好（`embedPreference`，内置 X 优先），**运行时零 merge 逻辑**——agent 处理时把最全元数据写进主 source，其余来源进登记表。已确认：默认偏好顺序 twitter > youtube > bilibili；范围 = 核心数据模型/去重/前端渲染 + skill 文档 + 详情页来源展示，不做手动编辑表单。

## 设计

- **主 `source`** = 元数据展示来源 + 去重键（PG 场景 = YouTube，标题/简介/章节/发布时间/字幕快车道全来自它）。
- **`altSources: Vec<AltSource>`** = 来源登记表，每项 `{ url, platform, title?, author?, publishedAt?, thumbnailUrl?, duration?, description? }`（可选字段全部 `#[serde(default)]`；PG 场景登记 X 链接 + 其自身元数据快照）。
- **`embedPreference: Option<String>`**（platform 名，如 `"twitter"`）：显式指定嵌入偏好；未设置时内置顺序 `twitter > youtube > bilibili`。
- 转写/章节/音频仍是单时间轴（YouTube captions 2655s ≈ X 2654.5s，容差内），不变。

## 改动

### 1. Rust 数据模型 — `crates/lumen-pod-core/src/episode.rs`
- 新增 `AltSource` struct（url/platform 必填，其余 `#[serde(default)]`，camelCase）。
- `Episode` 加 `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub alt_sources: Vec<AltSource>` 和 `#[serde(default, skip_serializing_if = "Option::is_none")] pub embed_preference: Option<String>`（照 `collection_id` 的写法，episode.rs:47-48）。

### 2. Schema — `crates/lumen-pod-core/src/schema.sql`（追加幂等迁移，文件末尾风格同 102-120 行）
```sql
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS alt_sources JSONB NOT NULL DEFAULT '[]'::jsonb;
CREATE INDEX IF NOT EXISTS idx_episodes_alt_sources ON episodes USING GIN (alt_sources jsonb_path_ops);
```

### 3. Store — `crates/lumen-pod-core/src/store.rs`
- `insert_episode`（96-153）、`get_episode`（159+）、`row_to_episode`、`row_to_episode_card` 的列清单加 `alt_sources`。
- `update_episode`（705-873）：`altSources`/`alt_sources`、`embedPreference`/`embed_preference` 键走整体替换（同 739-743 source 的模式）；UPDATE 写回 `alt_sources` 列；**同步重写 `source_url` 列 = 新 `source.url`**（否则主来源切换后去重键 stale）。
- `episode_exists_by_url`（1042）/ `find_episode_by_url`（1051）扩展：`WHERE source_url = $1 OR alt_sources @> $2::jsonb`（`jsonb_build_array(jsonb_build_object('url', $1))`，GIN 命中）——提交 twin URL 自动 409。
- store tests（1913+ 模块）加：带 altSources 的 insert/find、PATCH altSources/embedPreference、按 altSource url 查重。

### 4. API — `crates/lumen-pod-api/src/main.rs`
- `submit_url`/`lookup_episode_by_url`（1628-1678）无需新逻辑，自动受益于 store 查重扩展；YouTube alt-form fallback 保留。tests（3812+）补 submit twin URL → 409 用例。PATCH 无需新端点（新键透传 store）。

### 5. 前端 — `web/src/lib/types.ts` + `web/src/app/episodes/[id]/EpisodeDetail.tsx`
- `types.ts`：`AltSource` 类型；`Episode` 加可选 `altSources` / `embedPreference`。
- `EpisodeDetail.tsx`：
  - 嵌入提取改为扫所有来源 URL（`[source.url, ...altSources.map(s => s.url)]`，复用 19-32 行的三个 extract 函数），得到 embed 候选列表；
  - 偏好解析：`embedPreference` 显式 > 内置 `twitter > youtube > bilibili`，选出 preferred embed；
  - 渲染（842-853、1164-1207）：preferred embed 展开渲染（不再折叠进 `<details>`），其余嵌入收进"其他来源"折叠块；audio player 保留为主时间线，`canSeek` 逻辑不变；
  - 元数据区（1001 行附近"Source ↗"）扩为来源列表：主来源 + altSources 各自平台 icon + 外链。

### 6. Skill 文档 — `skills/process-podcast-from-url/`
- `SKILL.md` 新增"多来源合并"步骤：发现同内容双平台时，yt-dlp 抓元数据更全一方（如 YouTube）写进主 source（url 用该平台），另一平台登记进 `altSources`（含自身快照），设 `embedPreference`；转写优先走 YouTube captions 快车道（`asrEngine="youtube-subs"`），跳过 SenseVoice；单来源流程不变。
- `references/transcript-schema.md` 补 `altSources` / `embedPreference` 字段说明（含"source 仍整体替换"的注意事项）。

## 验证

1. `cargo test -p lumen-pod-core -p lumen-pod-api`（store 查重/更新 + API 409 用例）。
2. 本地起 api+web：用 PG 这条真实 shape POST 一个 episode（主 source=YouTube 元数据，altSources=[X 链接 2654.5s]，embedPreference="twitter"），详情页确认 X 嵌入优先渲染、来源列表两条、音频可 seek。
3. PATCH 该 episode 的 twin URL 提交 `submit-url` → 409；`pod_update` 写 `altSources` 走通。
