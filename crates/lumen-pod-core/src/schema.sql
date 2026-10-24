-- lumen-pod PostgreSQL schema

CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL,
    source JSONB NOT NULL,
    processing JSONB NOT NULL,
    summary JSONB,
    chapters JSONB NOT NULL DEFAULT '[]'::jsonb,
    mindmap JSONB,
    transcript JSONB,
    keywords JSONB NOT NULL DEFAULT '[]'::jsonb,
    highlights JSONB NOT NULL DEFAULT '[]'::jsonb,
    qas JSONB NOT NULL DEFAULT '[]'::jsonb,
    clips JSONB NOT NULL DEFAULT '[]'::jsonb,
    article JSONB,
    shownotes TEXT,
    is_read BOOLEAN NOT NULL DEFAULT FALSE,
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    collection_id TEXT,
    owner_id TEXT,
    visibility TEXT NOT NULL DEFAULT 'public' CHECK (visibility IN ('public', 'private')),
    search_vector TSVECTOR,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_episode_reads (
    user_id TEXT NOT NULL,
    episode_id TEXT NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    is_read BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, episode_id)
);

CREATE TABLE IF NOT EXISTS collections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    cover_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    episode_id TEXT NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    name TEXT,
    avatar_url TEXT,
    role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('admin', 'member')),
    google_sub TEXT UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS invites (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    invited_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    accepted_at TIMESTAMPTZ
);

-- Migration: add approved column to users
ALTER TABLE users ADD COLUMN IF NOT EXISTS approved BOOLEAN NOT NULL DEFAULT FALSE;

-- Migration: RBAC permissions on users
ALTER TABLE users ADD COLUMN IF NOT EXISTS can_edit BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE users ADD COLUMN IF NOT EXISTS can_delete BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE users ADD COLUMN IF NOT EXISTS can_publish_private BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE users SET can_edit = TRUE, can_delete = TRUE, can_publish_private = TRUE WHERE role = 'admin';

-- Migration: episode ownership and private visibility
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS owner_id TEXT;
ALTER TABLE episodes ADD COLUMN IF NOT EXISTS visibility TEXT NOT NULL DEFAULT 'public';

-- Migration: per-user chat messages
ALTER TABLE chat_messages ADD COLUMN IF NOT EXISTS user_id TEXT NOT NULL DEFAULT '';

-- Migration: runtime-editable settings (admin panel); values override env defaults
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Migration: issued API tokens (hashed at rest; env LUMEN_POD_API_TOKEN stays valid)
CREATE TABLE IF NOT EXISTS api_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    token_hint TEXT NOT NULL,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    use_count BIGINT NOT NULL DEFAULT 0,
    revoked_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_episodes_source_url ON episodes(source_url);
CREATE INDEX IF NOT EXISTS idx_episodes_created_at ON episodes(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_episodes_search ON episodes USING GIN(search_vector);
CREATE INDEX IF NOT EXISTS idx_episodes_collection ON episodes(collection_id);
CREATE INDEX IF NOT EXISTS idx_episodes_owner_visibility ON episodes(owner_id, visibility);
CREATE INDEX IF NOT EXISTS idx_user_episode_reads_user ON user_episode_reads(user_id);
CREATE INDEX IF NOT EXISTS idx_chat_episode ON chat_messages(episode_id, created_at);
CREATE INDEX IF NOT EXISTS idx_chat_episode_user ON chat_messages(episode_id, user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_invites_email ON invites(email);
