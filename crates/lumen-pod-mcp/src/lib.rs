//! MCP server (stdio JSON-RPC) — expose lumen-pod as tools for agents.
//!
//! Agents (Claude, Grokbot, etc.) launch `lumen-pod mcp` and talk JSON-RPC
//! over stdio. Tools: pod_publish, pod_list, pod_get, pod_search, pod_delete,
//! pod_exists, pod_update, pod_upload_audio_guide.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::AsyncBufReadExt;

use lumen_pod_core::store::Store;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run_stdio(database_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(Store::new(database_url).await?);
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let resp = handle(&store, req).await;
        if let Ok(resp) = resp {
            if resp != Value::Null {
                println!("{}", resp);
            }
        }
    }
    Ok(())
}

async fn handle(store: &Store, req: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let ok = |result: Value| json!({"jsonrpc":"2.0","id":id,"result":result});

    match method {
        "initialize" => Ok(ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "lumen-pod", "version": VERSION}
        }))),
        "notifications/initialized" => Ok(Value::Null),
        "tools/list" => Ok(ok(json!({"tools": tools()}))),
        "tools/call" => {
            let name = req
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or(json!({}));
            match call_tool(store, name, &args).await {
                Ok(text) => Ok(ok(json!({"content":[{"type":"text","text":text}]}))),
                Err(e) => Ok(ok(json!({
                    "isError": true,
                    "content":[{"type":"text","text":format!("{e:#}")}]
                }))),
            }
        }
        _ => Ok(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"unknown method"}}),
        ),
    }
}

fn tools() -> Vec<Value> {
    vec![
        tool(
            "pod_publish",
            "Publish a fully processed episode to Lumen Pod. The episode JSON must include:\n\
             - source: {url, platform, title, author, duration, description, podcastName, language, thumbnailUrl}\n\
             - processing: {status: \"done\", asrEngine, llmModel, agentId}\n\
             - summary: {oneLiner, abstract, keyTakeaways[], thinkingQuestions[], glossary[{term,definition}]}\n\
             - article: {title, content, format: \"markdown\", lang, focus} (YouMind v54 style standalone 'Read-and-Done' long-form article with scene-first hook, reasoning chain, primary cases, pinned terms, tri-part quotes with [MM:SS], and mentioned books/tools)\n\
             - transcript: {paragraphs[{id, speaker, sentences[{id, text, words[{id, text, start, end}]}]}], translations: {}}\n\
             - chapters: [{title, startTime, endTime, summary, hasAds, deepRead}]\n\
             - mindmap: {markdown} (markdown formatted mind map for Markmap rendering)\n\
             - keywords: [{key, desc}]\n\
             - highlights: [{time, content, cueId, note}]\n\
             - qas: [{question, answer, questionSpeaker, answerSpeaker}]\n\
             - tags: string[]\n\n\
             After publishing, you can upload the audio file using the REST API (see pod_upload_audio_guide).\n\
             All field names use camelCase. The episode ID is auto-generated if not provided.",
            &["episode_json"],
        ),
        tool(
            "pod_list",
            "List episodes with pagination. Returns title, author, status, and id.",
            &[],
        ),
        tool(
            "pod_get",
            "Get a single episode by id with full transcript and summary.",
            &["id"],
        ),
        tool(
            "pod_search",
            "Full-text search across episode transcripts, titles, and summaries.",
            &["query"],
        ),
        tool(
            "pod_delete",
            "Delete an episode by id.",
            &["id"],
        ),
        tool(
            "pod_exists",
            "Check if an episode with the given source URL already exists.",
            &["url"],
        ),
        tool(
            "pod_update",
            "Update an existing episode's fields. Pass the episode id and a JSON string with only the fields to update.\n\n\
             Supported top-level fields: source, processing, transcript, summary, chapters, mindmap, keywords, highlights, qas, clips, article, shownotes, isRead, tags, collectionId.\n\n\
             To set the audio path after uploading via REST API:\n\
             {\"source\": {\"audioPath\": \"/audio/<episode_id>.mp3\", ...keep other source fields}}\n\n\
             Both camelCase and snake_case field names are accepted.",
            &["id", "updates_json"],
        ),
        tool(
            "pod_upload_audio_guide",
            "Returns instructions for uploading an MP3 audio file to an episode via the REST API.\n\n\
             This tool does NOT upload the file itself — it returns the curl command and API details \
             that the agent should execute to upload the audio. The agent typically has the MP3 file \
             locally after downloading/extracting audio from the source video/podcast.\n\n\
             After upload, the episode's source.audioPath is automatically updated and the \
             self-hosted audio player will be used instead of YouTube/Bilibili embed.",
            &["episode_id"],
        ),
    ]
}

fn tool(name: &str, desc: &str, required: &[&str]) -> Value {
    let props: Value = required
        .iter()
        .map(|p| (p.to_string(), json!({"type":"string"})))
        .collect();
    json!({
        "name": name,
        "description": desc,
        "inputSchema": {
            "type": "object",
            "properties": props,
            "required": required,
        }
    })
}

async fn call_tool(
    store: &Store,
    name: &str,
    args: &Value,
) -> Result<String, Box<dyn std::error::Error>> {
    match name {
        "pod_publish" => {
            let episode_json = args
                .get("episode_json")
                .and_then(|v| v.as_str())
                .ok_or("missing episode_json")?;
            let mut episode: lumen_pod_core::episode::Episode =
                serde_json::from_str(episode_json)?;

            if episode.id.is_empty() {
                episode.id = uuid::Uuid::new_v4().to_string();
            }
            if store.episode_exists_by_url(&episode.source.url).await? {
                return Ok(json!({"status": "duplicate", "url": episode.source.url}).to_string());
            }

            let now = chrono::Utc::now();
            episode.created_at = now;
            episode.updated_at = now;
            store.insert_episode(&episode).await?;

            Ok(json!({
                "status": "ok",
                "id": episode.id,
                "message": "Episode published. To add audio, use pod_upload_audio_guide to get the upload instructions."
            })
            .to_string())
        }
        "pod_list" => {
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as usize;
            let offset = args
                .get("offset")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let episodes = store.list_episodes(limit, offset).await?;
            let total = store.episode_count().await?;
            let items: Vec<Value> = episodes
                .iter()
                .map(|ep| {
                    json!({
                        "id": ep.id,
                        "title": ep.source.title,
                        "author": ep.source.author,
                        "platform": ep.source.platform,
                        "status": ep.processing.status,
                        "duration": ep.source.duration,
                        "hasAudio": ep.source.audio_path.is_some(),
                        "createdAt": ep.created_at,
                    })
                })
                .collect();
            Ok(json!({"total": total, "episodes": items}).to_string())
        }
        "pod_get" => {
            let id = args.get("id").and_then(|v| v.as_str()).ok_or("missing id")?;
            let episode = store.get_episode(id).await?;
            Ok(serde_json::to_string_pretty(&episode)?)
        }
        "pod_search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or("missing query")?;
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as usize;
            let ids = store.search_episodes(query, limit).await?;
            let mut episodes = Vec::new();
            for id in &ids {
                if let Ok(ep) = store.get_episode(id).await {
                    episodes.push(json!({
                        "id": ep.id,
                        "title": ep.source.title,
                        "author": ep.source.author,
                        "oneLiner": ep.summary.as_ref().map(|s| s.one_liner.as_str()),
                    }));
                }
            }
            Ok(json!({"results": episodes}).to_string())
        }
        "pod_delete" => {
            let id = args.get("id").and_then(|v| v.as_str()).ok_or("missing id")?;
            let deleted = store.delete_episode(id).await?;
            Ok(json!({"deleted": deleted}).to_string())
        }
        "pod_exists" => {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or("missing url")?;
            let exists = store.episode_exists_by_url(url).await?;
            Ok(json!({"exists": exists}).to_string())
        }
        "pod_update" => {
            let id = args.get("id").and_then(|v| v.as_str()).ok_or("missing id")?;
            let updates_json = args
                .get("updates_json")
                .and_then(|v| v.as_str())
                .ok_or("missing updates_json")?;
            let updates: Value = serde_json::from_str(updates_json)?;
            let episode = store.update_episode(id, &updates).await?;
            Ok(
                json!({"status": "ok", "id": episode.id, "updated_at": episode.updated_at})
                    .to_string(),
            )
        }
        "pod_upload_audio_guide" => {
            let episode_id = args
                .get("episode_id")
                .and_then(|v| v.as_str())
                .ok_or("missing episode_id")?;

            let api_base = std::env::var("CORS_ORIGINS")
                .unwrap_or_else(|_| "https://pod.lumenopen.com".into())
                .split(',')
                .next()
                .unwrap_or("https://pod.lumenopen.com")
                .trim()
                .to_string();

            let guide = format!(
                r#"## Upload Audio to Lumen Pod

To upload an MP3 file for episode `{episode_id}`, send a POST request with the raw audio bytes:

### Using curl:
```bash
curl -X POST "{api_base}/api/episodes/{episode_id}/audio" \
  -H "Authorization: Bearer $LUMEN_POD_API_TOKEN" \
  -H "Content-Type: application/octet-stream" \
  --data-binary @/path/to/audio.mp3
```

### Using Python:
```python
import requests

with open("audio.mp3", "rb") as f:
    resp = requests.post(
        "{api_base}/api/episodes/{episode_id}/audio",
        headers={{
            "Authorization": "Bearer " + os.environ["LUMEN_POD_API_TOKEN"],
            "Content-Type": "application/octet-stream",
        }},
        data=f.read(),
    )
print(resp.json())
```

### Using fetch (Node.js):
```javascript
const fs = require('fs');
const audio = fs.readFileSync('audio.mp3');
const resp = await fetch("{api_base}/api/episodes/{episode_id}/audio", {{
  method: "POST",
  headers: {{
    "Authorization": `Bearer ${{process.env.LUMEN_POD_API_TOKEN}}`,
    "Content-Type": "application/octet-stream",
  }},
  body: audio,
}});
console.log(await resp.json());
```

### Environment variable:
The API token is set as `LUMEN_POD_API_TOKEN`. Ask the user for the token if you don't have it.

### Response:
```json
{{"status": "ok", "audioPath": "/audio/{episode_id}.mp3", "sizeBytes": 12345678}}
```

After upload, the episode's `source.audioPath` is automatically updated. The UI will show a self-hosted audio player instead of YouTube/Bilibili embed, which avoids platform embed restrictions.

### Typical agent workflow:
1. Download audio using yt-dlp: `yt-dlp -x --audio-format mp3 "<source_url>" -o audio.mp3`
2. Transcribe with ASR (e.g., Whisper, Qwen3-ASR)
3. Generate summary/chapters/etc with LLM
4. Publish episode using `pod_publish`
5. Upload audio using the curl command above"#
            );

            Ok(guide)
        }
        _ => Err(format!("unknown MCP tool: {name}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_store() -> Store {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://lumenpod:lumenpod@127.0.0.1:5488/lumenpod".into());
        Store::new(&url).await.unwrap()
    }

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let store = test_store().await;
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize"});
        let resp = handle(&store, req).await.unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "lumen-pod");
    }

    #[tokio::test]
    async fn tools_list_advertises_tools() {
        let store = test_store().await;
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
        let resp = handle(&store, req).await.unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 8);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"pod_publish"));
        assert!(names.contains(&"pod_search"));
        assert!(names.contains(&"pod_update"));
        assert!(names.contains(&"pod_upload_audio_guide"));
    }
}
