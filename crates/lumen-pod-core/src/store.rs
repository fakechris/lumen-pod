use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::episode::{ApiToken, Chapter, Episode, Invite, User, UserRole};
use crate::error::{PodError, PodResult};

/// Per-user list filtering/ordering for episode card queries.
#[derive(Debug, Default, Clone)]
pub struct EpisodeListQuery {
    /// `Some("unread")` — only episodes the user hasn't read;
    /// `Some("favorites")` — only the user's favorites.
    pub filter: Option<String>,
    /// Only episodes carrying this personal tag for the user.
    pub tag: Option<String>,
    /// `Some("unread")` — unread first; default `recent` (created_at DESC).
    pub sort: Option<String>,
}

/// Runtime bind value for dynamically-built card queries (all binds are text).
enum BindVal {
    S(String),
}

pub struct Store {
    pool: PgPool,
    episode_locks: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>,
        >,
    >,
}

impl Store {
    pub async fn new(database_url: &str) -> PodResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        let store = Self {
            pool,
            episode_locks: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        };
        store.migrate().await?;
        Ok(store)
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            episode_locks: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    /// Return a per-episode async mutex so concurrent read-modify-write operations
    /// on the same episode are serialized in-process in addition to DB row locking.
    pub fn episode_lock(&self, id: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        let mut map = self.episode_locks.lock().unwrap();
        map.entry(id.to_string())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
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
        let visibility = if ep.visibility == "private" {
            "private"
        } else {
            "public"
        };

        sqlx::query(
            "INSERT INTO episodes (
                id, source_url, source, processing, summary, chapters,
                mindmap, transcript, keywords, highlights, qas, clips,
                article, shownotes, is_read, tags, collection_id,
                owner_id, visibility,
                search_vector, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16, $17,
                $18, $19,
                to_tsvector('english', $20), $21, $22
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
        .bind(&ep.owner_id)
        .bind(visibility)
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
                    is_read, tags, collection_id, owner_id, visibility, created_at, updated_at
             FROM episodes WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| PodError::NotFound(id.to_string()))?;

        Self::row_to_episode(id, &row)
    }

    pub async fn list_episodes(&self, limit: usize, offset: usize) -> PodResult<Vec<Episode>> {
        self.list_episodes_for_user(limit, offset, None, true).await
    }

    pub async fn list_episodes_for_user(
        &self,
        limit: usize,
        offset: usize,
        user_id: Option<&str>,
        is_admin: bool,
    ) -> PodResult<Vec<Episode>> {
        let rows = if is_admin {
            sqlx::query(
                "SELECT id, source, processing, summary, chapters, mindmap, transcript,
                        keywords, highlights, qas, clips, article, shownotes,
                        is_read, tags, collection_id, owner_id, visibility, created_at, updated_at
                 FROM episodes ORDER BY created_at DESC LIMIT $1 OFFSET $2",
            )
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, source, processing, summary, chapters, mindmap, transcript,
                        keywords, highlights, qas, clips, article, shownotes,
                        is_read, tags, collection_id, owner_id, visibility, created_at, updated_at
                 FROM episodes
                 WHERE visibility = 'public' OR owner_id = $3
                 ORDER BY created_at DESC LIMIT $1 OFFSET $2",
            )
            .bind(limit as i64)
            .bind(offset as i64)
            .bind(user_id.unwrap_or(""))
            .fetch_all(&self.pool)
            .await?
        };

        let mut episodes: Vec<Episode> = rows
            .iter()
            .map(|row| {
                let id: &str = row.get("id");
                Self::row_to_episode(id, row)
            })
            .collect::<PodResult<Vec<_>>>()?;

        if let Some(uid) = user_id {
            self.hydrate_episodes_user_state(&mut episodes, uid).await?;
        }

        Ok(episodes)
    }

    /// Slim card variant of [`Self::list_episodes_for_user`] for list/search UIs:
    /// skips transcript / article / mindmap / chapters / qas / clips / highlights /
    /// keywords / shownotes so a 50-item page stays in the KB range instead of
    /// shipping tens of MB of word-level transcript JSON.
    ///
    /// Supports per-user filtering (`unread` / `favorites` / personal `tag`)
    /// and ordering (`recent` default / `unread` first).
    pub async fn list_episode_cards_for_user(
        &self,
        limit: usize,
        offset: usize,
        user_id: Option<&str>,
        is_admin: bool,
        query: &EpisodeListQuery,
    ) -> PodResult<Vec<Episode>> {
        let uid = user_id.unwrap_or("");
        let (where_sql, binds) = Self::episode_filter_sql(uid, is_admin, query);
        let order_sql = Self::episode_order_sql(query);

        let sql = format!(
            "SELECT e.id AS id, e.source AS source, e.processing AS processing,
                    e.summary AS summary, e.is_read AS is_read, e.tags AS tags,
                    e.collection_id AS collection_id, e.owner_id AS owner_id,
                    e.visibility AS visibility, e.created_at AS created_at,
                    e.updated_at AS updated_at
             FROM episodes e
             LEFT JOIN user_episode_reads r ON r.episode_id = e.id AND r.user_id = $1
             {where_sql}
             ORDER BY {order_sql}
             LIMIT {limit} OFFSET {offset}"
        );

        let mut q = sqlx::query(&sql).bind(uid.to_string());
        for v in binds {
            q = match v {
                BindVal::S(s) => q.bind(s),
            };
        }
        let rows = q.fetch_all(&self.pool).await?;

        let mut episodes: Vec<Episode> = rows
            .iter()
            .map(Self::row_to_episode_card)
            .collect::<PodResult<Vec<_>>>()?;

        if !uid.is_empty() {
            self.hydrate_episodes_user_state(&mut episodes, uid).await?;
        }

        Ok(episodes)
    }

    /// Row count matching the same filters as
    /// [`Self::list_episode_cards_for_user`] so list pagination stays correct
    /// when filters narrow the result set.
    pub async fn episode_count_for_user_filtered(
        &self,
        user_id: Option<&str>,
        is_admin: bool,
        query: &EpisodeListQuery,
    ) -> PodResult<usize> {
        let uid = user_id.unwrap_or("");
        let (where_sql, binds) = Self::episode_filter_sql(uid, is_admin, query);
        let sql = format!(
            "SELECT COUNT(*) AS cnt FROM episodes e
             LEFT JOIN user_episode_reads r ON r.episode_id = e.id AND r.user_id = $1
             {where_sql}"
        );
        let mut q = sqlx::query(&sql).bind(uid.to_string());
        for v in binds {
            q = match v {
                BindVal::S(s) => q.bind(s),
            };
        }
        let row = q.fetch_one(&self.pool).await?;
        Ok(row.get::<i64, _>("cnt") as usize)
    }

    /// Build the WHERE clause shared by list/count card queries. The caller
    /// must bind `$1` = user_id (the read-state join) before the returned
    /// extra binds, in order.
    fn episode_filter_sql(user_id: &str, is_admin: bool, query: &EpisodeListQuery) -> (String, Vec<BindVal>) {
        let mut binds = Vec::new();
        let mut wheres: Vec<String> = Vec::new();

        if !is_admin {
            binds.push(BindVal::S(user_id.to_string()));
            let n = binds.len() + 1; // +1: $1 is reserved for the reads join
            wheres.push(format!("(e.visibility = 'public' OR e.owner_id = ${n})"));
        }
        match query.filter.as_deref() {
            Some("unread") => wheres.push("NOT COALESCE(r.is_read, FALSE)".to_string()),
            Some("favorites") => {
                binds.push(BindVal::S(user_id.to_string()));
                let n = binds.len() + 1;
                wheres.push(format!(
                    "EXISTS (SELECT 1 FROM user_favorites f WHERE f.episode_id = e.id AND f.user_id = ${n})"
                ));
            }
            _ => {}
        }
        if let Some(tag) = query.tag.as_deref().filter(|t| !t.is_empty()) {
            binds.push(BindVal::S(tag.to_string()));
            let n = binds.len() + 1;
            wheres.push(format!(
                "EXISTS (SELECT 1 FROM user_episode_tags t \
                 WHERE t.episode_id = e.id AND t.user_id = $1 AND t.tag = ${n})"
            ));
        }

        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wheres.join(" AND "))
        };
        (where_sql, binds)
    }

    fn episode_order_sql(query: &EpisodeListQuery) -> &'static str {
        match query.sort.as_deref() {
            Some("unread") => "COALESCE(r.is_read, FALSE) ASC, e.created_at DESC",
            _ => "e.created_at DESC",
        }
    }

    /// Slim card fetch by explicit ids (used by search: FTS returns ranked ids,
    /// we only need card fields for the results page).
    pub async fn get_episode_cards_by_ids(&self, ids: &[String]) -> PodResult<Vec<Episode>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT id, source, processing, summary,
                    is_read, tags, collection_id, owner_id, visibility, created_at, updated_at
             FROM episodes WHERE id = ANY($1)",
        )
        .bind(ids.to_vec())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_episode_card).collect()
    }

    /// Chapters only (with any persisted deep reads). Used by the batch
    /// deep-read status endpoint to ship newly generated deep reads without
    /// pulling the full episode — the transcript alone averages ~700KB/episode.
    pub async fn get_episode_chapters(&self, id: &str) -> PodResult<Vec<Chapter>> {
        let row = sqlx::query("SELECT chapters FROM episodes WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row
            .and_then(|r| r.get::<Option<serde_json::Value>, _>("chapters"))
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default())
    }

    /// Map a card-only row (see [`Self::list_episode_cards_for_user`]) to an
    /// Episode with the heavy AI-content sections left empty — the list card
    /// and search result renderers never read them.
    fn row_to_episode_card(row: &sqlx::postgres::PgRow) -> PodResult<Episode> {
        Ok(Episode {
            id: row.get("id"),
            source: serde_json::from_value(row.get::<serde_json::Value, _>("source"))?,
            processing: serde_json::from_value(row.get::<serde_json::Value, _>("processing"))?,
            summary: row
                .get::<Option<serde_json::Value>, _>("summary")
                .and_then(|v| serde_json::from_value(v).ok()),
            chapters: Vec::new(),
            mindmap: None,
            transcript: None,
            keywords: Vec::new(),
            highlights: Vec::new(),
            qas: Vec::new(),
            clips: Vec::new(),
            article: None,
            shownotes: None,
            is_read: row.get("is_read"),
            is_favorite: false,
            my_tags: Vec::new(),
            tags: row
                .get::<Option<serde_json::Value>, _>("tags")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            collection_id: row.get("collection_id"),
            owner_id: row.try_get("owner_id").unwrap_or(None),
            visibility: row
                .try_get::<String, _>("visibility")
                .unwrap_or_else(|_| "public".to_string()),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn set_user_episode_read(
        &self,
        user_id: &str,
        episode_id: &str,
        is_read: bool,
    ) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO user_episode_reads (user_id, episode_id, is_read, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (user_id, episode_id) DO UPDATE SET
               is_read = EXCLUDED.is_read,
               updated_at = NOW()",
        )
        .bind(user_id)
        .bind(episode_id)
        .bind(is_read)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_user_episode_read(
        &self,
        user_id: &str,
        episode_id: &str,
    ) -> PodResult<bool> {
        let row = sqlx::query(
            "SELECT is_read FROM user_episode_reads WHERE user_id = $1 AND episode_id = $2",
        )
        .bind(user_id)
        .bind(episode_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.get::<bool, _>("is_read")).unwrap_or(false))
    }

    pub async fn hydrate_episodes_read_state(
        &self,
        episodes: &mut [Episode],
        user_id: &str,
    ) -> PodResult<()> {
        if episodes.is_empty() || user_id.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = episodes.iter().map(|e| e.id.clone()).collect();
        let rows = sqlx::query(
            "SELECT episode_id, is_read FROM user_episode_reads
             WHERE user_id = $1 AND episode_id = ANY($2)",
        )
        .bind(user_id)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;

        let mut read_map = std::collections::HashMap::new();
        for r in rows {
            let ep_id: String = r.get("episode_id");
            let is_read: bool = r.get("is_read");
            read_map.insert(ep_id, is_read);
        }

        for ep in episodes.iter_mut() {
            ep.is_read = *read_map.get(&ep.id).unwrap_or(&false);
        }
        Ok(())
    }

    /// Hydrate all per-user state on a batch of episodes in one pass:
    /// read flags, favorite flags and personal tags.
    pub async fn hydrate_episodes_user_state(
        &self,
        episodes: &mut [Episode],
        user_id: &str,
    ) -> PodResult<()> {
        if episodes.is_empty() || user_id.is_empty() {
            return Ok(());
        }
        self.hydrate_episodes_read_state(episodes, user_id).await?;

        let ids: Vec<String> = episodes.iter().map(|e| e.id.clone()).collect();

        let fav_rows = sqlx::query(
            "SELECT episode_id FROM user_favorites
             WHERE user_id = $1 AND episode_id = ANY($2)",
        )
        .bind(user_id)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        let fav_set: std::collections::HashSet<String> = fav_rows
            .iter()
            .map(|r| r.get::<String, _>("episode_id"))
            .collect();

        let tag_rows = sqlx::query(
            "SELECT episode_id, tag FROM user_episode_tags
             WHERE user_id = $1 AND episode_id = ANY($2)
             ORDER BY created_at ASC",
        )
        .bind(user_id)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await?;
        let mut tag_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for r in tag_rows {
            let ep_id: String = r.get("episode_id");
            let tag: String = r.get("tag");
            tag_map.entry(ep_id).or_default().push(tag);
        }

        for ep in episodes.iter_mut() {
            ep.is_favorite = fav_set.contains(&ep.id);
            ep.my_tags = tag_map.remove(&ep.id).unwrap_or_default();
        }
        Ok(())
    }

    // ---- Per-user favorites ----

    pub async fn set_user_favorite(
        &self,
        user_id: &str,
        episode_id: &str,
        favorited: bool,
    ) -> PodResult<()> {
        if favorited {
            sqlx::query(
                "INSERT INTO user_favorites (user_id, episode_id)
                 VALUES ($1, $2)
                 ON CONFLICT (user_id, episode_id) DO NOTHING",
            )
            .bind(user_id)
            .bind(episode_id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("DELETE FROM user_favorites WHERE user_id = $1 AND episode_id = $2")
                .bind(user_id)
                .bind(episode_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn get_user_favorite(&self, user_id: &str, episode_id: &str) -> PodResult<bool> {
        let row = sqlx::query(
            "SELECT 1 AS one FROM user_favorites WHERE user_id = $1 AND episode_id = $2",
        )
        .bind(user_id)
        .bind(episode_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Favorite episode ids for a user, newest first.
    pub async fn list_user_favorite_ids(&self, user_id: &str) -> PodResult<Vec<String>> {
        let rows = sqlx::query(
            "SELECT episode_id FROM user_favorites WHERE user_id = $1
             ORDER BY created_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| r.get::<String, _>("episode_id")).collect())
    }

    // ---- Per-user personal tags ----

    /// Replace the user's personal tag set on an episode (full replace).
    pub async fn set_user_episode_tags(
        &self,
        user_id: &str,
        episode_id: &str,
        tags: &[String],
    ) -> PodResult<Vec<String>> {
        let mut clean: Vec<String> = tags
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect();
        clean.sort();
        clean.dedup();

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM user_episode_tags WHERE user_id = $1 AND episode_id = $2")
            .bind(user_id)
            .bind(episode_id)
            .execute(&mut *tx)
            .await?;
        for tag in &clean {
            sqlx::query(
                "INSERT INTO user_episode_tags (user_id, episode_id, tag)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (user_id, episode_id, tag) DO NOTHING",
            )
            .bind(user_id)
            .bind(episode_id)
            .bind(tag)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(clean)
    }

    pub async fn get_user_episode_tags(
        &self,
        user_id: &str,
        episode_id: &str,
    ) -> PodResult<Vec<String>> {
        let rows = sqlx::query(
            "SELECT tag FROM user_episode_tags
             WHERE user_id = $1 AND episode_id = $2
             ORDER BY created_at ASC",
        )
        .bind(user_id)
        .bind(episode_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|r| r.get::<String, _>("tag")).collect())
    }

    /// The user's personal tag cloud: (tag, usage count), most used first.
    pub async fn list_user_tags(&self, user_id: &str) -> PodResult<Vec<(String, i64)>> {
        let rows = sqlx::query(
            "SELECT tag, COUNT(*) AS cnt FROM user_episode_tags
             WHERE user_id = $1
             GROUP BY tag
             ORDER BY cnt DESC, tag ASC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("tag"), r.get::<i64, _>("cnt")))
            .collect())
    }

    // ---- Per-user behavioral events ----

    /// Record opened-at on the per-user read row (does not flip is_read).
    pub async fn mark_user_episode_opened(&self, user_id: &str, episode_id: &str) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO user_episode_reads (user_id, episode_id, is_read, last_opened_at)
             VALUES ($1, $2, TRUE, NOW())
             ON CONFLICT (user_id, episode_id) DO UPDATE SET last_opened_at = NOW()",
        )
        .bind(user_id)
        .bind(episode_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Append a batch of behavioral events for the user.
    pub async fn record_user_events(
        &self,
        user_id: &str,
        events: &[(String, String, Option<serde_json::Value>)], // (episode_id, event_type, payload)
    ) -> PodResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for (episode_id, event_type, payload) in events {
            sqlx::query(
                "INSERT INTO user_events (user_id, episode_id, event_type, payload)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(user_id)
            .bind(episode_id)
            .bind(event_type)
            .bind(payload)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn update_episode(
        &self,
        id: &str,
        updates: &serde_json::Value,
    ) -> PodResult<Episode> {
        let lock = self.episode_lock(id);
        let _guard = lock.lock().await;

        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT source, processing, summary, chapters, mindmap, transcript,
                    keywords, highlights, qas, clips, article, shownotes,
                    is_read, tags, collection_id, owner_id, visibility, created_at, updated_at
             FROM episodes WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| PodError::NotFound(id.to_string()))?;

        let mut ep = Self::row_to_episode(id, &row)?;

        // Helper: try both camelCase and snake_case keys
        macro_rules! patch {
            ($field:ident, $key1:expr, $key2:expr) => {
                if let Some(v) = updates.get($key1).or_else(|| updates.get($key2)) {
                    if let Ok(val) = serde_json::from_value(v.clone()) {
                        ep.$field = val;
                    }
                }
            };
        }

        // Source & processing (new)
        if let Some(v) = updates.get("source") {
            if let Ok(s) = serde_json::from_value(v.clone()) {
                ep.source = s;
            }
        }
        if let Some(v) = updates.get("processing") {
            if let Ok(p) = serde_json::from_value(v.clone()) {
                ep.processing = p;
            }
        }
        if let Some(v) = updates.get("transcript") {
            if let Ok(t) = serde_json::from_value(v.clone()) {
                ep.transcript = Some(t);
            }
        }

        // Existing fields
        if let Some(v) = updates.get("isRead").or_else(|| updates.get("is_read")) {
            ep.is_read = v.as_bool().unwrap_or(false);
        }
        patch!(tags, "tags", "tags");
        if let Some(v) = updates.get("collectionId").or_else(|| updates.get("collection_id")) {
            ep.collection_id = if v.is_null() {
                None
            } else {
                v.as_str().filter(|s| !s.is_empty()).map(String::from)
            };
        }
        if let Some(v) = updates.get("visibility") {
            if let Some(vis) = v.as_str() {
                if vis == "public" || vis == "private" {
                    ep.visibility = vis.to_string();
                }
            }
        }
        if let Some(v) = updates.get("ownerId").or_else(|| updates.get("owner_id")) {
            if let Some(oid) = v.as_str() {
                ep.owner_id = Some(oid.to_string());
            }
        }
        if let Some(v) = updates.get("summary") {
            if v.is_null() {
                ep.summary = None;
            } else if let Ok(s) = serde_json::from_value(v.clone()) {
                ep.summary = Some(s);
            }
        }
        patch!(keywords, "keywords", "keywords");
        patch!(qas, "qas", "qas");
        patch!(clips, "clips", "clips");
        patch!(highlights, "highlights", "highlights");
        patch!(chapters, "chapters", "chapters");
        if let Some(v) = updates.get("article") {
            if v.is_null() {
                ep.article = None;
            } else if let Ok(a) = serde_json::from_value(v.clone()) {
                ep.article = Some(a);
            }
        }
        if let Some(v) = updates.get("shownotes") {
            ep.shownotes = if v.is_null() {
                None
            } else {
                v.as_str().map(String::from)
            };
        }
        if let Some(v) = updates.get("mindmap") {
            if v.is_null() {
                ep.mindmap = None;
            } else if let Ok(m) = serde_json::from_value(v.clone()) {
                ep.mindmap = Some(m);
            }
        }

        ep.updated_at = Utc::now();
        let search_text = Self::build_search_text(&ep);

        sqlx::query(
            "UPDATE episodes SET
                source = $1, processing = $2, transcript = $3,
                summary = $4, chapters = $5, mindmap = $6,
                keywords = $7, highlights = $8, qas = $9,
                clips = $10, article = $11, shownotes = $12,
                is_read = $13, tags = $14, collection_id = $15,
                owner_id = $16, visibility = $17,
                search_vector = to_tsvector('english', $18),
                updated_at = $19
             WHERE id = $20",
        )
        .bind(serde_json::to_value(&ep.source)?)
        .bind(serde_json::to_value(&ep.processing)?)
        .bind(
            ep.transcript
                .as_ref()
                .map(serde_json::to_value)
                .transpose()?,
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
        .bind(&ep.owner_id)
        .bind(&ep.visibility)
        .bind(&search_text)
        .bind(ep.updated_at)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(ep)
    }

    /// Concurrency-safe, single-chapter update for `chapters[idx].deepRead`.
    ///
    /// Only the `chapters` column is touched, and only the one array element is
    /// rewritten (`jsonb_set`). The previous implementation selected the whole
    /// episode row `FOR UPDATE` — transcript included, ~700KB — and then
    /// rewrote every column, so persisting a single chapter re-wrote the
    /// entire episode. A batch over N chapters amplified into N full-episode
    /// writes; measured on production, Postgres checkpoints stretched to 17–22s
    /// while a batch was running.
    ///
    /// The targeted UPDATE is also *safer* than read-modify-write: Postgres
    /// applies `jsonb_set` to the row's current version under a row lock, so
    /// concurrent chapter writes serialise without clobbering each other and no
    /// `SELECT ... FOR UPDATE` of the full row is needed.
    pub async fn update_chapter_deep_read(
        &self,
        id: &str,
        idx: usize,
        deep_read: &str,
    ) -> PodResult<()> {
        self.update_chapter_deep_read_with_delay(id, idx, deep_read, None)
            .await
    }

    pub async fn update_chapter_deep_read_with_delay(
        &self,
        id: &str,
        idx: usize,
        deep_read: &str,
        after_read_delay: Option<std::time::Duration>,
    ) -> PodResult<()> {
        let lock = self.episode_lock(id);
        let _guard = lock.lock().await;

        if let Some(delay) = after_read_delay {
            tokio::time::sleep(delay).await;
        }

        // Guard the index before writing: `jsonb_set` on an out-of-range index
        // would silently pad the chapters array with nulls.
        let count: Option<i32> =
            sqlx::query_scalar("SELECT jsonb_array_length(chapters) FROM episodes WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(count) = count else {
            return Err(PodError::NotFound(id.to_string()));
        };
        if idx as i32 >= count {
            return Err(PodError::InvalidInput(format!(
                "chapter index {idx} out of range (len={count})"
            )));
        }

        let result = sqlx::query(
            "UPDATE episodes
                SET chapters = jsonb_set(chapters, ARRAY[($2::int)::text, 'deepRead'], to_jsonb($3::text), true),
                    updated_at = now()
              WHERE id = $1",
        )
        .bind(id)
        .bind(idx as i32)
        .bind(deep_read)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(PodError::NotFound(id.to_string()));
        }
        Ok(())
    }

    /// Targeted, concurrency-safe update for `episode.article` that never touches `chapters` or other columns.
    pub async fn update_episode_article(
        &self,
        id: &str,
        article: &crate::episode::Article,
    ) -> PodResult<()> {
        let lock = self.episode_lock(id);
        let _guard = lock.lock().await;

        let result = sqlx::query("UPDATE episodes SET article = $1, updated_at = $2 WHERE id = $3")
            .bind(serde_json::to_value(article)?)
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(PodError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub async fn delete_episode(&self, id: &str) -> PodResult<bool> {
        let result = sqlx::query("DELETE FROM episodes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.episode_locks.lock().unwrap().remove(id);
        Ok(result.rows_affected() > 0)
    }

    pub async fn search_episodes(&self, query: &str, limit: usize) -> PodResult<Vec<String>> {
        self.search_episodes_for_user(query, limit, None, true).await
    }

    pub async fn search_episodes_for_user(
        &self,
        query: &str,
        limit: usize,
        user_id: Option<&str>,
        is_admin: bool,
    ) -> PodResult<Vec<String>> {
        let rows = if is_admin {
            sqlx::query(
                "SELECT id FROM episodes
                 WHERE search_vector @@ websearch_to_tsquery('english', $1)
                 ORDER BY ts_rank(search_vector, websearch_to_tsquery('english', $1)) DESC
                 LIMIT $2",
            )
            .bind(query)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id FROM episodes
                 WHERE search_vector @@ websearch_to_tsquery('english', $1)
                   AND (visibility = 'public' OR owner_id = $3)
                 ORDER BY ts_rank(search_vector, websearch_to_tsquery('english', $1)) DESC
                 LIMIT $2",
            )
            .bind(query)
            .bind(limit as i64)
            .bind(user_id.unwrap_or(""))
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows.iter().map(|r| r.get::<String, _>("id")).collect())
    }

    pub async fn episode_count(&self) -> PodResult<usize> {
        self.episode_count_for_user(None, true).await
    }

    pub async fn episode_count_for_user(
        &self,
        user_id: Option<&str>,
        is_admin: bool,
    ) -> PodResult<usize> {
        let row = if is_admin {
            sqlx::query("SELECT COUNT(*) as cnt FROM episodes")
                .fetch_one(&self.pool)
                .await?
        } else {
            sqlx::query(
                "SELECT COUNT(*) as cnt FROM episodes WHERE visibility = 'public' OR owner_id = $1",
            )
            .bind(user_id.unwrap_or(""))
            .fetch_one(&self.pool)
            .await?
        };
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

    pub async fn find_episode_by_url(&self, url: &str) -> PodResult<Option<Episode>> {
        let row = sqlx::query(
            "SELECT id, source, processing, summary, chapters, mindmap, transcript,
                    keywords, highlights, qas, clips, article, shownotes,
                    is_read, tags, collection_id, owner_id, visibility, created_at, updated_at
             FROM episodes WHERE source_url = $1 LIMIT 1",
        )
        .bind(url)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => {
                let id: String = r.get("id");
                Ok(Some(Self::row_to_episode(&id, &r)?))
            }
            None => Ok(None),
        }
    }

    // ---- User / Auth ----

    pub async fn upsert_user(&self, user: &User) -> PodResult<()> {
        let is_admin = user.role == UserRole::Admin;
        sqlx::query(
            "INSERT INTO users (id, email, name, avatar_url, role, approved, can_edit, can_delete, can_publish_private, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT(email) DO UPDATE SET
               name = EXCLUDED.name,
               avatar_url = EXCLUDED.avatar_url,
               can_edit = EXCLUDED.can_edit,
               can_delete = EXCLUDED.can_delete,
               can_publish_private = EXCLUDED.can_publish_private",
        )
        .bind(&user.id)
        .bind(&user.email)
        .bind(&user.name)
        .bind(&user.avatar_url)
        .bind(if is_admin { "admin" } else { "member" })
        .bind(user.approved)
        .bind(is_admin || user.can_edit)
        .bind(is_admin || user.can_delete)
        .bind(is_admin || user.can_publish_private)
        .bind(user.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Seed a built-in admin. Idempotent: if user exists, upgrades to admin+approved with full permissions.
    pub async fn seed_admin(&self, email: &str) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO users (id, email, role, approved, can_edit, can_delete, can_publish_private, created_at)
             VALUES ($1, $2, 'admin', TRUE, TRUE, TRUE, TRUE, NOW())
             ON CONFLICT(email) DO UPDATE SET
               role = 'admin',
               approved = TRUE,
               can_edit = TRUE,
               can_delete = TRUE,
               can_publish_private = TRUE",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(email)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Upsert user from Google OAuth. Does NOT block anyone.
    /// - First human user in the system (no admin with google_sub yet, or empty users table) → role=admin, approved=true, all permissions=true
    /// - Admin emails → role=admin, approved=true, all permissions=true
    /// - Invited emails → role=member, approved=true
    /// - Others → role=member, approved=false (pending)
    pub async fn upsert_user_google(
        &self,
        email: &str,
        name: Option<&str>,
        avatar_url: Option<&str>,
        google_sub: &str,
        admin_emails: &[String],
    ) -> PodResult<User> {
        let has_google_admin: bool = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM users WHERE role = 'admin' AND google_sub IS NOT NULL) as exists",
        )
        .fetch_one(&self.pool)
        .await
        .map(|r| r.get::<bool, _>("exists"))
        .unwrap_or(false);

        let is_admin = !has_google_admin
            || admin_emails.iter().any(|e| e.eq_ignore_ascii_case(email));
        let is_invited = self.is_email_invited(email).await.unwrap_or(false);
        let auto_approved = is_admin || is_invited;

        let role = if is_admin { "admin" } else { "member" };

        let row = sqlx::query(
            "INSERT INTO users (
                id, email, name, avatar_url, role, approved,
                can_edit, can_delete, can_publish_private, google_sub, created_at
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $7, $7, $8, NOW())
             ON CONFLICT(email) DO UPDATE SET
               name = COALESCE(EXCLUDED.name, users.name),
               avatar_url = COALESCE(EXCLUDED.avatar_url, users.avatar_url),
               google_sub = EXCLUDED.google_sub,
               role = CASE WHEN EXCLUDED.role = 'admin' THEN 'admin' ELSE users.role END,
               approved = CASE WHEN EXCLUDED.approved THEN TRUE ELSE users.approved END,
               can_edit = CASE WHEN EXCLUDED.role = 'admin' OR users.role = 'admin' THEN TRUE ELSE users.can_edit END,
               can_delete = CASE WHEN EXCLUDED.role = 'admin' OR users.role = 'admin' THEN TRUE ELSE users.can_delete END,
               can_publish_private = CASE WHEN EXCLUDED.role = 'admin' OR users.role = 'admin' THEN TRUE ELSE users.can_publish_private END
             RETURNING id, email, name, avatar_url, role, approved, can_edit, can_delete, can_publish_private, created_at",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(email)
        .bind(name)
        .bind(avatar_url)
        .bind(role)
        .bind(auto_approved)
        .bind(is_admin)
        .bind(google_sub)
        .fetch_one(&self.pool)
        .await?;

        Ok(Self::row_to_user(&row))
    }

    pub async fn get_user_by_email(&self, email: &str) -> PodResult<Option<User>> {
        let row = sqlx::query(
            "SELECT id, email, name, avatar_url, role, approved, can_edit, can_delete, can_publish_private, created_at
             FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.as_ref().map(Self::row_to_user))
    }

    pub async fn get_user_by_id(&self, id: &str) -> PodResult<Option<User>> {
        let row = sqlx::query(
            "SELECT id, email, name, avatar_url, role, approved, can_edit, can_delete, can_publish_private, created_at
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.as_ref().map(Self::row_to_user))
    }

    pub async fn list_users(&self) -> PodResult<Vec<User>> {
        let rows = sqlx::query(
            "SELECT id, email, name, avatar_url, role, approved, can_edit, can_delete, can_publish_private, created_at
             FROM users ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_user).collect())
    }

    pub async fn approve_user(&self, user_id: &str) -> PodResult<bool> {
        let result = sqlx::query("UPDATE users SET approved = TRUE WHERE id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn reject_user(&self, user_id: &str) -> PodResult<bool> {
        let result = sqlx::query("DELETE FROM users WHERE id = $1 AND role != 'admin'")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_user_role(&self, user_id: &str, role: &str) -> PodResult<bool> {
        let is_admin = role == "admin";
        let result = sqlx::query(
            "UPDATE users SET
                role = $1,
                can_edit = CASE WHEN $3 THEN TRUE ELSE can_edit END,
                can_delete = CASE WHEN $3 THEN TRUE ELSE can_delete END,
                can_publish_private = CASE WHEN $3 THEN TRUE ELSE can_publish_private END
             WHERE id = $2",
        )
        .bind(role)
        .bind(user_id)
        .bind(is_admin)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_user_permissions(
        &self,
        user_id: &str,
        approved: Option<bool>,
        role: Option<&str>,
        can_edit: Option<bool>,
        can_delete: Option<bool>,
        can_publish_private: Option<bool>,
    ) -> PodResult<bool> {
        let Some(mut user) = self.get_user_by_id(user_id).await? else {
            return Ok(false);
        };
        if let Some(a) = approved {
            user.approved = a;
        }
        if let Some(r) = role {
            if r == "admin" {
                user.role = UserRole::Admin;
                user.can_edit = true;
                user.can_delete = true;
                user.can_publish_private = true;
            } else if r == "member" {
                user.role = UserRole::Member;
            }
        }
        if let Some(ce) = can_edit {
            user.can_edit = ce || user.role == UserRole::Admin;
        }
        if let Some(cd) = can_delete {
            user.can_delete = cd || user.role == UserRole::Admin;
        }
        if let Some(cp) = can_publish_private {
            user.can_publish_private = cp || user.role == UserRole::Admin;
        }

        let role_str = match user.role {
            UserRole::Admin => "admin",
            UserRole::Member => "member",
        };
        let result = sqlx::query(
            "UPDATE users SET
                approved = $1,
                role = $2,
                can_edit = $3,
                can_delete = $4,
                can_publish_private = $5
             WHERE id = $6",
        )
        .bind(user.approved)
        .bind(role_str)
        .bind(user.can_edit)
        .bind(user.can_delete)
        .bind(user.can_publish_private)
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn is_email_invited(&self, email: &str) -> PodResult<bool> {
        let row = sqlx::query(
            "SELECT EXISTS(SELECT 1 FROM invites WHERE email = $1) as exists",
        )
        .bind(email)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<bool, _>("exists"))
    }

    pub async fn create_invite(&self, invite: &Invite) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO invites (id, email, invited_by, created_at, accepted_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT DO NOTHING",
        )
        .bind(&invite.id)
        .bind(&invite.email)
        .bind(&invite.invited_by)
        .bind(invite.created_at)
        .bind(invite.accepted_at)
        .execute(&self.pool)
        .await?;

        // Auto-approve the user if they already registered
        sqlx::query("UPDATE users SET approved = TRUE WHERE email = $1 AND NOT approved")
            .bind(&invite.email)
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

    // ---- Settings (runtime-editable config; overrides env defaults) ----

    pub async fn get_setting(&self, key: &str) -> PodResult<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>("value")))
    }

    pub async fn get_settings(&self) -> PodResult<std::collections::HashMap<String, String>> {
        let rows = sqlx::query("SELECT key, value FROM settings")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("key"), r.get::<String, _>("value")))
            .collect())
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES ($1, $2, now())
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_setting(&self, key: &str) -> PodResult<()> {
        sqlx::query("DELETE FROM settings WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---- Issued API tokens ----

    pub async fn get_api_token_by_hash(&self, token_hash: &str) -> PodResult<Option<ApiToken>> {
        let row = sqlx::query(
            "SELECT id::text AS id, name, token_hash, token_hint, created_by, created_at,
                    last_used_at, use_count, revoked_at
             FROM api_tokens WHERE token_hash = $1 AND revoked_at IS NULL",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Self::row_to_api_token))
    }

    pub async fn list_api_tokens(&self) -> PodResult<Vec<ApiToken>> {
        let rows = sqlx::query(
            "SELECT id::text AS id, name, token_hash, token_hint, created_by, created_at,
                    last_used_at, use_count, revoked_at
             FROM api_tokens ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Self::row_to_api_token).collect())
    }

    pub async fn insert_api_token(&self, token: &ApiToken) -> PodResult<()> {
        sqlx::query(
            "INSERT INTO api_tokens (id, name, token_hash, token_hint, created_by, created_at)
             VALUES ($1::uuid, $2, $3, $4, $5, $6)",
        )
        .bind(&token.id)
        .bind(&token.name)
        .bind(&token.token_hash)
        .bind(&token.token_hint)
        .bind(&token.created_by)
        .bind(token.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn revoke_api_token(&self, id: &str) -> PodResult<bool> {
        let result = sqlx::query(
            "UPDATE api_tokens SET revoked_at = now() WHERE id = $1::uuid AND revoked_at IS NULL",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Usage bookkeeping — called fire-and-forget after a successful auth.
    pub async fn touch_api_token(&self, id: &str) -> PodResult<()> {
        sqlx::query("UPDATE api_tokens SET last_used_at = now(), use_count = use_count + 1 WHERE id = $1::uuid")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    fn row_to_api_token(r: sqlx::postgres::PgRow) -> ApiToken {
        ApiToken {
            id: r.get("id"),
            name: r.get("name"),
            token_hash: r.get("token_hash"),
            token_hint: r.get("token_hint"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
            last_used_at: r.get("last_used_at"),
            use_count: r.get("use_count"),
            revoked_at: r.get("revoked_at"),
        }
    }

    fn row_to_user(r: &sqlx::postgres::PgRow) -> User {
        let role = match r.get::<String, _>("role").as_str() {
            "admin" => UserRole::Admin,
            _ => UserRole::Member,
        };
        let is_admin = role == UserRole::Admin;
        User {
            id: r.get("id"),
            email: r.get("email"),
            name: r.get("name"),
            avatar_url: r.get("avatar_url"),
            role,
            approved: r.get("approved"),
            can_edit: is_admin || r.try_get("can_edit").unwrap_or(false),
            can_delete: is_admin || r.try_get("can_delete").unwrap_or(false),
            can_publish_private: is_admin || r.try_get("can_publish_private").unwrap_or(false),
            created_at: r.get("created_at"),
        }
    }

    // ---- Chat Messages (Per-User Scoped) ----

    pub async fn add_chat_message(
        &self,
        episode_id: &str,
        role: &str,
        content: &str,
    ) -> PodResult<crate::episode::ChatMessage> {
        self.add_user_chat_message(episode_id, "", role, content).await
    }

    pub async fn add_user_chat_message(
        &self,
        episode_id: &str,
        user_id: &str,
        role: &str,
        content: &str,
    ) -> PodResult<crate::episode::ChatMessage> {
        let id = uuid::Uuid::new_v4().to_string();
        let row = sqlx::query(
            "INSERT INTO chat_messages (id, episode_id, user_id, role, content, created_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             RETURNING id, episode_id, user_id, role, content, created_at",
        )
        .bind(&id)
        .bind(episode_id)
        .bind(user_id)
        .bind(role)
        .bind(content)
        .fetch_one(&self.pool)
        .await?;

        let uid: String = row.get("user_id");
        Ok(crate::episode::ChatMessage {
            id: row.get("id"),
            episode_id: row.get("episode_id"),
            user_id: if uid.is_empty() { None } else { Some(uid) },
            session_id: None,
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
        self.list_user_chat_messages(episode_id, "").await
    }

    pub async fn list_user_chat_messages(
        &self,
        episode_id: &str,
        user_id: &str,
    ) -> PodResult<Vec<crate::episode::ChatMessage>> {
        let rows = sqlx::query(
            "SELECT id, episode_id, user_id, session_id, role, content, created_at
             FROM chat_messages WHERE episode_id = $1 AND user_id = $2
               AND session_id IS NULL
             ORDER BY created_at ASC",
        )
        .bind(episode_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| {
                let uid: String = r.get("user_id");
                crate::episode::ChatMessage {
                    id: r.get("id"),
                    episode_id: r.get("episode_id"),
                    user_id: if uid.is_empty() { None } else { Some(uid) },
                    session_id: r.get("session_id"),
                    role: match r.get::<String, _>("role").as_str() {
                        "assistant" => crate::episode::ChatRole::Assistant,
                        _ => crate::episode::ChatRole::User,
                    },
                    content: r.get("content"),
                    created_at: r.get("created_at"),
                }
            })
            .collect())
    }

    // ---- Chat sessions (multi-conversation per episode) ----

    pub async fn create_chat_session(
        &self,
        user_id: &str,
        episode_id: &str,
        title: &str,
        anchor: Option<&serde_json::Value>,
    ) -> PodResult<crate::episode::ChatSession> {
        let id = uuid::Uuid::new_v4().to_string();
        let row = sqlx::query(
            "INSERT INTO chat_sessions (id, user_id, episode_id, title, anchor, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
             RETURNING id, user_id, episode_id, title, anchor, created_at, updated_at",
        )
        .bind(&id)
        .bind(user_id)
        .bind(episode_id)
        .bind(title)
        .bind(anchor)
        .fetch_one(&self.pool)
        .await?;
        Ok(Self::row_to_chat_session(&row))
    }

    /// Sessions for a user under an episode, newest activity first, with
    /// message counts and last-message excerpts for the sidebar list.
    pub async fn list_chat_sessions(
        &self,
        user_id: &str,
        episode_id: &str,
    ) -> PodResult<Vec<crate::episode::ChatSessionWithStats>> {
        let rows = sqlx::query(
            "SELECT s.id, s.user_id, s.episode_id, s.title, s.anchor, s.created_at, s.updated_at,
                    COALESCE(m.cnt, 0) AS message_count,
                    m.last_content AS last_message
             FROM chat_sessions s
             LEFT JOIN (
                 SELECT session_id, COUNT(*) AS cnt,
                        (array_agg(content ORDER BY created_at DESC))[1] AS last_content
                 FROM chat_messages
                 WHERE session_id IS NOT NULL
                 GROUP BY session_id
             ) m ON m.session_id = s.id
             WHERE s.user_id = $1 AND s.episode_id = $2
             ORDER BY s.updated_at DESC",
        )
        .bind(user_id)
        .bind(episode_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|r| crate::episode::ChatSessionWithStats {
                session: Self::row_to_chat_session(r),
                message_count: r.get("message_count"),
                last_message: r.get("last_message"),
            })
            .collect())
    }

    /// Fetch a session owned by the given user (None otherwise).
    pub async fn get_chat_session_owned(
        &self,
        session_id: &str,
        user_id: &str,
    ) -> PodResult<Option<crate::episode::ChatSession>> {
        let row = sqlx::query(
            "SELECT id, user_id, episode_id, title, anchor, created_at, updated_at
             FROM chat_sessions WHERE id = $1 AND user_id = $2",
        )
        .bind(session_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(Self::row_to_chat_session))
    }

    pub async fn rename_chat_session(
        &self,
        session_id: &str,
        user_id: &str,
        title: &str,
    ) -> PodResult<bool> {
        let res = sqlx::query(
            "UPDATE chat_sessions SET title = $3, updated_at = NOW()
             WHERE id = $1 AND user_id = $2",
        )
        .bind(session_id)
        .bind(user_id)
        .bind(title)
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Delete a session and only its own messages. Other sessions and the
    /// legacy default conversation are untouched.
    pub async fn delete_chat_session(&self, session_id: &str, user_id: &str) -> PodResult<bool> {
        let mut tx = self.pool.begin().await?;
        let del_msgs = sqlx::query(
            "DELETE FROM chat_messages WHERE session_id = $1 AND user_id = $2",
        )
        .bind(session_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        let del_sess = sqlx::query("DELETE FROM chat_sessions WHERE id = $1 AND user_id = $2")
            .bind(session_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(del_sess.rows_affected() > 0 || del_msgs.rows_affected() > 0)
    }

    fn row_to_chat_session(row: &sqlx::postgres::PgRow) -> crate::episode::ChatSession {
        crate::episode::ChatSession {
            id: row.get("id"),
            user_id: row.get("user_id"),
            episode_id: row.get("episode_id"),
            title: row.get("title"),
            anchor: row.get("anchor"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }

    /// Append a message to a specific chat session.
    pub async fn add_session_chat_message(
        &self,
        session_id: &str,
        user_id: &str,
        episode_id: &str,
        role: &str,
        content: &str,
    ) -> PodResult<crate::episode::ChatMessage> {
        let id = uuid::Uuid::new_v4().to_string();
        let row = sqlx::query(
            "INSERT INTO chat_messages (id, episode_id, user_id, session_id, role, content, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, NOW())
             RETURNING id, episode_id, user_id, session_id, role, content, created_at",
        )
        .bind(&id)
        .bind(episode_id)
        .bind(user_id)
        .bind(session_id)
        .bind(role)
        .bind(content)
        .fetch_one(&self.pool)
        .await?;
        Ok(Self::row_to_chat_message(&row))
    }

    pub async fn list_session_chat_messages(
        &self,
        session_id: &str,
    ) -> PodResult<Vec<crate::episode::ChatMessage>> {
        let rows = sqlx::query(
            "SELECT id, episode_id, user_id, session_id, role, content, created_at
             FROM chat_messages WHERE session_id = $1
             ORDER BY created_at ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(Self::row_to_chat_message).collect())
    }

    /// Bump a session's updated_at so the sidebar ordering reflects activity.
    pub async fn touch_chat_session(&self, session_id: &str) -> PodResult<()> {
        sqlx::query("UPDATE chat_sessions SET updated_at = NOW() WHERE id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    fn row_to_chat_message(row: &sqlx::postgres::PgRow) -> crate::episode::ChatMessage {
        let uid: String = row.get("user_id");
        crate::episode::ChatMessage {
            id: row.get("id"),
            episode_id: row.get("episode_id"),
            user_id: if uid.is_empty() { None } else { Some(uid) },
            session_id: row.get("session_id"),
            role: match row.get::<String, _>("role").as_str() {
                "assistant" => crate::episode::ChatRole::Assistant,
                _ => crate::episode::ChatRole::User,
            },
            content: row.get("content"),
            created_at: row.get("created_at"),
        }
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
            is_favorite: false,
            my_tags: Vec::new(),
            tags: row
                .get::<Option<serde_json::Value>, _>("tags")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            collection_id: row.get("collection_id"),
            owner_id: row.try_get("owner_id").unwrap_or(None),
            visibility: row
                .try_get::<String, _>("visibility")
                .unwrap_or_else(|_| "public".to_string()),
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
        // Per-test database isolation: parallel tests each get a private
        // database so exact-count assertions stay valid. Falls back to the
        // shared database when the user cannot CREATE DATABASE (locked-down
        // dev DB); leftover lumenpod_test_* databases on throwaway test
        // containers are harmless.
        if let Some(server) = server_base_url(&url) {
            let db_name = format!("lumenpod_test_{}", uuid::Uuid::new_v4().simple());
            if let Ok(pool) = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&server)
                .await
            {
                let created = sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
                    .execute(&pool)
                    .await
                    .is_ok();
                pool.close().await;
                if created {
                    return Store::new(&format!("{server}/{db_name}")).await.unwrap();
                }
            }
        }
        Store::new(&url).await.unwrap()
    }

    /// `postgres://user:pass@host:port/dbname?params` → `postgres://user:pass@host:port`
    fn server_base_url(url: &str) -> Option<String> {
        let no_query = url.split('?').next().unwrap_or(url);
        let (server, _) = no_query.rsplit_once('/')?;
        if !server.contains("://") {
            return None;
        }
        Some(server.to_string())
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
                audio_paths: Vec::new(),
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
                deep_read: None,
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
            is_favorite: false,
            my_tags: vec![],
            tags: vec!["ai".into(), "tech".into()],
            collection_id: None,
            owner_id: None,
            visibility: "public".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn settings_roundtrip_and_reset() {
        let store = test_store().await;
        assert_eq!(store.get_setting("llm_model").await.unwrap(), None);

        store.set_setting("llm_model", "MiniMax-M3.1-Flash").await.unwrap();
        assert_eq!(
            store.get_setting("llm_model").await.unwrap().as_deref(),
            Some("MiniMax-M3.1-Flash")
        );
        // Upsert overwrites.
        store.set_setting("llm_model", "next-gen-model").await.unwrap();
        assert_eq!(
            store.get_setting("llm_model").await.unwrap().as_deref(),
            Some("next-gen-model")
        );
        let all = store.get_settings().await.unwrap();
        assert_eq!(all.get("llm_model").map(String::as_str), Some("next-gen-model"));

        // Delete resets to "no override".
        store.delete_setting("llm_model").await.unwrap();
        assert_eq!(store.get_setting("llm_model").await.unwrap(), None);
    }

    #[tokio::test]
    async fn get_episode_chapters_roundtrip() {
        let store = test_store().await;
        let id = uuid::Uuid::new_v4().to_string();
        let ep = Episode {
            id: id.clone(),
            source: Source {
                url: "https://example.com/v".into(),
                platform: "youtube".into(),
                title: "t".into(),
                author: "a".into(),
                published_at: None,
                thumbnail_url: None,
                duration: 0.0,
                feed_url: None,
                description: None,
                podcast_name: None,
                episode_cover_url: None,
                language: None,
                audio_path: None,
                audio_paths: vec![],
            },
            processing: Processing {
                status: ProcessingStatus::Done,
                asr_engine: None,
                llm_model: None,
                processed_at: None,
                agent_id: None,
                error: None,
            },
            summary: None,
            chapters: vec![Chapter {
                title: "intro".into(),
                start_time: 0.0,
                end_time: 10.0,
                summary: "s".into(),
                has_ads: false,
                deep_read: None,
            }],
            mindmap: None,
            transcript: None,
            keywords: vec![],
            highlights: vec![],
            qas: vec![],
            clips: vec![],
            article: None,
            shownotes: None,
            is_read: false,
            is_favorite: false,
            my_tags: vec![],
            tags: vec![],
            collection_id: None,
            owner_id: None,
            visibility: "public".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.insert_episode(&ep).await.unwrap();

        assert_eq!(store.get_episode_chapters(&id).await.unwrap().len(), 1);
        assert!(
            store.get_episode_chapters(&id).await.unwrap()[0]
                .deep_read
                .is_none()
        );

        store
            .update_chapter_deep_read(&id, 0, "analysis")
            .await
            .unwrap();
        let chapters = store.get_episode_chapters(&id).await.unwrap();
        assert_eq!(chapters[0].deep_read.as_deref(), Some("analysis"));

        // Unknown id yields empty (status endpoint is polled during races).
        assert!(store.get_episode_chapters("no-such-id").await.unwrap().is_empty());

        store.delete_episode(&id).await.unwrap();
    }

    #[tokio::test]
    async fn api_token_lifecycle() {
        let store = test_store().await;
        let hash = format!("hash-{}", uuid::Uuid::new_v4().simple());
        let token = ApiToken {
            id: uuid::Uuid::new_v4().to_string(),
            name: "cue-download-expert".into(),
            token_hash: hash.clone(),
            token_hint: "lp_deadbeef".into(),
            created_by: Some("admin-1".into()),
            created_at: Utc::now(),
            last_used_at: None,
            use_count: 0,
            revoked_at: None,
        };
        store.insert_api_token(&token).await.unwrap();

        let found = store.get_api_token_by_hash(&hash).await.unwrap().unwrap();
        assert_eq!(found.name, "cue-download-expert");
        assert_eq!(found.use_count, 0);
        assert!(found.revoked_at.is_none());
        assert!(store.get_api_token_by_hash("wrong-hash").await.unwrap().is_none());

        store.touch_api_token(&token.id).await.unwrap();
        store.touch_api_token(&token.id).await.unwrap();
        let found = store.get_api_token_by_hash(&hash).await.unwrap().unwrap();
        assert_eq!(found.use_count, 2);
        assert!(found.last_used_at.is_some());

        assert!(store.revoke_api_token(&token.id).await.unwrap());
        // Revoking twice is a no-op; revoked tokens no longer authenticate.
        assert!(!store.revoke_api_token(&token.id).await.unwrap());
        assert!(store.get_api_token_by_hash(&hash).await.unwrap().is_none());

        let listed = store.list_api_tokens().await.unwrap();
        let ours: Vec<_> = listed.iter().filter(|t| t.token_hash == hash).collect();
        assert_eq!(ours.len(), 1);
        assert!(ours[0].revoked_at.is_some());
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
            approved: false,
            can_edit: false,
            can_delete: false,
            can_publish_private: false,
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

    #[tokio::test]
    async fn patch_article_null_clears_article() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();

        // Set an article
        let set_updates = serde_json::json!({
            "article": {
                "title": "Test Article",
                "content": "Some body content",
                "lang": "zh"
            }
        });
        let updated = store.update_episode(&ep.id, &set_updates).await.unwrap();
        assert!(updated.article.is_some());
        assert_eq!(updated.article.as_ref().unwrap().title, "Test Article");

        // Clear article via {"article": null}
        let clear_updates = serde_json::json!({
            "article": null
        });
        let cleared = store.update_episode(&ep.id, &clear_updates).await.unwrap();
        assert!(cleared.article.is_none());

        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_chapter_deep_reads_with_aligned_read_write_window_preserve_all_chapters() {
        let store = std::sync::Arc::new(test_store().await);
        let mut ep = sample_episode();
        ep.chapters = (0..4)
            .map(|i| Chapter {
                title: format!("Chapter {i}"),
                start_time: (i * 300) as f64,
                end_time: ((i + 1) * 300) as f64,
                summary: format!("Summary {i}"),
                has_ads: false,
                deep_read: None,
            })
            .collect();
        store.insert_episode(&ep).await.unwrap();

        // Align all 4 chapter tasks + 1 concurrent article update at a barrier, and inject an
        // 80ms sleep inside the read-to-write window. Without per-episode locking / FOR UPDATE,
        // all 4 tasks read `deep_read: None` before any writes and clobber each other.
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(5));
        let mut handles = Vec::new();
        for idx in 0..4usize {
            let store_c = store.clone();
            let barrier_c = barrier.clone();
            let ep_id = ep.id.clone();
            handles.push(tokio::spawn(async move {
                barrier_c.wait().await;
                store_c
                    .update_chapter_deep_read_with_delay(
                        &ep_id,
                        idx,
                        &format!("Deep read content for chapter #{idx}"),
                        Some(std::time::Duration::from_millis(80)),
                    )
                    .await
                    .unwrap();
            }));
        }

        let store_art = store.clone();
        let barrier_art = barrier.clone();
        let ep_id_art = ep.id.clone();
        let art_handle = tokio::spawn(async move {
            barrier_art.wait().await;
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            store_art
                .update_episode_article(
                    &ep_id_art,
                    &Article {
                        title: "Concurrent Article".into(),
                        content: "Full article body".into(),
                        format: Some("markdown".into()),
                        lang: Some("zh".into()),
                        focus: None,
                        generated_at: None,
                    },
                )
                .await
                .unwrap();
        });

        for h in handles {
            h.await.unwrap();
        }
        art_handle.await.unwrap();

        let final_ep = store.get_episode(&ep.id).await.unwrap();
        assert_eq!(final_ep.chapters.len(), 4);
        for idx in 0..4usize {
            assert_eq!(
                final_ep.chapters[idx].deep_read.as_deref(),
                Some(format!("Deep read content for chapter #{idx}").as_str()),
                "chapter {idx} deepRead was lost under concurrent aligned writes"
            );
        }
        assert_eq!(
            final_ep.article.as_ref().map(|a| a.title.as_str()),
            Some("Concurrent Article")
        );

        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn favorites_and_personal_tags_are_per_user() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();

        let user_a = format!("ua-{}", uuid::Uuid::new_v4());
        let user_b = format!("ub-{}", uuid::Uuid::new_v4());

        // A favorites and tags; B stays untouched.
        store.set_user_favorite(&user_a, &ep.id, true).await.unwrap();
        store
            .set_user_episode_tags(&user_a, &ep.id, &["machine-learning".into(), " ".into(), "rl".into()])
            .await
            .unwrap();

        assert!(store.get_user_favorite(&user_a, &ep.id).await.unwrap());
        assert!(!store.get_user_favorite(&user_b, &ep.id).await.unwrap());
        assert_eq!(
            store.get_user_episode_tags(&user_a, &ep.id).await.unwrap(),
            vec!["machine-learning".to_string(), "rl".to_string()]
        );
        assert!(store.get_user_episode_tags(&user_b, &ep.id).await.unwrap().is_empty());

        // A's tag cloud has counts; B's is empty.
        let a_tags = store.list_user_tags(&user_a).await.unwrap();
        assert!(a_tags.contains(&("machine-learning".to_string(), 1)));
        assert!(store.list_user_tags(&user_b).await.unwrap().is_empty());

        // Full replace: A sets a new tag set; old tags gone.
        store
            .set_user_episode_tags(&user_a, &ep.id, &["llm".into(), "llm".into()])
            .await
            .unwrap();
        assert_eq!(
            store.get_user_episode_tags(&user_a, &ep.id).await.unwrap(),
            vec!["llm".to_string()]
        );

        // Unfavorite.
        store.set_user_favorite(&user_a, &ep.id, false).await.unwrap();
        assert!(!store.get_user_favorite(&user_a, &ep.id).await.unwrap());
        assert!(store.list_user_favorite_ids(&user_a).await.unwrap().is_empty());

        // Hydration fills per-user fields for A only.
        let mut eps = vec![store.get_episode(&ep.id).await.unwrap()];
        store.set_user_favorite(&user_a, &ep.id, true).await.unwrap();
        store
            .set_user_episode_tags(&user_a, &ep.id, &["llm".into()])
            .await
            .unwrap();
        store.hydrate_episodes_user_state(&mut eps, &user_a).await.unwrap();
        assert!(eps[0].is_favorite);
        assert_eq!(eps[0].my_tags, vec!["llm".to_string()]);
        let mut eps_b = vec![store.get_episode(&ep.id).await.unwrap()];
        store.hydrate_episodes_user_state(&mut eps_b, &user_b).await.unwrap();
        assert!(!eps_b[0].is_favorite);
        assert!(eps_b[0].my_tags.is_empty());

        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn list_cards_filter_and_sort() {
        let store = test_store().await;
        let user = format!("u-{}", uuid::Uuid::new_v4());

        // ep_old: created first, UNREAD, favorited, personal-tagged.
        // ep_new: created later, READ. This makes default (recent) order and
        // unread-first order differ, so both can be asserted.
        let mut ep_old = sample_episode();
        ep_old.id = uuid::Uuid::new_v4().to_string();
        ep_old.source.url = format!("https://x.example/{}", ep_old.id);
        let mut ep_new = sample_episode();
        ep_new.id = uuid::Uuid::new_v4().to_string();
        ep_new.source.url = format!("https://x.example/{}", ep_new.id);
        store.insert_episode(&ep_old).await.unwrap();
        // Ensure distinct created_at ordering.
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        store.insert_episode(&ep_new).await.unwrap();

        store.set_user_episode_read(&user, &ep_new.id, true).await.unwrap();
        store.set_user_favorite(&user, &ep_old.id, true).await.unwrap();
        store
            .set_user_episode_tags(&user, &ep_old.id, &["deep-dive".into()])
            .await
            .unwrap();

        let q = EpisodeListQuery::default();
        let all = store
            .list_episode_cards_for_user(10, 0, Some(&user), false, &q)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        // Default: recent first, and per-user state hydrated.
        assert_eq!(all[0].id, ep_new.id);
        assert!(!all[0].is_favorite && all[0].my_tags.is_empty());
        assert!(all[1].is_favorite && all[1].my_tags == vec!["deep-dive".to_string()]);
        assert_eq!(
            store.episode_count_for_user_filtered(Some(&user), false, &q).await.unwrap(),
            2
        );

        // filter=unread → only ep_old; count follows.
        let unread = EpisodeListQuery {
            filter: Some("unread".into()),
            ..Default::default()
        };
        let got = store
            .list_episode_cards_for_user(10, 0, Some(&user), false, &unread)
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, ep_old.id);
        assert_eq!(
            store.episode_count_for_user_filtered(Some(&user), false, &unread).await.unwrap(),
            1
        );

        // filter=favorites → only ep_old.
        let favs = EpisodeListQuery {
            filter: Some("favorites".into()),
            ..Default::default()
        };
        let got = store
            .list_episode_cards_for_user(10, 0, Some(&user), false, &favs)
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, ep_old.id);

        // tag=deep-dive → only ep_old; unknown tag → empty.
        let by_tag = EpisodeListQuery {
            tag: Some("deep-dive".into()),
            ..Default::default()
        };
        let got = store
            .list_episode_cards_for_user(10, 0, Some(&user), false, &by_tag)
            .await
            .unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, ep_old.id);
        let by_tag = EpisodeListQuery {
            tag: Some("nope".into()),
            ..Default::default()
        };
        assert!(store
            .list_episode_cards_for_user(10, 0, Some(&user), false, &by_tag)
            .await
            .unwrap()
            .is_empty());

        // sort=unread → ep_old (older but unread) jumps ahead of ep_new.
        let unread_sort = EpisodeListQuery {
            sort: Some("unread".into()),
            ..Default::default()
        };
        let got = store
            .list_episode_cards_for_user(10, 0, Some(&user), false, &unread_sort)
            .await
            .unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, ep_old.id, "unread episode must sort first");

        // Pagination offset still applies with filters.
        let page = store
            .list_episode_cards_for_user(1, 1, Some(&user), false, &q)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, ep_old.id);

        store.delete_episode(&ep_old.id).await.unwrap();
        store.delete_episode(&ep_new.id).await.unwrap();
    }

    #[tokio::test]
    async fn chat_sessions_lifecycle() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();
        let user = format!("u-{}", uuid::Uuid::new_v4());
        let other = format!("o-{}", uuid::Uuid::new_v4());

        // Legacy default conversation stays separate from named sessions.
        store
            .add_user_chat_message(&ep.id, &user, "user", "legacy question")
            .await
            .unwrap();
        store
            .add_user_chat_message(&ep.id, &user, "assistant", "legacy answer")
            .await
            .unwrap();

        let s1 = store
            .create_chat_session(
                &user,
                &ep.id,
                "新对话",
                Some(&serde_json::json!({"type": "chapter", "chapterIndex": 0})),
            )
            .await
            .unwrap();
        store
            .add_session_chat_message(&s1.id, &user, &ep.id, "user", "q1")
            .await
            .unwrap();
        store
            .add_session_chat_message(&s1.id, &user, &ep.id, "assistant", "a1")
            .await
            .unwrap();
        store.touch_chat_session(&s1.id).await.unwrap();

        let s2 = store
            .create_chat_session(&other, &ep.id, "theirs", None)
            .await
            .unwrap();

        // List with stats, per-user isolated.
        let list = store.list_chat_sessions(&user, &ep.id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].session.id, s1.id);
        assert_eq!(list[0].message_count, 2);
        assert_eq!(list[0].last_message.as_deref(), Some("a1"));
        assert!(list[0]
            .session
            .anchor
            .as_ref()
            .and_then(|a| a.get("type"))
            .and_then(|v| v.as_str())
            .is_some_and(|t| t == "chapter"));

        // Ownership checks.
        assert!(store.get_chat_session_owned(&s1.id, &user).await.unwrap().is_some());
        assert!(store.get_chat_session_owned(&s1.id, &other).await.unwrap().is_none());

        // Rename.
        assert!(store
            .rename_chat_session(&s1.id, &user, "chapter deep dive")
            .await
            .unwrap());
        assert_eq!(
            store.get_chat_session_owned(&s1.id, &user).await.unwrap().unwrap().title,
            "chapter deep dive"
        );

        // Legacy stream untouched by session messages and vice versa.
        let legacy = store.list_user_chat_messages(&ep.id, &user).await.unwrap();
        assert_eq!(legacy.len(), 2);
        assert!(legacy.iter().all(|m| m.session_id.is_none()));
        let sess_msgs = store.list_session_chat_messages(&s1.id).await.unwrap();
        assert_eq!(sess_msgs.len(), 2);
        assert!(sess_msgs.iter().all(|m| m.session_id.as_deref() == Some(s1.id.as_str())));

        // Delete s1 → only its own messages go; legacy + other users untouched.
        assert!(store.delete_chat_session(&s1.id, &user).await.unwrap());
        assert!(store.list_session_chat_messages(&s1.id).await.unwrap().is_empty());
        assert!(store.get_chat_session_owned(&s1.id, &user).await.unwrap().is_none());
        assert_eq!(store.list_user_chat_messages(&ep.id, &user).await.unwrap().len(), 2);
        assert!(store.get_chat_session_owned(&s2.id, &other).await.unwrap().is_some());

        store.delete_episode(&ep.id).await.unwrap();
    }

    #[tokio::test]
    async fn user_events_and_opened_at() {
        let store = test_store().await;
        let ep = sample_episode();
        store.insert_episode(&ep).await.unwrap();
        let user = format!("u-{}", uuid::Uuid::new_v4());

        // Events land in the stream with payloads.
        store
            .record_user_events(
                &user,
                &[
                    (
                        ep.id.clone(),
                        "view".into(),
                        None,
                    ),
                    (
                        ep.id.clone(),
                        "play_progress".into(),
                        Some(serde_json::json!({ "progressPct": 42 })),
                    ),
                ],
            )
            .await
            .unwrap();

        // "view" side effect: last_opened_at set on the reads row (is_read untouched).
        store.mark_user_episode_opened(&user, &ep.id).await.unwrap();
        let opened = sqlx::query(
            "SELECT is_read, last_opened_at IS NOT NULL AS has_opened
             FROM user_episode_reads WHERE user_id = $1 AND episode_id = $2",
        )
        .bind(&user)
        .bind(&ep.id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(opened.get::<bool, _>("has_opened"));
        assert!(opened.get::<bool, _>("is_read"));

        let cnt = sqlx::query("SELECT COUNT(*) AS c FROM user_events WHERE user_id = $1")
            .bind(&user)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(cnt.get::<i64, _>("c"), 2);

        store.delete_episode(&ep.id).await.unwrap();
    }
}
