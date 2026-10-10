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
                highlights_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
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
             highlights_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                serde_json::to_string(&ep.highlights)?,
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
        let summary_text = ep
            .summary
            .as_ref()
            .map(|s| {
                format!(
                    "{} {} {}",
                    s.one_liner,
                    s.r#abstract,
                    s.key_takeaways.join(" ")
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
        let row = conn
            .query_row(
                "SELECT source_json, processing_json, summary_json, chapters_json,
                 mindmap_json, transcript_json, highlights_json, created_at, updated_at
                 FROM episodes WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| PodError::NotFound(id.to_string()))?;

        Ok(Episode {
            id: id.to_string(),
            source: serde_json::from_str(&row.0)?,
            processing: serde_json::from_str(&row.1)?,
            summary: row.2.as_deref().map(serde_json::from_str).transpose()?,
            chapters: serde_json::from_str(&row.3)?,
            mindmap: row.4.as_deref().map(serde_json::from_str).transpose()?,
            transcript: row.5.as_deref().map(serde_json::from_str).transpose()?,
            highlights: serde_json::from_str(&row.6)?,
            created_at: chrono::DateTime::parse_from_rfc3339(&row.7)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| PodError::InvalidInput(e.to_string()))?,
            updated_at: chrono::DateTime::parse_from_rfc3339(&row.8)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| PodError::InvalidInput(e.to_string()))?,
        })
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
            },
            processing: Processing {
                status: ProcessingStatus::Done,
                asr_engine: Some("qwen3-asr".into()),
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
                keywords: vec!["AI".into(), "podcast".into()],
            }),
            chapters: vec![Chapter {
                title: "Introduction".into(),
                start_time: 0.0,
                end_time: 120.0,
                summary: "The host introduces the topic".into(),
            }],
            mindmap: None,
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
            highlights: vec![],
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
