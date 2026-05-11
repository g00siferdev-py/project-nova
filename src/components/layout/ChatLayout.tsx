import { useMemo, useState } from "react";
import { useChat } from "@/hooks/useChat";
import { ChatMain } from "@/components/chat/ChatMain";
import { ConversationSidebar } from "@/components/sidebar/ConversationSidebar";
import { SettingsPanel } from "@/components/settings/SettingsPanel";

function truncate(s: string, max: number): string {
  const t = s.trim().replace(/\s+/g, " ");
  if (t.length <= max) return t;
  return `${t.slice(0, max - 1)}…`;
}

export function ChatLayout() {
  const [settingsOpen, setSettingsOpen] = useState(false);
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
      <ChatMain
        title={title}
        subtitle={subtitle}
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
