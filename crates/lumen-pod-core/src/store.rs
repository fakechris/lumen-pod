use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::episode::{Episode, Invite, User, UserRole};
use crate::error::{PodError, PodResult};

pub struct Store {
    pool: PgPool,
}

impl Store {
    pub async fn new(database_url: &str) -> PodResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    async fn migrate(&self) -> PodResult<()> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("SELECT pg_advisory_lock(839201)")
            .execute(&mut *conn)
            .await?;
        sqlx::raw_sql(include_str!("schema.sql"))
            .execute(&mut *conn)
            .await?;
        sqlx::query("SELECT pg_advisory_unlock(839201)")
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    // ---- Episode CRUD ----

    pub async fn insert_episode(&self, ep: &Episode) -> PodResult<()> {
        let search_text = Self::build_search_text(ep);

        sqlx::query(
            "INSERT INTO episodes (
                id, source_url, source, processing, summary, chapters,
                mindmap, transcript, keywords, highlights, qas, clips,
                article, shownotes, is_read, tags, collection_id,
                search_vector, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16, $17,
                to_tsvector('english', $18), $19, $20
            )",
        )
        .bind(&ep.id)
        .bind(&ep.source.url)
        .bind(serde_json::to_value(&ep.source)?)
        .bind(serde_json::to_value(&ep.processing)?)
        .bind(
            ep.summary
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
        )
        .bind(serde_json::to_value(&ep.chapters)?)
        .bind(
            ep.mindmap
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
        )
        .bind(
            ep.transcript
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
        )
        .bind(serde_json::to_value(&ep.keywords)?)
        .bind(serde_json::to_value(&ep.highlights)?)
        .bind(serde_json::to_value(&ep.qas)?)
        .bind(serde_json::to_value(&ep.clips)?)
        .bind(
            ep.article
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
        )
        .bind(&ep.shownotes)
        .bind(ep.is_read)
        .bind(serde_json::to_value(&ep.tags)?)
        .bind(&ep.collection_id)
        .bind(&search_text)
        .bind(ep.created_at)
        .bind(ep.updated_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_episode(&self, id: &str) -> PodResult<Episode> {
        let row = sqlx::query(
            "SELECT source, processing, summary, chapters, mindmap, transcript,
                    keywords, highlights, qas, clips, article, shownotes,
                    is_read, tags, collection_id, created_at, updated_at
             FROM episodes WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| PodError::NotFound(id.to_string()))?;

        Self::row_to_episode(id, &row)
    }

    pub async fn list_episodes(&self, limit: usize, offset: usize) -> PodResult<Vec<Episode>> {
        let rows = sqlx::query(
            "SELECT id, source, processing, summary, chapters, mindmap, transcript,
                    keywords, highlights, qas, clips, article, shownotes,
                    is_read, tags, collection_id, created_at, updated_at
             FROM episodes ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.iter()
            .map(|row| {
                let id: &str = row.get("id");
                Self::row_to_episode(id, row)
            })
            .collect()
    }

    pub async fn update_episode(
        &self,
        id: &str,
        updates: &serde_json::Value,
    ) -> PodResult<Episode> {
        let mut ep = self.get_episode(id).await?;

        if let Some(v) = updates.get("is_read") {
            ep.is_read = v.as_bool().unwrap_or(false);
        }
        if let Some(v) = updates.get("tags") {
            if let Ok(tags) = serde_json::from_value(v.clone()) {
                ep.tags = tags;
            }
        }
        if let Some(v) = updates.get("collection_id") {
            ep.collection_id = v.as_str().map(String::from);
        }
        if let Some(v) = updates.get("summary") {
            if let Ok(s) = serde_json::from_value(v.clone()) {
                ep.summary = Some(s);
            }
        }
        if let Some(v) = updates.get("keywords") {
            if let Ok(k) = serde_json::from_value(v.clone()) {
                ep.keywords = k;
            }
        }
        if let Some(v) = updates.get("qas") {
            if let Ok(q) = serde_json::from_value(v.clone()) {
                ep.qas = q;
            }
        }
        if let Some(v) = updates.get("clips") {
            if let Ok(c) = serde_json::from_value(v.clone()) {
                ep.clips = c;
            }
        }
        if let Some(v) = updates.get("highlights") {
            if let Ok(h) = serde_json::from_value(v.clone()) {
                ep.highlights = h;
            }
        }
        if let Some(v) = updates.get("article") {
            if let Ok(a) = serde_json::from_value(v.clone()) {
                ep.article = Some(a);
            }
        }
        if let Some(v) = updates.get("shownotes") {
            ep.shownotes = v.as_str().map(String::from);
        }
        if let Some(v) = updates.get("mindmap") {
            if let Ok(m) = serde_json::from_value(v.clone()) {
                ep.mindmap = Some(m);
            }
        }
        if let Some(v) = updates.get("chapters") {
            if let Ok(c) = serde_json::from_value(v.clone()) {
                ep.chapters = c;
            }
        }

        ep.updated_at = Utc::now();
        let search_text = Self::build_search_text(&ep);

        sqlx::query(
            "UPDATE episodes SET
                summary = $1, chapters = $2, mindmap = $3,
                keywords = $4, highlights = $5, qas = $6,
                clips = $7, article = $8, shownotes = $9,
                is_read = $10, tags = $11, collection_id = $12,
                search_vector = to_tsvector('english', $13),
                updated_at = $14
             WHERE id = $15",
        )
        .bind(
            ep.summary
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
        )
        .bind(serde_json::to_value(&ep.chapters)?)
        .bind(
            ep.mindmap
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
        )
        .bind(serde_json::to_value(&ep.keywords)?)
        .bind(serde_json::to_value(&ep.highlights)?)
        .bind(serde_json::to_value(&ep.qas)?)
        .bind(serde_json::to_value(&ep.clips)?)
        .bind(
            ep.article
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
        )
        .bind(&ep.shownotes)
        .bind(ep.is_read)
        .bind(serde_json::to_value(&ep.tags)?)
        .bind(&ep.collection_id)
        .bind(&search_text)
        .bind(ep.updated_at)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(ep)
    }

    pub async fn delete_episode(&self, id: &str) -> PodResult<bool> {
        let result = sqlx::query("DELETE FROM episodes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn search_episodes(&self, query: &str, limit: usize) -> PodResult<Vec<String>> {
        let rows = sqlx::query(
            "SELECT id FROM episodes
             WHERE search_vector @@ websearch_to_tsquery('english', $1)
             ORDER BY ts_rank(search_vector, websearch_to_tsquery('english', $1)) DESC
             LIMIT $2",
        )
        .bind(query)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|r| r.get::<String, _>("id")).collect())
    }

    pub async fn episode_count(&self) -> PodResult<usize> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM episodes")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("cnt") as usize)
    }

    pub async fn episode_exists_by_url(&self, url: &str) -> PodResult<bool> {
        let row =
            sqlx::query("SELECT EXISTS(SELECT 1 FROM episodes WHERE source_url = $1) as exists")
                .bind(url)
                .fetch_one(&self.pool)
                .await?;
        Ok(row.get::<bool, _>("exists"))
    }

    // ---- User / Auth ----

    pub async fn upsert_user(&self, user: &User) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO users (id, email, name, avatar_url, role, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(email) DO UPDATE SET
               name = EXCLUDED.name,
               avatar_url = EXCLUDED.avatar_url",
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.name)
        .bind(&user.avatar_url)
        .bind(match user.role {
            UserRole::Admin => "admin",
            UserRole::Member => "member",
        })
        .bind(user.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_user_google(
        &self,
        email: &str,
        name: Option<&str>,
        avatar_url: Option<&str>,
        google_sub: &str,
    ) -> PodResult<User> {
        let row = sqlx::query(
            "INSERT INTO users (id, email, name, avatar_url, role, google_sub, created_at)
             VALUES ($1, $2, $3, $4, 'member', $5, NOW())
             ON CONFLICT(email) DO UPDATE SET
               name = COALESCE(EXCLUDED.name, users.name),
               avatar_url = COALESCE(EXCLUDED.avatar_url, users.avatar_url),
               google_sub = EXCLUDED.google_sub
             RETURNING id, email, name, avatar_url, role, created_at",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(email)
        .bind(name)
        .bind(avatar_url)
        .bind(google_sub)
        .fetch_one(&self.pool)
        .await?;

        Ok(User {
            id: row.get("id"),
            email: row.get("email"),
            name: row.get("name"),
            avatar_url: row.get("avatar_url"),
            role: match row.get::<String, _>("role").as_str() {
                "admin" => UserRole::Admin,
                _ => UserRole::Member,
            },
            created_at: row.get("created_at"),
        })
    }

    pub async fn get_user_by_email(&self, email: &str) -> PodResult<Option<User>> {
        let row = sqlx::query(
            "SELECT id, email, name, avatar_url, role, created_at FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| User {
            id: r.get("id"),
            email: r.get("email"),
            name: r.get("name"),
            avatar_url: r.get("avatar_url"),
            role: match r.get::<String, _>("role").as_str() {
                "admin" => UserRole::Admin,
                _ => UserRole::Member,
            },
            created_at: r.get("created_at"),
        }))
    }

    pub async fn is_email_invited(&self, email: &str) -> PodResult<bool> {
        let row = sqlx::query(
            "SELECT EXISTS(
                SELECT 1 FROM invites WHERE email = $1
                UNION
                SELECT 1 FROM users WHERE email = $1
             ) as exists",
        )
        .bind(email)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<bool, _>("exists"))
    }

    pub async fn create_invite(&self, invite: &Invite) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO invites (id, email, invited_by, created_at, accepted_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&invite.id)
        .bind(&invite.email)
        .bind(&invite.invited_by)
        .bind(invite.created_at)
        .bind(invite.accepted_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_invites(&self) -> PodResult<Vec<Invite>> {
        let rows = sqlx::query(
            "SELECT id, email, invited_by, created_at, accepted_at
             FROM invites ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| Invite {
                id: r.get("id"),
                email: r.get("email"),
                invited_by: r.get("invited_by"),
                created_at: r.get("created_at"),
                accepted_at: r.get("accepted_at"),
            })
            .collect())
    }

    // ---- Chat Messages ----

    pub async fn add_chat_message(
        &self,
        episode_id: &str,
        role: &str,
        content: &str,
    ) -> PodResult<crate::episode::ChatMessage> {
        let id = uuid::Uuid::new_v4().to_string();
        let row = sqlx::query(
            "INSERT INTO chat_messages (id, episode_id, role, content, created_at)
             VALUES ($1, $2, $3, $4, NOW())
             RETURNING id, episode_id, role, content, created_at",
        )
        .bind(&id)
        .bind(episode_id)
        .bind(role)
        .bind(content)
        .fetch_one(&self.pool)
        .await?;

        Ok(crate::episode::ChatMessage {
            id: row.get("id"),
            episode_id: row.get("episode_id"),
            role: match row.get::<String, _>("role").as_str() {
                "assistant" => crate::episode::ChatRole::Assistant,
                _ => crate::episode::ChatRole::User,
            },
            content: row.get("content"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn list_chat_messages(
        &self,
        episode_id: &str,
    ) -> PodResult<Vec<crate::episode::ChatMessage>> {
        let rows = sqlx::query(
            "SELECT id, episode_id, role, content, created_at
             FROM chat_messages WHERE episode_id = $1
             ORDER BY created_at ASC",
        )
        .bind(episode_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| crate::episode::ChatMessage {
                id: r.get("id"),
                episode_id: r.get("episode_id"),
                role: match r.get::<String, _>("role").as_str() {
                    "assistant" => crate::episode::ChatRole::Assistant,
                    _ => crate::episode::ChatRole::User,
                },
                content: r.get("content"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    // ---- Collections ----

    pub async fn create_collection(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> PodResult<crate::episode::Collection> {
        let id = uuid::Uuid::new_v4().to_string();
        let row = sqlx::query(
            "INSERT INTO collections (id, name, description, created_at, updated_at)
             VALUES ($1, $2, $3, NOW(), NOW())
             RETURNING id, name, description, cover_url, created_at, updated_at",
        )
        .bind(&id)
        .bind(name)
        .bind(description)
        .fetch_one(&self.pool)
        .await?;

        Ok(crate::episode::Collection {
            id: row.get("id"),
            name: row.get("name"),
            description: row.get("description"),
            cover_url: row.get("cover_url"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn list_collections(&self) -> PodResult<Vec<crate::episode::Collection>> {
        let rows = sqlx::query(
            "SELECT id, name, description, cover_url, created_at, updated_at
             FROM collections ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| crate::episode::Collection {
                id: r.get("id"),
                name: r.get("name"),
                description: r.get("description"),
                cover_url: r.get("cover_url"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    // ---- Helpers ----

    fn build_search_text(ep: &Episode) -> String {
        let mut parts = vec![ep.source.title.clone(), ep.source.author.clone()];

        if let Some(summary) = &ep.summary {
            parts.push(summary.one_liner.clone());
            parts.push(summary.r#abstract.clone());
            parts.extend(summary.key_takeaways.iter().cloned());
        }

        for kw in &ep.keywords {
            parts.push(kw.key.clone());
            if let Some(desc) = &kw.desc {
                parts.push(desc.clone());
            }
        }

        if let Some(transcript) = &ep.transcript {
            for para in &transcript.paragraphs {
                for sent in &para.sentences {
                    parts.push(sent.text.clone());
                }
            }
        }

        parts.join(" ")
    }

    fn row_to_episode(id: &str, row: &sqlx::postgres::PgRow) -> PodResult<Episode> {
        Ok(Episode {
            id: id.to_string(),
            source: serde_json::from_value(row.get::<serde_json::Value, _>("source"))?,
            processing: serde_json::from_value(row.get::<serde_json::Value, _>("processing"))?,
            summary: row
                .get::<Option<serde_json::Value>, _>("summary")
                .and_then(|v| serde_json::from_value(v).ok()),
            chapters: row
                .get::<Option<serde_json::Value>, _>("chapters")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            mindmap: row
                .get::<Option<serde_json::Value>, _>("mindmap")
                .and_then(|v| serde_json::from_value(v).ok()),
            transcript: row
                .get::<Option<serde_json::Value>, _>("transcript")
                .and_then(|v| serde_json::from_value(v).ok()),
            keywords: row
                .get::<Option<serde_json::Value>, _>("keywords")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            highlights: row
                .get::<Option<serde_json::Value>, _>("highlights")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            qas: row
                .get::<Option<serde_json::Value>, _>("qas")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            clips: row
                .get::<Option<serde_json::Value>, _>("clips")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            article: row
                .get::<Option<serde_json::Value>, _>("article")
                .and_then(|v| serde_json::from_value(v).ok()),
            shownotes: row.get("shownotes"),
            is_read: row.get("is_read"),
            tags: row
                .get::<Option<serde_json::Value>, _>("tags")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            collection_id: row.get("collection_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::*;

    async fn test_store() -> Store {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://lumenpod:lumenpod@127.0.0.1:5488/lumenpod".into());
        Store::new(&url).await.unwrap()
    }

    fn sample_episode() -> Episode {
        Episode {
            id: uuid::Uuid::new_v4().to_string(),
            source: Source {
                url: format!(
                    "https://www.youtube.com/watch?v={}",
                    uuid::Uuid::new_v4()
                ),
                platform: "youtube".into(),
                title: "Test Podcast Episode".into(),
                author: "Test Author".into(),
                published_at: Some(Utc::now()),
                thumbnail_url: Some("https://img.youtube.com/vi/test123/0.jpg".into()),
                duration: 3600.0,
                feed_url: None,
                description: Some("Original show notes".into()),
                podcast_name: Some("Test Podcast".into()),
                episode_cover_url: None,
                language: Some("en".into()),
                audio_path: None,
            },
            processing: Processing {
                status: ProcessingStatus::Done,
                asr_engine: Some("qwen3-asr".into()),
                llm_model: Some("claude-4-opus".into()),
                processed_at: Some(Utc::now()),
                agent_id: Some("claude".into()),
                error: None,
            },
            summary: Some(Summary {
                one_liner: "A great podcast about AI".into(),
                r#abstract: "This episode covers recent advances in AI...".into(),
                key_takeaways: vec![
                    "AI is changing fast".into(),
                    "Local models are improving".into(),
                ],
                thinking_questions: vec!["What if local models surpass cloud?".into()],
                glossary: vec![GlossaryTerm {
                    term: "LLM".into(),
                    definition: "Large Language Model".into(),
                }],
            }),
            chapters: vec![Chapter {
                title: "Introduction".into(),
                start_time: 0.0,
                end_time: 120.0,
                summary: "The host introduces the topic".into(),
                has_ads: false,
            }],
            mindmap: Some(Mindmap {
                markdown: "# AI\n## Local Models\n## Cloud".into(),
                opml: None,
            }),
            transcript: Some(Transcript {
                paragraphs: vec![Paragraph {
                    id: 1,
                    speaker: Some("Host".into()),
                    sentences: vec![Sentence {
                        id: "s1".into(),
                        text: "Welcome to the show".into(),
                        words: vec![
                            Word {
                                id: "w0".into(),
                                text: "Welcome".into(),
                                start: 0.0,
                                end: 0.5,
                            },
                            Word {
                                id: "w1".into(),
                                text: "to".into(),
                                start: 0.5,
                                end: 0.6,
                            },
                            Word {
                                id: "w2".into(),
                                text: "the".into(),
                                start: 0.6,
                                end: 0.7,
                            },
                            Word {
                                id: "w3".into(),
                                text: "show".into(),
                                start: 0.7,
                                end: 1.0,
                            },
                        ],
                    }],
                }],
                translations: Default::default(),
            }),
            keywords: vec![
                Keyword {
                    key: "AI".into(),
                    desc: Some("Artificial Intelligence".into()),
                },
                Keyword {
                    key: "podcast".into(),
                    desc: None,
                },
            ],
            highlights: vec![Highlight {
                time: 12.5,
                content: "Key insight about AI models".into(),
                cue_id: Some("s1".into()),
                note: None,
                created_at: Utc::now(),
            }],
            qas: vec![QA {
                question: "What is the main thesis?".into(),
                answer: "AI models are becoming local-first".into(),
                question_speaker: Some("Host".into()),
                answer_speaker: Some("Guest".into()),
            }],
            clips: vec![],
            article: None,
            shownotes: Some("Original show notes from the podcast feed".into()),
            is_read: false,
            tags: vec!["ai".into(), "tech".into()],
            collection_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn roundtrip_insert_get() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();
        let loaded = store.get_episode(&ep.id).await.unwrap();
        assert_eq!(loaded.id, ep.id);
        assert_eq!(loaded.source.title, "Test Podcast Episode");
        assert_eq!(
            loaded.summary.unwrap().one_liner,
            "A great podcast about AI"
        );
        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn list_and_count() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();
        let count = store.episode_count().await.unwrap();
        assert!(count >= 1);
        let list = store.list_episodes(10, 0).await.unwrap();
        assert!(!list.is_empty());
        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn search_fts() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();
        let results = store.search_episodes("podcast", 10).await.unwrap();
        assert!(results.contains(&ep.id));
        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn delete_removes_episode() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();
        assert!(store.delete_episode(&ep.id).await.unwrap());
        assert!(store.get_episode(&ep.id).await.is_err());
    }

    #[tokio::test]
    async fn duplicate_url_check() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();
        assert!(store.episode_exists_by_url(&ep.source.url).await.unwrap());
        assert!(!store
            .episode_exists_by_url("https://nonexistent.url")
            .await
            .unwrap());
        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn user_invite_flow() {
        let store = test_store().await;
        let email = format!("test-{}@example.com", uuid::Uuid::new_v4());
        assert!(!store.is_email_invited(&email).await.unwrap());

        let invite = Invite {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.clone(),
            invited_by: "admin-id".into(),
            created_at: Utc::now(),
            accepted_at: None,
        };
        store.create_invite(&invite).await.unwrap();
        assert!(store.is_email_invited(&email).await.unwrap());

        let user = User {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.clone(),
            name: Some("Test User".into()),
            avatar_url: None,
            role: UserRole::Member,
            created_at: Utc::now(),
        };
        store.upsert_user(&user).await.unwrap();
        let loaded = store.get_user_by_email(&email).await.unwrap().unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Test User"));

        // Cleanup
        sqlx::query("DELETE FROM users WHERE email = $1")
            .bind(&email)
            .execute(store.pool())
            .await
            .unwrap();
        sqlx::query("DELETE FROM invites WHERE email = $1")
            .bind(&email)
            .execute(store.pool())
            .await
            .unwrap();
    }
}
