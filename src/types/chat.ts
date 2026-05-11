/** Thread row from `memory_list_conversations` / `memory_get_conversation`. */
export interface StoredConversation {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
}

/** Long-term Memory Anchor (raw or curated layer). */
export interface StoredAnchor {
  id: string;
  conversationId: string | null;
  anchorType: string;
  content: string;
  importance: number;
  hasEmbedding: boolean;
  createdAt: string;
}

/** Project row from `memory_list_projects`. */
export interface StoredProject {
  id: string;
  title: string;
  description: string;
  status: string;
  createdAt: string;
}

/** Message row from `memory_get_recent` (camelCase from Rust serde). */
export interface StoredMessage {
  id: number;
  role: "user" | "assistant";
  content: string;
  createdAt: string;
  /** Present when returned from cross-thread `memory_recall`. */
  conversationId?: string;
  conversationTitle?: string;
}

/** Hybrid recall from `memory_recall` (anchors + scoped messages). */
export interface MemoryRecallBundle {
  anchors: StoredAnchor[];
  messages: StoredMessage[];
}

/** UI message (stable React keys). */
export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
}

/** Result of `chat_send_message` (camelCase from Rust). */
export interface ChatSendResult {
  reply: string;
  toolCalls: unknown[];
  providerId: string;
  modelId: string;
}

/** @deprecated Legacy sidebar type — memory panel now shows startup briefing text. */
export interface MemoryPin {
  id: string;
  text: string;
}

export function storedToChatMessage(m: StoredMessage): ChatMessage {
  return {
    id: String(m.id),
    role: m.role,
    content: m.content,
  };
}
