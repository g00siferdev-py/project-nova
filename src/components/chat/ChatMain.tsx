import { useEffect, useRef, useState, type FormEvent } from "react";
import { ChevronDown, Loader2, PanelRightOpen, Send, Sparkles, Users } from "lucide-react";
import type { ChatMessage } from "@/types/chat";
import type { StreamAssistantState } from "@/hooks/useChat";

export type CompanionHeaderOption = {
  id: string;
  companionName: string;
  profileName: string;
};

type Props = {
  title: string;
  subtitle: string;
  messages: ChatMessage[];
  threadLoading: boolean;
  sending: boolean;
  streamAssistant: StreamAssistantState;
  error: string | null;
  settingsOpen: boolean;
  onToggleSettings: () => void;
  onSendMessage: (text: string) => void;
  /** Which companion profile is active for memory + new chats. */
  activeCompanionProfileId: string;
  activeCompanionLabel: string;
  companionOptions: CompanionHeaderOption[];
  /** Called when the user picks a companion; awaited from the select handler when async. */
  onCompanionChange: (profileId: string) => void | Promise<unknown>;
};

export function ChatMain({
  title,
  subtitle,
  messages,
  threadLoading,
  sending,
  streamAssistant,
  error,
  settingsOpen,
  onToggleSettings,
  onSendMessage,
  activeCompanionProfileId,
  activeCompanionLabel,
  companionOptions,
  onCompanionChange,
}: Props) {
  const endRef = useRef<HTMLDivElement | null>(null);
  const [draft, setDraft] = useState("");

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, threadLoading, streamAssistant]);

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!draft.trim() || sending || threadLoading) return;
    onSendMessage(draft);
    setDraft("");
  };

  return (
    <section className="flex min-w-0 flex-1 flex-col bg-gradient-to-b from-slate-950 via-slate-950 to-slate-900/80">
      <header className="flex shrink-0 flex-wrap items-center gap-3 border-b border-slate-800/80 px-4 py-3">
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <Sparkles className="size-4 shrink-0 text-indigo-400" aria-hidden />
          <div className="min-w-0">
            <h1 className="truncate text-sm font-semibold text-white">{title}</h1>
            <p className="truncate text-xs text-slate-500" title={subtitle}>
              {subtitle}
            </p>
          </div>
        </div>
        <div
          className="flex shrink-0 items-center gap-2 rounded-xl border border-indigo-500/35 bg-indigo-950/40 px-2.5 py-1.5 shadow-inner shadow-indigo-950/30"
          title={`Active companion: ${activeCompanionLabel}. Choose who to talk to before starting a new chat.`}
        >
          <Users className="size-4 shrink-0 text-indigo-300" aria-hidden />
          <div className="relative">
            <label htmlFor="nova-header-companion" className="sr-only">
              Companion for new chats
            </label>
            <ChevronDown
              className="pointer-events-none absolute right-2 top-1/2 size-3.5 -translate-y-1/2 text-indigo-300/80"
              aria-hidden
            />
            <select
              id="nova-header-companion"
              value={activeCompanionProfileId}
              onChange={async (e) => {
                const next = e.target.value;
                console.info("[nova-chat] companion dropdown: user selected personality_id", {
                  personalityId: next,
                  previousPersonalityId: activeCompanionProfileId,
                });
                await onCompanionChange(next);
                console.info("[nova-chat] companion dropdown: handler finished for personality_id", {
                  personalityId: next,
                });
              }}
              disabled={threadLoading}
              className="h-9 max-w-[min(18rem,calc(100vw-12rem))] min-w-[11rem] appearance-none rounded-lg border border-indigo-400/40 bg-slate-950/90 py-1.5 pl-2.5 pr-8 text-xs font-semibold text-white outline-none transition hover:border-indigo-400/60 focus-visible:border-indigo-400 focus-visible:ring-2 focus-visible:ring-indigo-500/30 disabled:opacity-50"
              title="This companion receives new chats and uses their isolated memory"
            >
              {companionOptions.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.companionName}
                  {o.profileName && o.profileName !== o.companionName
                    ? ` · ${o.profileName}`
                    : ""}
                </option>
              ))}
            </select>
          </div>
        </div>
        <button
          type="button"
          onClick={onToggleSettings}
          aria-expanded={settingsOpen}
          aria-controls="nova-settings-panel"
          className="inline-flex items-center gap-2 rounded-lg border border-slate-700/80 bg-slate-900/60 px-3 py-1.5 text-xs font-medium text-slate-200 shadow-sm transition hover:border-slate-600 hover:bg-slate-800/80 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500"
        >
          <PanelRightOpen className="size-4 text-slate-400" aria-hidden />
          {settingsOpen ? "Hide" : "Settings"}
        </button>
      </header>

      {error ? (
        <div
          role="alert"
          className="shrink-0 border-b border-amber-500/30 bg-amber-500/10 px-4 py-2 text-xs text-amber-200"
        >
          {error}
        </div>
      ) : null}

      <div className="relative min-h-0 flex-1 overflow-y-auto px-4 py-4">
        {threadLoading ? (
          <div className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-slate-950/70 backdrop-blur-[2px]">
            <Loader2
              className="size-8 animate-spin text-indigo-400"
              aria-hidden
            />
            <p className="text-sm text-slate-400">Loading history & context…</p>
          </div>
        ) : null}
        <div className="mx-auto flex max-w-3xl flex-col gap-4">
          {messages.length === 0 && !threadLoading ? (
            <p className="rounded-xl border border-dashed border-slate-800/90 bg-slate-900/30 px-4 py-8 text-center text-sm text-slate-500">
              No messages in this conversation yet. Say hello below — everything
              stays in your local SQLite store.
            </p>
          ) : (
            messages.map((m) => (
              <article
                key={m.id}
                className={
                  m.role === "user"
                    ? "ml-8 rounded-2xl rounded-br-md border border-slate-800/80 bg-slate-900/70 px-4 py-3 text-sm leading-relaxed text-slate-100 shadow-sm"
                    : "mr-8 rounded-2xl rounded-bl-md border border-indigo-500/20 bg-indigo-500/10 px-4 py-3 text-sm leading-relaxed text-slate-100 shadow-sm"
                }
              >
                <p className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-slate-500">
                  {m.role === "user" ? "You" : "Nova"}
                </p>
                <p className="whitespace-pre-wrap">{m.content}</p>
              </article>
            ))
          )}
          {streamAssistant ? (
            <article className="mr-8 rounded-2xl rounded-bl-md border border-indigo-500/30 bg-indigo-500/10 px-4 py-3 text-sm leading-relaxed text-slate-100 shadow-sm">
              <p className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-slate-500">
                Nova
              </p>
              {streamAssistant.thinking && !streamAssistant.text ? (
                <p className="flex items-center gap-2 text-slate-400">
                  <Loader2 className="size-4 shrink-0 animate-spin text-indigo-400" aria-hidden />
                  <span>Thinking…</span>
                </p>
              ) : (
                <p className="whitespace-pre-wrap text-slate-100">{streamAssistant.text}</p>
              )}
            </article>
          ) : null}
          <div ref={endRef} />
        </div>
      </div>

      <footer className="shrink-0 border-t border-slate-800/80 p-4">
        <form
          onSubmit={handleSubmit}
          className="mx-auto flex max-w-3xl gap-2"
        >
          <label className="sr-only" htmlFor="nova-composer">
            Message
          </label>
          <textarea
            id="nova-composer"
            rows={2}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                if (!draft.trim() || sending || threadLoading) return;
                onSendMessage(draft);
                setDraft("");
              }
            }}
            disabled={threadLoading || sending}
            placeholder="Message Nova…"
            className="min-h-[2.75rem] flex-1 resize-none rounded-xl border border-slate-800/90 bg-slate-900/60 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-600 shadow-inner outline-none ring-0 transition focus:border-indigo-500/50 focus:ring-2 focus:ring-indigo-500/30 disabled:opacity-50"
          />
          <button
            type="submit"
            disabled={threadLoading || sending || !draft.trim()}
            className="inline-flex shrink-0 items-center justify-center gap-2 self-end rounded-xl bg-indigo-500 px-3 py-2 text-white shadow-sm shadow-indigo-500/25 transition hover:bg-indigo-400 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 disabled:pointer-events-none disabled:opacity-40"
            aria-label="Send message"
          >
            {sending ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Send className="size-4" aria-hidden />
            )}
          </button>
        </form>
      </footer>
    </section>
  );
}
