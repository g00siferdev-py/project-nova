import { useEffect, useRef, useState } from "react";
import {
  Anchor,
  Brain,
  ListRestart,
  ListX,
  Loader2,
  MessageSquare,
  PenLine,
  Plus,
  Search,
  Sparkles,
  Trash2,
} from "lucide-react";
import type { MemoryRecallBundle, StoredAnchor, StoredConversation } from "@/types/chat";
import { memoryRecall } from "@/hooks/useNovaMemory";

type Props = {
  conversations: StoredConversation[];
  /** Whether SQLite still has threads (for copy + restore when list is hidden). */
  hasThreadsInDatabase: boolean;
  /** UI-only: sidebar list cleared; database unchanged. */
  threadListHiddenFromSidebar: boolean;
  onClearThreadListFromView: () => void;
  onRestoreThreadListFromView: () => void;
  activeId: string | null;
  onSelect: (id: string) => void;
  onNewChat: () => void;
  onRename: (id: string, title: string) => void;
  onDelete: (id: string) => void;
  listLoading: boolean;
  briefing: string;
  briefingLoading: boolean;
  anchors: StoredAnchor[];
  onExtractAnchors: () => void;
};

function formatUpdated(iso: string): string {
  const d = Date.parse(iso);
  if (Number.isNaN(d)) return iso;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(d));
}

export function ConversationSidebar({
  conversations,
  hasThreadsInDatabase,
  threadListHiddenFromSidebar,
  onClearThreadListFromView,
  onRestoreThreadListFromView,
  activeId,
  onSelect,
  onNewChat,
  onRename,
  onDelete,
  listLoading,
  briefing,
  briefingLoading,
  anchors,
  onExtractAnchors,
}: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);

  const [recallQuery, setRecallQuery] = useState("");
  const [recallBusy, setRecallBusy] = useState(false);
  const [recallBundle, setRecallBundle] = useState<MemoryRecallBundle | null>(null);
  const [recallError, setRecallError] = useState<string | null>(null);

  const recentAnchorsByDate = [...anchors].sort(
    (a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt),
  );

  useEffect(() => {
    if (editingId) inputRef.current?.focus();
  }, [editingId]);

  useEffect(() => {
    setRecallBundle(null);
    setRecallError(null);
    setRecallQuery("");
  }, [activeId]);

  const startEditMouseDown = (c: StoredConversation, e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setEditingId(c.id);
    setEditValue(c.title);
  };

  const commitRename = (id: string) => {
    const t = editValue.trim();
    if (t) onRename(id, t);
    setEditingId(null);
    setEditValue("");
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditValue("");
  };

  const runRecall = async () => {
    const q = recallQuery.trim();
    if (!q) {
      setRecallBundle(null);
      setRecallError(null);
      return;
    }
    const scope = activeId && activeId.length > 0 ? activeId : null;
    setRecallBusy(true);
    setRecallError(null);
    try {
      const bundle = await memoryRecall(q, scope, 16, 8);
      setRecallBundle(bundle);
    } catch (e) {
      setRecallBundle(null);
      setRecallError(e instanceof Error ? e.message : String(e));
    } finally {
      setRecallBusy(false);
    }
  };

  return (
    <aside className="flex h-full min-h-0 w-80 shrink-0 flex-col overflow-hidden border-r border-slate-800/80 bg-slate-900/40">
      <div className="flex items-center gap-2 border-b border-slate-800/80 px-4 py-3">
        <img
          src="/nova-icon.svg"
          alt=""
          className="size-9 rounded-lg ring-1 ring-slate-700/80"
        />
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold tracking-tight text-white">
            Nova
          </p>
          <p className="truncate text-xs text-slate-500">Companion</p>
        </div>
      </div>

      <div className="p-3">
        <button
          type="button"
          onClick={() => onNewChat()}
          className="flex w-full items-center justify-center gap-2 rounded-lg bg-indigo-500 px-3 py-2 text-sm font-medium text-white shadow-sm shadow-indigo-500/20 transition hover:bg-indigo-400 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
        >
          <Plus className="size-4" aria-hidden />
          New chat
        </button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col gap-1 px-2 pb-2">
        <div className="flex items-center justify-between gap-2 px-2 pb-1 pt-2 text-[11px] font-semibold uppercase tracking-wider text-slate-500">
          <span className="flex items-center gap-2">
            <MessageSquare className="size-3.5" aria-hidden />
            Conversations
          </span>
          {!threadListHiddenFromSidebar && hasThreadsInDatabase ? (
            <button
              type="button"
              title="Hide thread list from this sidebar only — does not delete SQLite data"
              onClick={() => onClearThreadListFromView()}
              className="inline-flex shrink-0 items-center gap-1 rounded-md border border-slate-700/80 bg-slate-900/60 px-2 py-1 text-[10px] font-medium normal-case tracking-normal text-slate-400 transition hover:border-slate-600 hover:bg-slate-800/80 hover:text-slate-200"
            >
              <ListX className="size-3.5" aria-hidden />
              Clear view
            </button>
          ) : null}
        </div>
        <nav className="max-h-[28vh] min-h-0 space-y-0.5 overflow-y-auto pr-1">
          {listLoading ? (
            <div className="flex items-center justify-center gap-2 py-8 text-xs text-slate-500">
              <Loader2 className="size-4 animate-spin text-indigo-400" aria-hidden />
              Loading…
            </div>
          ) : threadListHiddenFromSidebar && hasThreadsInDatabase ? (
            <div className="space-y-3 px-2 py-4">
              <p className="text-center text-xs leading-relaxed text-slate-400">
                Thread list is hidden from this panel only. Nothing was removed from your database.
              </p>
              <button
                type="button"
                onClick={() => onRestoreThreadListFromView()}
                className="flex w-full items-center justify-center gap-2 rounded-lg border border-slate-600 bg-slate-800/60 px-3 py-2 text-xs font-medium text-slate-200 transition hover:bg-slate-800"
              >
                <ListRestart className="size-3.5" aria-hidden />
                Show threads from database
              </button>
            </div>
          ) : conversations.length === 0 ? (
            <p className="px-2 py-4 text-center text-xs text-slate-500">
              No conversations yet. Start one with New chat.
            </p>
          ) : (
            conversations.map((c) => {
              const active = c.id === activeId;
              const editing = editingId === c.id;
              return (
                <div
                  key={c.id}
                  className={
                    active
                      ? "flex items-start gap-1 rounded-lg bg-slate-800/90 px-2 py-2 ring-1 ring-indigo-500/40"
                      : "flex items-start gap-1 rounded-lg px-2 py-2 transition hover:bg-slate-800/60"
                  }
                >
                  {editing ? (
                    <input
                      ref={inputRef}
                      value={editValue}
                      onChange={(e) => setEditValue(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          commitRename(c.id);
                        }
                        if (e.key === "Escape") cancelEdit();
                      }}
                      onBlur={(e) => {
                        const next = e.relatedTarget as Node | null;
                        if (next && e.currentTarget.parentElement?.contains(next)) return;
                        commitRename(c.id);
                      }}
                      className="min-w-0 flex-1 rounded border border-slate-600 bg-slate-950 px-2 py-1 text-sm text-white outline-none focus:ring-1 focus:ring-indigo-500"
                    />
                  ) : (
                    <>
                      <button
                        type="button"
                        onClick={() => onSelect(c.id)}
                        className="min-w-0 flex-1 text-left"
                      >
                        <span className="block truncate text-sm font-medium text-white">
                          {c.title}
                        </span>
                        <span
                          className="block text-xs text-slate-500"
                          title={`Created ${formatUpdated(c.createdAt)}`}
                        >
                          {formatUpdated(c.updatedAt)}
                        </span>
                      </button>
                      <button
                        type="button"
                        aria-label={`Rename ${c.title}`}
                        onMouseDown={(e) => startEditMouseDown(c, e)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            setEditingId(c.id);
                            setEditValue(c.title);
                          }
                        }}
                        className="shrink-0 rounded p-1 text-slate-500 transition hover:bg-slate-700/80 hover:text-slate-300"
                      >
                        <PenLine className="size-3.5" aria-hidden />
                      </button>
                      <button
                        type="button"
                        aria-label={`Delete ${c.title}`}
                        onClick={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          setEditingId(null);
                          setEditValue("");
                          onDelete(c.id);
                        }}
                        className="shrink-0 rounded p-1 text-slate-500 transition hover:bg-red-950/60 hover:text-red-300"
                      >
                        <Trash2 className="size-3.5" aria-hidden />
                      </button>
                    </>
                  )}
                </div>
              );
            })
          )}
        </nav>

        <div className="mt-1 min-h-0 flex-1 space-y-3 border-t border-slate-800/80 pt-3">
          <div className="flex items-center gap-2 px-2 text-[11px] font-semibold uppercase tracking-wider text-slate-500">
            <Brain className="size-3.5" aria-hidden />
            Memory Anchor
          </div>

          <div className="space-y-2 px-1">
            <div className="space-y-1 px-0.5 text-[10px] leading-snug text-slate-500">
              <p className="flex items-center gap-1.5">
                <Sparkles className="size-3 shrink-0 text-indigo-400" aria-hidden />
                <span>Raw + curated layers · local only</span>
              </p>
              <p>
                Chat messages live in the main transcript (SQLite). <strong className="text-slate-400">Recent anchors</strong>{" "}
                lists extracted snippets only — use <strong className="text-slate-400">Extract raw anchors</strong> or recall
                search below; they are not auto-filled from every reply.
              </p>
            </div>

            <div className="max-h-36 overflow-y-auto rounded-lg border border-slate-800/80 bg-slate-950/40 px-2.5 py-2">
              {briefingLoading ? (
                <div className="flex items-center gap-2 py-4 text-xs text-slate-500">
                  <Loader2 className="size-4 animate-spin text-indigo-400" aria-hidden />
                  Loading briefing…
                </div>
              ) : (
                <pre className="whitespace-pre-wrap break-words font-sans text-[11px] leading-relaxed text-slate-400">
                  {briefing.trim() || "Open a chat to load the enriched startup briefing."}
                </pre>
              )}
            </div>

            <div className="flex gap-2">
              <button
                type="button"
                disabled={!activeId || briefingLoading}
                onClick={() => onExtractAnchors()}
                className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-slate-700/80 bg-slate-800/50 px-2 py-1.5 text-[11px] font-medium text-slate-200 transition hover:bg-slate-800 disabled:opacity-40"
              >
                <Anchor className="size-3.5" aria-hidden />
                Extract raw anchors
              </button>
            </div>

            <div>
              <p className="mb-1 px-0.5 text-[10px] font-semibold uppercase tracking-wide text-slate-600">
                Recent anchors
              </p>
              <ul className="max-h-28 space-y-1 overflow-y-auto">
                {anchors.length === 0 ? (
                  <li className="px-1 text-[11px] text-slate-600">No anchors for this thread.</li>
                ) : (
                  recentAnchorsByDate.slice(0, 10).map((a) => (
                    <li
                      key={a.id}
                      className="rounded border border-slate-800/60 bg-slate-950/30 px-2 py-1 text-[11px] text-slate-400"
                    >
                      <span className="mr-1 text-[9px] text-slate-600">
                        {new Intl.DateTimeFormat(undefined, { dateStyle: "short" }).format(
                          new Date(a.createdAt),
                        )}
                      </span>
                      <span className="mr-1 rounded bg-slate-800 px-1 text-[9px] uppercase text-indigo-300">
                        {a.anchorType}
                      </span>
                      <span className="text-slate-500">·{a.importance}</span>
                      <span className="mt-0.5 block text-slate-300">{a.content}</span>
                    </li>
                  ))
                )}
              </ul>
            </div>

            <div>
              <p className="mb-1 px-0.5 text-[10px] font-semibold uppercase tracking-wide text-slate-600">
                Hybrid recall (FTS + keywords)
              </p>
              <div className="flex gap-1">
                <input
                  value={recallQuery}
                  onChange={(e) => setRecallQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void runRecall();
                  }}
                  placeholder="Search anchors & messages…"
                  className="min-w-0 flex-1 rounded-lg border border-slate-800/90 bg-slate-950/50 px-2 py-1 text-[11px] text-slate-200 placeholder:text-slate-600 outline-none focus:border-indigo-500/40"
                />
                <button
                  type="button"
                  onClick={() => void runRecall()}
                  disabled={recallBusy}
                  className="shrink-0 rounded-lg border border-slate-700/80 bg-slate-800/60 p-1.5 text-slate-300 hover:bg-slate-800"
                  aria-label="Search"
                >
                  {recallBusy ? (
                    <Loader2 className="size-3.5 animate-spin" aria-hidden />
                  ) : (
                    <Search className="size-3.5" aria-hidden />
                  )}
                </button>
              </div>
              {recallError ? (
                <p className="mt-1 text-[10px] text-amber-400/90">{recallError}</p>
              ) : null}
              {recallBundle &&
              (recallBundle.anchors.length > 0 || recallBundle.messages.length > 0) ? (
                <div className="mt-2 max-h-36 space-y-2 overflow-y-auto">
                  {recallBundle.anchors.length > 0 ? (
                    <ul className="space-y-1">
                      {recallBundle.anchors.map((a) => (
                        <li
                          key={a.id}
                          className="rounded bg-slate-900/50 px-2 py-0.5 text-[10px] text-slate-400"
                        >
                          <span className="text-indigo-400/90">{a.anchorType}</span> · {a.content}
                        </li>
                      ))}
                    </ul>
                  ) : null}
                  {recallBundle.messages.length > 0 ? (
                    <ul className="space-y-1 border-t border-slate-800/60 pt-1">
                      {recallBundle.messages.map((m) => (
                        <li
                          key={m.id}
                          className="rounded bg-slate-900/40 px-2 py-0.5 text-[10px] text-slate-500"
                        >
                          <span className="font-medium text-slate-400">{m.role}</span>:{" "}
                          {m.content.length > 160 ? `${m.content.slice(0, 160)}…` : m.content}
                        </li>
                      ))}
                    </ul>
                  ) : null}
                </div>
              ) : recallBundle &&
                recallBundle.anchors.length === 0 &&
                recallBundle.messages.length === 0 &&
                recallQuery.trim() ? (
                <p className="mt-1 text-[10px] text-slate-600">No matches.</p>
              ) : null}
            </div>
          </div>
        </div>
      </div>
    </aside>
  );
}
