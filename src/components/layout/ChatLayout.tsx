import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useChat } from "@/hooks/useChat";
import { ChatMain } from "@/components/chat/ChatMain";
import { ConversationSidebar } from "@/components/sidebar/ConversationSidebar";
import { SettingsPanel } from "@/components/settings/SettingsPanel";

/** Subset of `settings_get` for the main-window provider hint (no secrets). */
type SettingsForHint = {
  selectedProvider: string;
  hasOpenaiApiKey: boolean;
  hasAnthropicApiKey: boolean;
  hasOllamaApiKey: boolean;
};

function truncate(s: string, max: number): string {
  const t = s.trim().replace(/\s+/g, " ");
  if (t.length <= max) return t;
  return `${t.slice(0, max - 1)}…`;
}

export function ChatLayout() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [backendHint, setBackendHint] = useState<string | null>(null);
  const prevSettingsOpen = useRef(false);

  const loadBackendHint = useCallback(async () => {
    try {
      const s = await invoke<SettingsForHint>("settings_get");
      const p = (s.selectedProvider ?? "placeholder").trim().toLowerCase();
      if (p === "placeholder") {
        setBackendHint(
          "This install is using the offline placeholder model — nothing is sent to OpenAI, Anthropic, or Ollama. Open Settings → Provider and pick a live backend (and API key if required). Settings live in your Nova data folder, not the git repo, so each computer starts with its own copy.",
        );
        return;
      }
      if (p === "openai" && !s.hasOpenaiApiKey) {
        setBackendHint(
          "OpenAI is selected but no API key is stored on this machine. Add a key under Settings → OpenAI.",
        );
        return;
      }
      if (p === "anthropic" && !s.hasAnthropicApiKey) {
        setBackendHint(
          "Anthropic is selected but no API key is stored on this machine. Add a key under Settings → Anthropic.",
        );
        return;
      }
      if (p === "ollama_cloud" && !s.hasOllamaApiKey) {
        setBackendHint(
          "Ollama Cloud is selected but no API key is stored. Add a key under Settings, or switch to local Ollama.",
        );
        return;
      }
      setBackendHint(null);
    } catch {
      setBackendHint(null);
    }
  }, []);

  useEffect(() => {
    void loadBackendHint();
  }, [loadBackendHint]);

  useEffect(() => {
    if (prevSettingsOpen.current && !settingsOpen) {
      void loadBackendHint();
    }
    prevSettingsOpen.current = settingsOpen;
  }, [settingsOpen, loadBackendHint]);

  const {
    conversations,
    activeConversationId,
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
    applyActivePersonality,
    activePersonalityId,
    activeCompanionLabel,
    companionOptions,
  } = useChat();

  const title = useMemo(() => {
    if (!activeConversationId) return "Nova";
    return (
      conversations.find((c) => c.id === activeConversationId)?.title ?? "Chat"
    );
  }, [activeConversationId, conversations]);

  const subtitle = threadLoading
    ? "Loading context from MemoryAnchor…"
    : truncate(briefing, 120) || "Local SQLite · private by default";

  return (
    <div className="flex h-full w-full overflow-hidden">
      <ConversationSidebar
        conversations={conversations}
        activeId={activeConversationId}
        onSelect={selectConversation}
        onNewChat={() => void startNewConversation()}
        onRename={(id, title) => void renameConversation(id, title)}
        onDelete={(id) => void deleteConversation(id)}
        listLoading={listLoading}
        briefing={briefing}
        briefingLoading={threadLoading && !!activeConversationId}
        anchors={anchors}
        onExtractAnchors={() => void extractAnchorsFromChat()}
      />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {backendHint ? (
          <div
            role="status"
            className="shrink-0 border-b border-sky-800/50 bg-sky-950/50 px-4 py-2 text-xs leading-relaxed text-sky-100/95"
          >
            {backendHint}
          </div>
        ) : null}
        <ChatMain
          title={title}
          subtitle={subtitle}
          hasActiveConversation={activeConversationId != null}
          messages={messages}
          threadLoading={threadLoading}
          sending={sending}
          streamAssistant={streamAssistant}
          error={error}
          settingsOpen={settingsOpen}
          onToggleSettings={() => setSettingsOpen((v) => !v)}
          onSendMessage={(text) => void sendMessage(text)}
          activeCompanionProfileId={activePersonalityId}
          activeCompanionLabel={activeCompanionLabel}
          companionOptions={companionOptions}
          onCompanionChange={async (profileId) => {
            await applyActivePersonality(profileId);
          }}
        />
      </div>
      <SettingsPanel
        open={settingsOpen}
        chatActiveProfileId={activePersonalityId}
        onCompanionActiveProfileChange={(profileId) =>
          void applyActivePersonality(profileId)
        }
      />
    </div>
  );
}
