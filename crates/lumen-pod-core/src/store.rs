use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::episode::{Episode, Invite, User, UserRole};
use crate::error::{PodError, PodResult};

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> PodResult<Self> {
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_memory() -> PodResult<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("store lock poisoned")
    }

    fn migrate(&self) -> PodResult<()> {
        self.conn().execute_batch(
            "
            CREATE TABLE IF NOT EXISTS episodes (
                id TEXT PRIMARY KEY,
                source_url TEXT NOT NULL,
                source_json TEXT NOT NULL,
                processing_json TEXT NOT NULL,
                summary_json TEXT,
                chapters_json TEXT NOT NULL DEFAULT '[]',
                mindmap_json TEXT,
                transcript_json TEXT,
                keywords_json TEXT NOT NULL DEFAULT '[]',
                highlights_json TEXT NOT NULL DEFAULT '[]',
                qas_json TEXT NOT NULL DEFAULT '[]',
                clips_json TEXT NOT NULL DEFAULT '[]',
                article_json TEXT,
                shownotes TEXT,
                is_read INTEGER NOT NULL DEFAULT 0,
                tags_json TEXT NOT NULL DEFAULT '[]',
                collection_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS collections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                cover_url TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS chat_messages (
                id TEXT PRIMARY KEY,
                episode_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (episode_id) REFERENCES episodes(id)
            );

            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT NOT NULL UNIQUE,
                name TEXT,
                avatar_url TEXT,
                role TEXT NOT NULL DEFAULT 'member',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS invites (
                id TEXT PRIMARY KEY,
                email TEXT NOT NULL,
                invited_by TEXT NOT NULL,
                created_at TEXT NOT NULL,
                accepted_at TEXT
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS episodes_fts USING fts5(
                episode_id UNINDEXED,
                title,
                author,
                transcript_text,
                summary_text,
                tokenize='unicode61'
            );

            CREATE INDEX IF NOT EXISTS idx_episodes_source_url ON episodes(source_url);
            CREATE INDEX IF NOT EXISTS idx_episodes_created_at ON episodes(created_at);
            CREATE INDEX IF NOT EXISTS idx_invites_email ON invites(email);
            ",
        )?;
        Ok(())
    }

    // ---- Episode CRUD ----

    pub fn insert_episode(&self, ep: &Episode) -> PodResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO episodes (id, source_url, source_json, processing_json,
             summary_json, chapters_json, mindmap_json, transcript_json,
             keywords_json, highlights_json, qas_json, clips_json,
             article_json, shownotes, is_read, tags_json, collection_id,
             created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                ep.id,
                ep.source.url,
                serde_json::to_string(&ep.source)?,
                serde_json::to_string(&ep.processing)?,
                ep.summary
                    .as_ref()
                    .map(|s| serde_json::to_string(s))
                    .transpose()?,
                serde_json::to_string(&ep.chapters)?,
                ep.mindmap
                    .as_ref()
                    .map(|m| serde_json::to_string(m))
                    .transpose()?,
                ep.transcript
                    .as_ref()
                    .map(|t| serde_json::to_string(t))
                    .transpose()?,
                serde_json::to_string(&ep.keywords)?,
                serde_json::to_string(&ep.highlights)?,
                serde_json::to_string(&ep.qas)?,
                serde_json::to_string(&ep.clips)?,
                ep.article
                    .as_ref()
                    .map(|a| serde_json::to_string(a))
                    .transpose()?,
                ep.shownotes,
                ep.is_read as i32,
                serde_json::to_string(&ep.tags)?,
                ep.collection_id,
                ep.created_at.to_rfc3339(),
                ep.updated_at.to_rfc3339(),
            ],
        )?;

        let title = &ep.source.title;
        let author = &ep.source.author;
        let transcript_text = ep
            .transcript
            .as_ref()
            .map(|t| {
                t.paragraphs
                    .iter()
                    .flat_map(|p| p.sentences.iter())
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let keywords_text = ep
            .keywords
            .iter()
            .map(|k| {
                let mut s = k.key.clone();
                if let Some(d) = &k.desc {
                    s.push(' ');
                    s.push_str(d);
                }
                s
            })
            .collect::<Vec<_>>()
            .join(" ");
        let summary_text = ep
            .summary
            .as_ref()
            .map(|s| {
                format!(
                    "{} {} {} {}",
                    s.one_liner,
                    s.r#abstract,
                    s.key_takeaways.join(" "),
                    keywords_text,
                )
            })
            .unwrap_or_default();

        conn.execute(
            "INSERT INTO episodes_fts (episode_id, title, author, transcript_text, summary_text)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![ep.id, title, author, transcript_text, summary_text],
        )?;

        Ok(())
    }

    pub fn get_episode(&self, id: &str) -> PodResult<Episode> {
        let conn = self.conn();
        let ep = conn
            .query_row(
                "SELECT source_json, processing_json, summary_json, chapters_json,
                 mindmap_json, transcript_json, keywords_json, highlights_json,
                 qas_json, clips_json, article_json, shownotes,
                 is_read, tags_json, collection_id, created_at, updated_at
                 FROM episodes WHERE id = ?1",
                params![id],
                |row| {
                    let parse_dt = |s: String| {
                        chrono::DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now())
                    };
                    Ok(Episode {
                        id: id.to_string(),
                        source: serde_json::from_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| panic!("bad source json")),
                        processing: serde_json::from_str(&row.get::<_, String>(1)?).unwrap_or_else(|_| panic!("bad processing json")),
                        summary: row.get::<_, Option<String>>(2)?.and_then(|s| serde_json::from_str(&s).ok()),
                        chapters: row.get::<_, String>(3).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        mindmap: row.get::<_, Option<String>>(4)?.and_then(|s| serde_json::from_str(&s).ok()),
                        transcript: row.get::<_, Option<String>>(5)?.and_then(|s| serde_json::from_str(&s).ok()),
                        keywords: row.get::<_, String>(6).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        highlights: row.get::<_, String>(7).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        qas: row.get::<_, String>(8).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        clips: row.get::<_, String>(9).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        article: row.get::<_, Option<String>>(10)?.and_then(|s| serde_json::from_str(&s).ok()),
                        shownotes: row.get(11)?,
                        is_read: row.get::<_, i32>(12).unwrap_or(0) != 0,
                        tags: row.get::<_, String>(13).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                        collection_id: row.get(14)?,
                        created_at: parse_dt(row.get::<_, String>(15)?),
                        updated_at: parse_dt(row.get::<_, String>(16)?),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| PodError::NotFound(id.to_string()))?;

        Ok(ep)
    }

    pub fn list_episodes(&self, limit: usize, offset: usize) -> PodResult<Vec<Episode>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id FROM episodes ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let ids: Vec<String> = stmt
            .query_map(params![limit as i64, offset as i64], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);

        ids.iter().map(|id| self.get_episode(id)).collect()
    }

    pub fn update_episode(&self, id: &str, updates: &serde_json::Value) -> PodResult<Episode> {
        let mut ep = self.get_episode(id)?;
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

        ep.updated_at = chrono::Utc::now();

        let conn = self.conn();
        conn.execute(
            "UPDATE episodes SET
             summary_json = ?1, chapters_json = ?2, mindmap_json = ?3,
             keywords_json = ?4, highlights_json = ?5, qas_json = ?6,
             clips_json = ?7, article_json = ?8, shownotes = ?9,
             is_read = ?10, tags_json = ?11, collection_id = ?12,
             updated_at = ?13
             WHERE id = ?14",
            params![
                ep.summary.as_ref().map(|s| serde_json::to_string(s).unwrap()),
                serde_json::to_string(&ep.chapters)?,
                ep.mindmap.as_ref().map(|m| serde_json::to_string(m).unwrap()),
                serde_json::to_string(&ep.keywords)?,
                serde_json::to_string(&ep.highlights)?,
                serde_json::to_string(&ep.qas)?,
                serde_json::to_string(&ep.clips)?,
                ep.article.as_ref().map(|a| serde_json::to_string(a).unwrap()),
                ep.shownotes,
                ep.is_read as i32,
                serde_json::to_string(&ep.tags)?,
                ep.collection_id,
                ep.updated_at.to_rfc3339(),
                id,
            ],
        )?;

        Ok(ep)
    }

    pub fn delete_episode(&self, id: &str) -> PodResult<bool> {
        let conn = self.conn();
        let deleted = conn.execute("DELETE FROM episodes WHERE id = ?1", params![id])?;
        if deleted > 0 {
            conn.execute("DELETE FROM episodes_fts WHERE episode_id = ?1", params![id])?;
        }
        Ok(deleted > 0)
    }

    pub fn search_episodes(&self, query: &str, limit: usize) -> PodResult<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT episode_id FROM episodes_fts WHERE episodes_fts MATCH ?1 LIMIT ?2",
        )?;
        let ids = stmt
            .query_map(params![query, limit as i64], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    pub fn episode_count(&self) -> PodResult<usize> {
        let conn = self.conn();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM episodes", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn episode_exists_by_url(&self, url: &str) -> PodResult<bool> {
        let conn = self.conn();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM episodes WHERE source_url = ?1)",
            params![url],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    // ---- User / Auth ----

    pub fn upsert_user(&self, user: &User) -> PodResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO users (id, email, name, avatar_url, role, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(email) DO UPDATE SET
               name = excluded.name,
               avatar_url = excluded.avatar_url",
            params![
                user.id,
                user.email,
                user.name,
                user.avatar_url,
                match user.role {
                    UserRole::Admin => "admin",
                    UserRole::Member => "member",
                },
                user.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_user_by_email(&self, email: &str) -> PodResult<Option<User>> {
        let conn = self.conn();
        conn.query_row(
            "SELECT id, email, name, avatar_url, role, created_at FROM users WHERE email = ?1",
            params![email],
            |row| {
                Ok(User {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    name: row.get(2)?,
                    avatar_url: row.get(3)?,
                    role: match row.get::<_, String>(4)?.as_str() {
                        "admin" => UserRole::Admin,
                        _ => UserRole::Member,
                    },
                    created_at: chrono::DateTime::parse_from_rfc3339(
                        &row.get::<_, String>(5)?,
                    )
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                })
            },
        )
        .optional()
        .map_err(PodError::from)
    }

    pub fn is_email_invited(&self, email: &str) -> PodResult<bool> {
        let conn = self.conn();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM invites WHERE email = ?1
                UNION
                SELECT 1 FROM users WHERE email = ?1
             )",
            params![email],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    pub fn create_invite(&self, invite: &Invite) -> PodResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO invites (id, email, invited_by, created_at, accepted_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                invite.id,
                invite.email,
                invite.invited_by,
                invite.created_at.to_rfc3339(),
                invite.accepted_at.map(|dt| dt.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn list_invites(&self) -> PodResult<Vec<Invite>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, email, invited_by, created_at, accepted_at FROM invites ORDER BY created_at DESC",
        )?;
        let invites = stmt
            .query_map([], |row| {
                Ok(Invite {
                    id: row.get(0)?,
                    email: row.get(1)?,
                    invited_by: row.get(2)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(
                        &row.get::<_, String>(3)?,
                    )
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                    accepted_at: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(invites)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::*;

    fn sample_episode() -> Episode {
        Episode {
            id: uuid::Uuid::new_v4().to_string(),
            source: Source {
                url: "https://www.youtube.com/watch?v=test123".into(),
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
                            Word { id: "w0".into(), text: "Welcome".into(), start: 0.0, end: 0.5 },
                            Word { id: "w1".into(), text: "to".into(), start: 0.5, end: 0.6 },
                            Word { id: "w2".into(), text: "the".into(), start: 0.6, end: 0.7 },
                            Word { id: "w3".into(), text: "show".into(), start: 0.7, end: 1.0 },
                        ],
                    }],
                }],
                translations: Default::default(),
            }),
            keywords: vec![
                Keyword { key: "AI".into(), desc: Some("Artificial Intelligence".into()) },
                Keyword { key: "podcast".into(), desc: None },
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

    #[test]
    fn roundtrip_insert_get() {
        let store = Store::open_memory().unwrap();
        let ep = sample_episode();
        store.insert_episode(&ep).unwrap();
        let loaded = store.get_episode(&ep.id).unwrap();
        assert_eq!(loaded.id, ep.id);
        assert_eq!(loaded.source.title, "Test Podcast Episode");
        assert_eq!(
            loaded.summary.unwrap().one_liner,
            "A great podcast about AI"
        );
    }

    #[test]
    fn list_and_count() {
        let store = Store::open_memory().unwrap();
        let ep = sample_episode();
        store.insert_episode(&ep).unwrap();
        assert_eq!(store.episode_count().unwrap(), 1);
        let list = store.list_episodes(10, 0).unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn search_fts() {
        let store = Store::open_memory().unwrap();
        let ep = sample_episode();
        store.insert_episode(&ep).unwrap();
        let results = store.search_episodes("podcast", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], ep.id);
    }

    #[test]
    fn delete_removes_episode() {
        let store = Store::open_memory().unwrap();
        let ep = sample_episode();
        store.insert_episode(&ep).unwrap();
        assert!(store.delete_episode(&ep.id).unwrap());
        assert_eq!(store.episode_count().unwrap(), 0);
    }

    #[test]
    fn duplicate_url_check() {
        let store = Store::open_memory().unwrap();
        let ep = sample_episode();
        store.insert_episode(&ep).unwrap();
        assert!(store.episode_exists_by_url(&ep.source.url).unwrap());
        assert!(!store.episode_exists_by_url("https://other.url").unwrap());
    }

    #[test]
    fn user_invite_flow() {
        let store = Store::open_memory().unwrap();
        assert!(!store.is_email_invited("test@example.com").unwrap());

        let invite = Invite {
            id: uuid::Uuid::new_v4().to_string(),
            email: "test@example.com".into(),
            invited_by: "admin-id".into(),
            created_at: Utc::now(),
            accepted_at: None,
        };
        store.create_invite(&invite).unwrap();
        assert!(store.is_email_invited("test@example.com").unwrap());

        let user = User {
            id: uuid::Uuid::new_v4().to_string(),
            email: "test@example.com".into(),
            name: Some("Test User".into()),
            avatar_url: None,
            role: UserRole::Member,
            created_at: Utc::now(),
        };
        store.upsert_user(&user).unwrap();
        let loaded = store
            .get_user_by_email("test@example.com")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Test User"));
    }
}
