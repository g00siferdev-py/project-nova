import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ChatMessage,
  ChatSendResult,
  StoredAnchor,
  StoredConversation,
} from "@/types/chat";
import { storedToChatMessage } from "@/types/chat";
import {
  memoryCreateConversation,
  memoryDeleteConversation,
  memoryExtractAnchorsFromConversation,
  memoryGetRecent,
  memoryListAnchors,
  memoryListConversations,
  memoryRenameConversation,
  memorySetActivePersonality,
  memoryStartupBriefing,
} from "@/hooks/useNovaMemory";
import type { PersonalityFile } from "@/lib/personalityPrompt";

const RECENT_LIMIT = 200;

type PersonalityGetResponse = {
  file: PersonalityFile;
  generatedSystemPrompt: string;
};

function companionDisplayName(file: PersonalityFile | null, profileId: string): string {
  if (!file?.profiles?.length) return "Nova";
  const p = file.profiles.find((x) => x.id === profileId);
  if (!p) return "Nova";
  const n = p.companionName.trim();
  return n.length > 0 ? n : "Nova";
}

type ChatStreamStart = { conversationId: string };
type ChatStreamEvent = { conversationId: string; delta: string; done: boolean };

export type StreamAssistantState = {
  thinking: boolean;
  text: string;
} | null;

export function useChat() {
  const [conversations, setConversations] = useState<StoredConversation[]>([]);
  const [activeConversationId, setActiveConversationId] = useState<string | null>(
    null,
  );
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [briefing, setBriefing] = useState<string>("");
  const [anchors, setAnchors] = useState<StoredAnchor[]>([]);
  const [listLoading, setListLoading] = useState(true);
  const [threadLoading, setThreadLoading] = useState(false);
  const [sending, setSending] = useState(false);
  const [streamAssistant, setStreamAssistant] = useState<StreamAssistantState>(null);
  const [error, setError] = useState<string | null>(null);
  /** Companion profile id — MemoryAnchor is scoped to this for chats, recall, and threads. */
  const [activePersonalityId, setActivePersonalityId] = useState("default");
  /** Last `personality_get` snapshot — used for companion labels and header dropdown. */
  const [personalityFile, setPersonalityFile] = useState<PersonalityFile | null>(null);

  const loadSeq = useRef(0);
  const activeConversationIdRef = useRef<string | null>(null);
  /** Mirrors `activePersonalityId` for invoke payloads (always read right before IPC). */
  const activePersonalityIdRef = useRef(activePersonalityId);

  useEffect(() => {
    activeConversationIdRef.current = activeConversationId;
  }, [activeConversationId]);

  useEffect(() => {
    activePersonalityIdRef.current = activePersonalityId;
  }, [activePersonalityId]);

  const refreshConversations = useCallback(async () => {
    try {
      const list = await memoryListConversations();
      setConversations(list);
      setError(null);
      return list;
    } catch (e) {
      const msg =
        e instanceof Error
          ? e.message
          : "Could not load conversations. Run the desktop app with: npm run tauri dev (browser-only preview has no Rust backend).";
      setError(msg);
      return [];
    }
  }, []);

  const loadActiveThread = useCallback(async (conversationId: string) => {
    const seq = ++loadSeq.current;
    setThreadLoading(true);
    setError(null);
    try {
      const [brief, recent, anchorList] = await Promise.all([
        memoryStartupBriefing(conversationId),
        memoryGetRecent(conversationId, RECENT_LIMIT),
        memoryListAnchors(conversationId, 48),
      ]);
      if (seq !== loadSeq.current) return;
      setBriefing(brief);
      setAnchors(anchorList);
      setMessages(recent.map(storedToChatMessage));
    } catch (e) {
      if (seq !== loadSeq.current) return;
      const msg =
        e instanceof Error
          ? e.message
          : "Could not load chat history. Use npm run tauri dev for the full app.";
      setError(msg);
      setBriefing("");
      setAnchors([]);
      setMessages([]);
    } finally {
      // Always clear: a newer `loadSeq` (e.g. from an in-flight send) may have invalidated this load
      // while it still held threadLoading true.
      setThreadLoading(false);
    }
  }, []);

  /** Briefing + anchors only (e.g. after send) — does not replace `messages` or toggle thread loading. */
  const refreshSidebarContext = useCallback(async (conversationId: string) => {
    try {
      const [brief, anchorList] = await Promise.all([
        memoryStartupBriefing(conversationId),
        memoryListAnchors(conversationId, 48),
      ]);
      if (conversationId !== activeConversationIdRef.current) return;
      setBriefing(brief);
      setAnchors(anchorList);
    } catch {
      /* non-fatal: chat bubbles already updated locally */
    }
  }, []);

  const refreshPersonalityFile = useCallback(async () => {
    try {
      const snap = await invoke<PersonalityGetResponse>("personality_get");
      setPersonalityFile(snap.file);
    } catch {
      /* browser / no backend */
    }
  }, []);

  const applyActivePersonality = useCallback(
    async (personalityId: string) => {
      const id = personalityId.trim() || "default";
      try {
        console.info("[nova-chat] applyActivePersonality: awaiting memory_set_active_personality", {
          personalityId: id,
        });
        await memorySetActivePersonality(id);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        setError(`Could not activate companion for memory: ${msg}`);
        return [];
      }
      activePersonalityIdRef.current = id;
      loadSeq.current += 1;
      setActivePersonalityId(id);
      const list = await refreshConversations();
      setActiveConversationId((prev) => {
        if (prev && list.some((c) => c.id === prev)) return prev;
        return list[0]?.id ?? null;
      });
      await refreshPersonalityFile();
      console.info("[nova-chat] applyActivePersonality: memory + UI active personality_id", {
        personalityId: id,
      });
      return list;
    },
    [refreshConversations, refreshPersonalityFile],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setListLoading(true);
      setError(null);
      try {
        const snap = await invoke<PersonalityGetResponse>("personality_get");
        if (cancelled) return;
        setPersonalityFile(snap.file);
        const pid = snap.file.activeProfileId.trim() || "default";
        try {
          console.info("[nova-chat] bootstrap: awaiting memory_set_active_personality", {
            personalityId: pid,
          });
          await memorySetActivePersonality(pid);
        } catch {
          /* browser preview */
        }
        activePersonalityIdRef.current = pid;
        setActivePersonalityId(pid);
        const list = await refreshConversations();
        if (cancelled) return;
        setListLoading(false);
        if (list.length === 0) {
          setActiveConversationId(null);
          return;
        }
        setActiveConversationId((prev) => {
          if (prev && list.some((c) => c.id === prev)) return prev;
          return list[0]?.id ?? null;
        });
      } catch (e) {
        if (cancelled) return;
        const msg =
          e instanceof Error
            ? e.message
            : "Could not load conversations. Run the desktop app with: npm run tauri dev (browser-only preview has no Rust backend).";
        setError(msg);
        setListLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refreshConversations]);

  useEffect(() => {
    if (!activeConversationId) {
      setBriefing("");
      setAnchors([]);
      setMessages([]);
      return;
    }
    // Clear immediately so we never show the previous thread's bubbles while the new id loads
    // (avoids appending a send onto the wrong transcript).
    setBriefing("");
    setAnchors([]);
    setMessages([]);
    void loadActiveThread(activeConversationId);
  }, [activeConversationId, loadActiveThread]);

  const selectConversation = useCallback((id: string) => {
    setActiveConversationId(id);
  }, []);

  const startNewConversation = useCallback(async () => {
    setError(null);
    const pid = activePersonalityIdRef.current.trim() || activePersonalityId.trim() || "default";
    try {
      console.info("[nova-chat] startNewConversation: awaiting memory_set_active_personality before create", {
        personalityId: pid,
      });
      await memorySetActivePersonality(pid);
      activePersonalityIdRef.current = pid;
      setActivePersonalityId(pid);
      const label = companionDisplayName(personalityFile, pid);
      const title = `New Chat with ${label}`;
      console.info("[nova-chat] startNewConversation: creating conversation", { personalityId: pid, title });
      const id = await memoryCreateConversation(title);
      await refreshConversations();
      setActiveConversationId(id);
    } catch (e) {
      const msg =
        e instanceof Error ? e.message : "Could not create conversation (run in Tauri?)";
      setError(msg);
    }
  }, [activePersonalityId, personalityFile, refreshConversations]);

  const companionOptions = useMemo(() => {
    const base =
      personalityFile?.profiles?.map((p) => ({
        id: p.id,
        companionName: (p.companionName || "").trim() || "Nova",
        profileName: (p.profileName || "").trim() || p.id,
      })) ?? [];
    if (base.length === 0) {
      return [
        {
          id: activePersonalityId,
          companionName: companionDisplayName(personalityFile, activePersonalityId),
          profileName: "Default",
        },
      ];
    }
    if (!base.some((o) => o.id === activePersonalityId)) {
      return [
        ...base,
        {
          id: activePersonalityId,
          companionName: companionDisplayName(personalityFile, activePersonalityId),
          profileName: "Active",
        },
      ];
    }
    return base;
  }, [personalityFile, activePersonalityId]);

  const activeCompanionLabel = useMemo(
    () => companionDisplayName(personalityFile, activePersonalityId),
    [personalityFile, activePersonalityId],
  );

  const renameConversation = useCallback(
    async (conversationId: string, title: string) => {
      const trimmed = title.trim();
      if (!trimmed) return;
      setError(null);
      try {
        await memoryRenameConversation(conversationId, trimmed);
        setConversations((prev) =>
          prev.map((c) =>
            c.id === conversationId ? { ...c, title: trimmed } : c,
          ),
        );
        await refreshConversations();
      } catch (e) {
        const msg =
          e instanceof Error ? e.message : "Could not rename conversation (run in Tauri?)";
        setError(msg);
      }
    },
    [refreshConversations],
  );

  const deleteConversation = useCallback(
    async (conversationId: string) => {
      setError(null);
      try {
        await memoryDeleteConversation(conversationId);
        setConversations((prev) => prev.filter((c) => c.id !== conversationId));
        const list = await refreshConversations();
        setActiveConversationId((prev) => {
          if (prev !== conversationId) return prev;
          if (list.length === 0) return null;
          return list[0]?.id ?? null;
        });
      } catch (e) {
        const msg =
          e instanceof Error ? e.message : "Could not delete conversation (run in Tauri?)";
        setError(msg);
        await refreshConversations();
      }
    },
    [refreshConversations],
  );

  const extractAnchorsFromChat = useCallback(async () => {
    if (!activeConversationId) return;
    setError(null);
    try {
      await memoryExtractAnchorsFromConversation(activeConversationId, 12);
      await loadActiveThread(activeConversationId);
      await refreshConversations();
    } catch (e) {
      const msg =
        e instanceof Error ? e.message : "Could not extract anchors (run in Tauri?)";
      setError(msg);
    }
  }, [activeConversationId, loadActiveThread, refreshConversations]);

  const sendMessage = useCallback(
    async (text: string) => {
      const trimmed = text.trim();
      const convId = activeConversationId;
      if (!trimmed || sending) return;
      if (!convId) {
        setError(
          'No conversation is open. Click "New chat" in the sidebar (or restore your app data folder), then try again.',
        );
        return;
      }

      const tempUserId = `local-${Date.now()}`;
      setMessages((prev) => [...prev, { id: tempUserId, role: "user", content: trimmed }]);
      setSending(true);
      setStreamAssistant(null);
      setError(null);

      const unlisteners: Array<() => void> = [];

      try {
        // Invalidate any `loadActiveThread` still awaiting IPC for this (often new) thread. Without
        // this, that load can finish with an empty `get_recent` while `chat_send_message` is still
        // running and then `setMessages([])` wipes the optimistic transcript ("chat disappeared").
        loadSeq.current += 1;

        unlisteners.push(
          await listen<ChatStreamStart>("chat:stream-start", (event) => {
            if (event.payload.conversationId !== activeConversationIdRef.current) return;
            setStreamAssistant({ thinking: true, text: "" });
          }),
        );

        unlisteners.push(
          await listen<ChatStreamEvent>("chat:stream", (event) => {
            if (event.payload.conversationId !== activeConversationIdRef.current) return;
            const { delta, done } = event.payload;
            if (done) {
              setStreamAssistant(null);
              return;
            }
            if (delta) {
              setStreamAssistant((prev) => ({
                thinking: false,
                text: (prev?.text ?? "") + delta,
              }));
            }
          }),
        );

        unlisteners.push(
          await listen<string>("chat:stream-error", (event) => {
            if (convId !== activeConversationIdRef.current) return;
            setError(event.payload);
            setStreamAssistant(null);
          }),
        );

        const personalityIdForSend =
          activePersonalityIdRef.current.trim() || activePersonalityId.trim() || "default";
        console.info("[nova-chat] chat_send_message invoke", {
          personalityId: personalityIdForSend,
          conversationId: convId,
        });
        const result = await invoke<ChatSendResult>("chat_send_message", {
          conversationId: convId,
          message: trimmed,
          personalityId: personalityIdForSend,
        });

        const assistantId = `local-a-${Date.now()}`;
        setMessages((prev) => [
          ...prev,
          { id: assistantId, role: "assistant", content: result.reply },
        ]);

        void refreshSidebarContext(convId);
        await refreshConversations();
      } catch (e) {
        const msg =
          e instanceof Error
            ? e.message
            : "Could not send message. Use npm run tauri dev (invoke + streaming require the Tauri shell).";
        setError(msg);
        await loadActiveThread(convId);
        await refreshConversations();
      } finally {
        for (const u of unlisteners) {
          try {
            u();
          } catch {
            /* ignore */
          }
        }
        setStreamAssistant(null);
        setSending(false);
      }
    },
    [
      activeConversationId,
      activePersonalityId,
      sending,
      loadActiveThread,
      refreshConversations,
      refreshSidebarContext,
    ],
  );

  return {
    conversations,
    activeConversationId,
    activePersonalityId,
    activeCompanionLabel,
    companionOptions,
    messages,
    briefing,
    anchors,
    listLoading,
    threadLoading,
    sending,
    streamAssistant,
    error,
    selectConversation,
    startNewConversation,
    renameConversation,
    deleteConversation,
    extractAnchorsFromChat,
    sendMessage,
    refreshConversations,
    applyActivePersonality,
  };
}
