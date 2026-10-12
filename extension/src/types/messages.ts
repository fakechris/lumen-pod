export interface LookupRequest {
  type: "LOOKUP_VIDEO";
  videoId: string;
  url: string;
}

export interface LookupResponse {
  type: "LOOKUP_RESULT";
  found: boolean;
  episodeId?: string;
  title?: string;
}

export interface GetEpisodeRequest {
  type: "GET_EPISODE";
  episodeId: string;
}

export interface GetEpisodeResponse {
  type: "EPISODE_DATA";
  episode: Episode;
}

export interface ErrorResponse {
  type: "ERROR";
  error: string;
}

export interface TabUpdateMessage {
  type: "TAB_URL_CHANGED";
  videoId: string | null;
  url: string;
}

export type RequestMessage = LookupRequest | GetEpisodeRequest | TabUpdateMessage;
export type ResponseMessage = LookupResponse | GetEpisodeResponse | ErrorResponse;

// Re-export episode types for the extension
export interface Episode {
  id: string;
  source: Source;
  processing: Processing;
  summary?: Summary;
  chapters: Chapter[];
  mindmap?: Mindmap;
  transcript?: Transcript;
  keywords: Keyword[];
  highlights: Highlight[];
  qas: QA[];
  article?: Article;
  shownotes?: string;
  isRead: boolean;
  tags: string[];
  createdAt: string;
  updatedAt: string;
}

export interface Source {
  url: string;
  platform: string;
  title: string;
  author: string;
  publishedAt?: string;
  thumbnailUrl?: string;
  duration: number;
  feedUrl?: string;
  description?: string;
  podcastName?: string;
  episodeCoverUrl?: string;
  language?: string;
  audioPath?: string;
}

export interface Processing {
  status: "pending" | "processing" | "done" | "error";
  asrEngine?: string;
  llmModel?: string;
  processedAt?: string;
  error?: string;
}

export interface Summary {
  oneLiner: string;
  abstract: string;
  keyTakeaways: string[];
  thinkingQuestions: string[];
  glossary: GlossaryTerm[];
}

export interface GlossaryTerm {
  term: string;
  definition: string;
}

export interface Keyword {
  key: string;
  desc?: string;
}

export interface Chapter {
  title: string;
  startTime: number;
  endTime: number;
  summary: string;
  hasAds: boolean;
  deepRead?: string;
}

export interface Mindmap {
  markdown: string;
  opml?: string;
}

export interface Transcript {
  paragraphs: Paragraph[];
  translations: Record<string, Record<string, { text: string }>>;
}

export interface Paragraph {
  id: number;
  speaker?: string;
  sentences: Sentence[];
}

export interface Sentence {
  id: string;
  text: string;
  words: Word[];
}

export interface Word {
  id: string;
  text: string;
  start: number;
  end: number;
}

export interface QA {
  question: string;
  answer: string;
}

export interface Clip {
  id: string;
  title: string;
  content: string;
  takeaways: string[];
}

export interface Highlight {
  time: number;
  content: string;
  note?: string;
}

export interface Article {
  title: string;
  content: string;
  format?: string;
}
