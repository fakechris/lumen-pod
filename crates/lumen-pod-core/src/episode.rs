use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Top-level Episode — the core data unit for lumen-pod.
///
/// The `transcript` field is intentionally compatible with lumen-cut's
/// `Doc.paragraphs` + `Doc.translations` format so agent-side processing
/// can pipe lumen-cut output directly into `pod_publish`.
///
/// Feature parity targets: Podwise (summary/mindmap/transcript/clips/Q&A/
/// keywords/highlights/shownotes/takeaways/chapters/export) and BibiGPT
/// (structured summary/mindmap with clickable nodes/AI chat/visual
/// analysis/multi-model/collections/30+ platforms).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub id: String,
    pub source: Source,
    pub processing: Processing,

    // ---- AI-generated content sections (tabs in UI) ----
    pub summary: Option<Summary>,
    pub chapters: Vec<Chapter>,
    pub mindmap: Option<Mindmap>,
    pub transcript: Option<Transcript>,
    pub keywords: Vec<Keyword>,
    pub highlights: Vec<Highlight>,
    pub qas: Vec<QA>,
    pub clips: Vec<Clip>,
    pub article: Option<Article>,
    pub shownotes: Option<String>,

    // ---- User interaction & Access Control ----
    #[serde(default)]
    pub is_read: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub collection_id: Option<String>,
    /// Owner user ID who created/uploaded this episode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    /// Visibility: "public" (default) or "private" (only owner and admin can see).
    #[serde(default = "default_episode_visibility")]
    pub visibility: String,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub fn default_episode_visibility() -> String {
    "public".to_string()
}

// ---- Source metadata ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub url: String,
    pub platform: String,
    pub title: String,
    pub author: String,
    pub published_at: Option<DateTime<Utc>>,
    pub thumbnail_url: Option<String>,
    pub duration: f64,
    pub feed_url: Option<String>,
    /// Original description / show notes from the source platform.
    #[serde(default)]
    pub description: Option<String>,
    /// Podcast or channel name (distinct from episode author).
    #[serde(default)]
    pub podcast_name: Option<String>,
    /// Episode cover art (may differ from podcast cover).
    #[serde(default)]
    pub episode_cover_url: Option<String>,
    /// Source language detected by ASR.
    #[serde(default)]
    pub language: Option<String>,
    /// Audio file path (local storage, optional — user decides whether to upload).
    #[serde(default)]
    pub audio_path: Option<String>,
}

// ---- Processing state ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Processing {
    pub status: ProcessingStatus,
    pub asr_engine: Option<String>,
    /// LLM model used for summary/chapters/Q&A generation.
    #[serde(default)]
    pub llm_model: Option<String>,
    pub processed_at: Option<DateTime<Utc>>,
    pub agent_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessingStatus {
    Pending,
    Processing,
    Done,
    Error,
}

// ---- Summary (Podwise: summary + takeaways; BibiGPT: 深度总结) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// One-sentence summary.
    pub one_liner: String,
    /// 200-500 word abstract / overview.
    pub r#abstract: String,
    /// Key takeaways — top 3-7 actionable points.
    pub key_takeaways: Vec<String>,
    /// Deep thinking Q&A generated from content (BibiGPT style).
    #[serde(default)]
    pub thinking_questions: Vec<String>,
    /// Key terms / glossary with explanations (BibiGPT 术语解释).
    #[serde(default)]
    pub glossary: Vec<GlossaryTerm>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlossaryTerm {
    pub term: String,
    pub definition: String,
}

// ---- Keywords with descriptions (Podwise style) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Keyword {
    pub key: String,
    #[serde(default)]
    pub desc: Option<String>,
}

// ---- Chapters / Outlines ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub title: String,
    pub start_time: f64,
    pub end_time: f64,
    pub summary: String,
    /// Whether this chapter segment is detected as an ad (Podwise feature).
    #[serde(default)]
    pub has_ads: bool,
    /// AI-generated deep-read analysis, persisted once generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep_read: Option<String>,
}

// ---- Mind map ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mindmap {
    /// Markdown-formatted mind map (rendered with Markmap).
    pub markdown: String,
    /// Optional OPML export (XMind/MindNode compatible, BibiGPT style).
    #[serde(default)]
    pub opml: Option<String>,
}

// ---- Q&A extracted from content (Podwise + BibiGPT) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QA {
    pub question: String,
    pub answer: String,
    /// Speaker who asked (if identifiable).
    #[serde(default)]
    pub question_speaker: Option<String>,
    /// Speaker who answered (if identifiable).
    #[serde(default)]
    pub answer_speaker: Option<String>,
}

// ---- Clips — user-created key moments (Podwise feature) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    pub title: String,
    /// Excerpt text around the clip point.
    pub content: String,
    pub takeaways: Vec<String>,
    /// Playback point in seconds.
    pub point: f64,
    /// Generated excerpt boundaries.
    pub clip_start: f64,
    pub clip_end: f64,
    pub created_at: DateTime<Utc>,
}

// ---- Highlights — timestamped notable moments ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    /// Timestamp in seconds.
    pub time: f64,
    /// The highlighted content text.
    pub content: String,
    /// Optional sentence id reference for transcript linking.
    #[serde(default)]
    pub cue_id: Option<String>,
    /// User-added note.
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ---- Article — generated long-form article (YouMind v54 Read-and-Done style / BibiGPT 图文改写) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    pub title: String,
    pub content: String,
    /// Article format: "markdown", "blog", "newsletter", "xiaohongshu", "wechat".
    #[serde(default)]
    pub format: Option<String>,
    /// Target language code used when generating the article (e.g. "zh", "en").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Optional user-specified focus / angle for the article.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// ISO-8601 timestamp when the article was generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
}

// ---- Collections — group episodes by topic/playlist ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---- Transcript types (compatible with lumen-cut Doc format) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub paragraphs: Vec<Paragraph>,
    #[serde(default)]
    pub translations:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, TranslationEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paragraph {
    pub id: u32,
    pub speaker: Option<String>,
    pub sentences: Vec<Sentence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub id: String,
    pub text: String,
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub id: String,
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationEntry {
    pub text: String,
}

// ---- AI Chat history (BibiGPT 追问 / Podwise Ask AI) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub episode_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub role: ChatRole,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

// ---- Export records ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRecord {
    pub id: String,
    pub episode_id: String,
    pub format: ExportFormat,
    pub destination: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Markdown,
    Pdf,
    Srt,
    Vtt,
    Notion,
    Obsidian,
    Logseq,
    Readwise,
    Opml,
}

// ---- Auth / invite types ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: UserRole,
    pub approved: bool,
    #[serde(default)]
    pub can_edit: bool,
    #[serde(default)]
    pub can_delete: bool,
    #[serde(default)]
    pub can_publish_private: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: String,
    pub email: String,
    pub invited_by: String,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}
