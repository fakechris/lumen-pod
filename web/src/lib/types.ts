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
  clips: Clip[];
  article?: Article;
  shownotes?: string;
  isRead: boolean;
  tags: string[];
  collectionId?: string;
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
  agentId?: string;
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
}

export interface Mindmap {
  markdown: string;
  opml?: string;
}

export interface QA {
  question: string;
  answer: string;
  questionSpeaker?: string;
  answerSpeaker?: string;
}

export interface Clip {
  id: string;
  title: string;
  content: string;
  takeaways: string[];
  point: number;
  clipStart: number;
  clipEnd: number;
  createdAt: string;
}

export interface Highlight {
  time: number;
  content: string;
  cueId?: string;
  note?: string;
  createdAt: string;
}

export interface Article {
  title: string;
  content: string;
  format?: string;
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

export interface User {
  id: string;
  email: string;
  name?: string;
  avatarUrl?: string;
  role: string;
}
