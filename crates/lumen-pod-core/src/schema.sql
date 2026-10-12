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
    search_vector TSVECTOR,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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

CREATE INDEX IF NOT EXISTS idx_episodes_source_url ON episodes(source_url);
CREATE INDEX IF NOT EXISTS idx_episodes_created_at ON episodes(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_episodes_search ON episodes USING GIN(search_vector);
CREATE INDEX IF NOT EXISTS idx_episodes_collection ON episodes(collection_id);
CREATE INDEX IF NOT EXISTS idx_chat_episode ON chat_messages(episode_id, created_at);
CREATE INDEX IF NOT EXISTS idx_invites_email ON invites(email);
