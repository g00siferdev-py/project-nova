import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Cpu, FolderOpen, Heart, KeyRound, Loader2, Moon, SlidersHorizontal } from "lucide-react";
import { CompanionPersonalitySection } from "@/components/settings/CompanionPersonalitySection";

type Props = {
  open: boolean;
  /** When the user switches companion profile, refresh MemoryAnchor scope and chat threads. */
  onCompanionActiveProfileChange?: (profileId: string) => void | Promise<void>;
  /** Profile id currently used for chat memory (from `useChat`). */
  chatActiveProfileId?: string;
};

type SettingsView = {
  selectedProvider: string;
  openaiModel: string;
  openaiBaseUrl: string;
  ollamaModel: string;
  ollamaBaseUrl: string;
  anthropicModel: string;
  temperature: number;
  /** Omitted in JSON when unset (Rust `None`) — treat like `null` (model default). */
  maxTokens?: number | null;
  /** When true and the active provider supports it, the model may call built-in web search / URL fetch tools. */
  agentWebToolsEnabled: boolean;
  /** When true, the model may read/write/list files only under the app workspace folder (see data paths). */
  agentWorkspaceEnabled: boolean;
  hasOpenaiApiKey: boolean;
  hasAnthropicApiKey: boolean;
  hasOllamaApiKey: boolean;
};

type SettingsPatch = {
  selectedProvider?: string;
  openaiModel?: string;
  openaiBaseUrl?: string;
  ollamaModel?: string;
  ollamaBaseUrl?: string;
  anthropicModel?: string;
  temperature?: number;
  /** Omit = unchanged; null = clear cap */
  maxTokens?: number | null;
  agentWebToolsEnabled?: boolean;
  agentWorkspaceEnabled?: boolean;
};

type ProviderDescriptor = {
  id: string;
  label: string;
  localFirst: boolean;
  requiresApiKey: boolean;
};

const DEBOUNCE_MS = 400;

const MEMORY_WIPE_COPY = `This will permanently delete ALL conversations, messages, anchors, and memories across every personality.
Nova will forget everything it has learned about you.
Your API keys, settings, and personality profiles will be preserved.This action cannot be undone.
To proceed, type CONFIRM and click Wipe.`;

const FACTORY_RESET_COPY = `This will permanently delete ALL conversations, memories, anchors, and settings.
Nova will forget everything it has ever learned about you.
This action cannot be undone.To proceed, type CONFIRM in the box below and click Reset.`;

const TEMPERATURE_INFO =
  "Temperature controls creativity. Lower = more focused/predictable. Higher = more creative/random (0.0–2.0).";

const OLLAMA_CLOUD_KEYS_URL = "https://ollama.com/settings/keys";

const OLLAMA_CLOUD_MODEL_PLACEHOLDER = "kimi-k2.5:cloud or gpt-oss:120b-cloud";

const DEFAULT_OPENAI_MODELS = [
  "gpt-4o",
  "gpt-4o-mini",
  "gpt-4-turbo",
  "gpt-4",
  "gpt-3.5-turbo",
  "o1",
  "o1-mini",
  "o3-mini",
] as const;

const DEFAULT_OLLAMA_LOCAL_MODELS = ["llama3.2", "mistral", "phi3", "codellama", "llama3.1"] as const;

const DEFAULT_OLLAMA_CLOUD_MODELS = ["gpt-oss:120b-cloud", "kimi-k2.5:cloud"] as const;

const DEFAULT_ANTHROPIC_MODELS = [
  "claude-3-5-sonnet-20241022",
  "claude-3-5-haiku-20241022",
  "claude-3-opus-20240229",
  "claude-3-sonnet-20240229",
  "claude-3-haiku-20240307",
] as const;

function mergeModelOptions(
  defaults: readonly string[],
  fetched: string[] | null | undefined,
  current: string,
): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const d of defaults) {
    if (!seen.has(d)) {
      seen.add(d);
      out.push(d);
    }
  }
  if (fetched) {
    for (const f of [...fetched].sort((a, b) => a.localeCompare(b))) {
      if (!seen.has(f)) {
        seen.add(f);
        out.push(f);
      }
    }
  }
  const cur = current.trim();
  if (cur && !seen.has(cur)) {
    out.push(cur);
  }
  return out;
}

type ModelPickRowProps = {
  htmlFor: string;
  label: string;
  value: string;
  optionIds: string[];
  disabled?: boolean;
  loading: boolean;
  onChangeModel: (v: string) => void;
  onRefresh: () => void | Promise<void>;
  refreshLabel: string;
};

function ModelPickRow({
  htmlFor,
  label,
  value,
  optionIds,
  disabled,
  loading,
  onChangeModel,
  onRefresh,
  refreshLabel,
}: ModelPickRowProps) {
  const safeValue = optionIds.includes(value) ? value : optionIds[0] ?? "";
  return (
    <>
      <label className="block text-xs font-medium text-slate-400" htmlFor={htmlFor}>
        {label}
      </label>
      <div className="flex items-center gap-2">
        <select
          id={htmlFor}
          title="Select model…"
          className="min-w-0 flex-1 cursor-pointer rounded-lg border border-slate-800/90 bg-slate-950/60 py-2 pl-3 pr-2 font-mono text-sm text-slate-200 outline-none focus:border-indigo-500/50 disabled:cursor-not-allowed disabled:opacity-50 [color-scheme:dark]"
          value={safeValue}
          disabled={disabled || optionIds.length === 0}
          onChange={(e) => onChangeModel(e.target.value)}
        >
          {optionIds.map((id) => (
            <option key={id} value={id} className="bg-slate-900">
              {id}
            </option>
          ))}
        </select>
        <button
          type="button"
          disabled={disabled || loading}
          onClick={() => void onRefresh()}
          className="inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-slate-700 bg-slate-900 px-2.5 py-2 text-[11px] font-semibold text-slate-200 hover:bg-slate-800 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {loading ? <Loader2 className="size-4 shrink-0 animate-spin text-slate-300" aria-hidden /> : null}
          <span className="whitespace-nowrap">{refreshLabel}</span>
        </button>
      </div>
    </>
  );
}

/** Preset caps for assistant generation; `null` = defer to model / context (see backend). */
const MAX_TOKEN_SELECT_OPTIONS: { value: string; label: string; tokens: number | null }[] = [
  { value: "default", label: "Use model default (recommended)", tokens: null },
  { value: "4096", label: "4,096", tokens: 4096 },
  { value: "8192", label: "8,192", tokens: 8192 },
  { value: "16384", label: "16,384", tokens: 16384 },
  { value: "32768", label: "32,768", tokens: 32768 },
  { value: "128000", label: "128,000", tokens: 128_000 },
  { value: "200000", label: "200,000 (large-context models)", tokens: 200_000 },
];

function maxTokensSelectValue(settings: SettingsView | null): string {
  if (!settings) return "default";
  const mt = settings.maxTokens;
  if (mt == null) return "default";
  if (MAX_TOKEN_SELECT_OPTIONS.some((o) => o.tokens === mt)) {
    return String(mt);
  }
  return `legacy:${mt}`;
}

type SettingsTab = "general" | "companion";

type DestructiveModal = "memory" | "factory";

type AppDataPaths = {
  dataDirectory: string;
  databaseFile: string;
  workspaceDirectory: string;
  sqliteProfile: string;
  novaDataDirEnv: boolean;
  novaPortableEnv: boolean;
};

export function SettingsPanel({
  open,
  onCompanionActiveProfileChange,
  chatActiveProfileId,
}: Props) {
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
  const [backend, setBackend] = useState<string | null>(null);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [providers, setProviders] = useState<ProviderDescriptor[]>([]);
  const [openaiKeyInput, setOpenaiKeyInput] = useState("");
  const [anthropicKeyInput, setAnthropicKeyInput] = useState("");
  const [ollamaKeyInput, setOllamaKeyInput] = useState("");
  const [cloudModelTags, setCloudModelTags] = useState<string[] | null>(null);
  const [cloudTagsLoading, setCloudTagsLoading] = useState(false);
  const [openaiFetchedModels, setOpenaiFetchedModels] = useState<string[] | null>(null);
  const [openaiModelsLoading, setOpenaiModelsLoading] = useState(false);
  const [localOllamaTags, setLocalOllamaTags] = useState<string[] | null>(null);
  const [localOllamaTagsLoading, setLocalOllamaTagsLoading] = useState(false);
  const [anthropicFetchedModels, setAnthropicFetchedModels] = useState<string[] | null>(null);
  const [anthropicModelsLoading, setAnthropicModelsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [destructiveModal, setDestructiveModal] = useState<DestructiveModal | null>(null);
  const [wipeConfirmInput, setWipeConfirmInput] = useState("");
  const [wiping, setWiping] = useState(false);
  const [dataPaths, setDataPaths] = useState<AppDataPaths | null>(null);
  const [revealPathError, setRevealPathError] = useState<string | null>(null);

  const loadVersion = useCallback(async () => {
    try {
      const v = await invoke<string>("app_version");
      setBackend(v);
    } catch {
      setBackend("Unavailable (open via Tauri)");
    }
  }, []);

  const refreshSettings = useCallback(async () => {
    try {
      setError(null);
      const s = await invoke<SettingsView>("settings_get");
      setSettings(s);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadProviders = useCallback(async () => {
    try {
      const list = await invoke<ProviderDescriptor[]>("provider_list_available");
      setProviders(list);
    } catch {
      setProviders([]);
    }
  }, []);

  const refreshDataPaths = useCallback(async () => {
    try {
      const p = await invoke<AppDataPaths>("app_data_paths");
      setDataPaths(p);
    } catch {
      setDataPaths(null);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    void refreshSettings();
    void loadProviders();
    void refreshDataPaths();
  }, [open, refreshSettings, loadProviders, refreshDataPaths]);

  useEffect(() => {
    if (!open) {
      setDestructiveModal(null);
      setWipeConfirmInput("");
    }
  }, [open]);

  useEffect(() => {
    if (settings?.selectedProvider !== "ollama_cloud") {
      setCloudModelTags(null);
    }
  }, [settings?.selectedProvider]);

  const flushDebounce = useCallback(() => {
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
  }, []);

  const schedulePatch = useCallback(
    (patch: SettingsPatch) => {
      flushDebounce();
      debounceRef.current = setTimeout(() => {
        debounceRef.current = null;
        void (async () => {
          try {
            setError(null);
            const next = await invoke<SettingsView>("settings_update", { patch });
            setSettings(next);
          } catch (e) {
            setError(String(e));
          }
        })();
      }, DEBOUNCE_MS);
    },
    [flushDebounce],
  );

  const applyModelPatchImmediate = useCallback(
    async (patch: Pick<SettingsPatch, "openaiModel" | "ollamaModel" | "anthropicModel">) => {
      try {
        setError(null);
        flushDebounce();
        const next = await invoke<SettingsView>("settings_update", { patch });
        setSettings(next);
      } catch (e) {
        setError(String(e));
        await refreshSettings();
      }
    },
    [flushDebounce, refreshSettings],
  );

  useEffect(() => () => flushDebounce(), [flushDebounce]);

  const openaiModelOptions = useMemo(
    () => mergeModelOptions(DEFAULT_OPENAI_MODELS, openaiFetchedModels, settings?.openaiModel ?? ""),
    [openaiFetchedModels, settings?.openaiModel],
  );

  const localOllamaModelOptions = useMemo(
    () =>
      mergeModelOptions(DEFAULT_OLLAMA_LOCAL_MODELS, localOllamaTags, settings?.ollamaModel ?? ""),
    [localOllamaTags, settings?.ollamaModel],
  );

  const cloudOllamaModelOptions = useMemo(
    () =>
      mergeModelOptions(DEFAULT_OLLAMA_CLOUD_MODELS, cloudModelTags, settings?.ollamaModel ?? ""),
    [cloudModelTags, settings?.ollamaModel],
  );

  const anthropicModelOptions = useMemo(
    () =>
      mergeModelOptions(
        DEFAULT_ANTHROPIC_MODELS,
        anthropicFetchedModels,
        settings?.anthropicModel ?? "",
      ),
    [anthropicFetchedModels, settings?.anthropicModel],
  );

  const saveOpenaiKey = async () => {
    try {
      setError(null);
      await invoke("settings_save_api_key", { provider: "openai", apiKey: openaiKeyInput });
      setOpenaiKeyInput("");
      await refreshSettings();
    } catch (e) {
      setError(String(e));
    }
  };

  const saveAnthropicKey = async () => {
    try {
      setError(null);
      await invoke("settings_save_api_key", { provider: "anthropic", apiKey: anthropicKeyInput });
      setAnthropicKeyInput("");
      await refreshSettings();
    } catch (e) {
      setError(String(e));
    }
  };

  const saveOllamaCloudKey = async () => {
    try {
      setError(null);
      await invoke("settings_save_api_key", { provider: "ollama", apiKey: ollamaKeyInput });
      setOllamaKeyInput("");
      await refreshSettings();
    } catch (e) {
      setError(String(e));
    }
  };

  const refreshOllamaCloudModels = useCallback(async () => {
    try {
      setCloudTagsLoading(true);
      setError(null);
      const tags = await invoke<string[]>("ollama_cloud_list_models");
      setCloudModelTags(tags);
    } catch (e) {
      setCloudModelTags(null);
      setError(String(e));
    } finally {
      setCloudTagsLoading(false);
    }
  }, []);

  const refreshOpenaiModels = useCallback(async () => {
    try {
      setOpenaiModelsLoading(true);
      setError(null);
      const ids = await invoke<string[]>("openai_list_models");
      setOpenaiFetchedModels(ids);
    } catch (e) {
      setOpenaiFetchedModels(null);
      setError(String(e));
    } finally {
      setOpenaiModelsLoading(false);
    }
  }, []);

  const refreshLocalOllamaModels = useCallback(async () => {
    try {
      setLocalOllamaTagsLoading(true);
      setError(null);
      const tags = await invoke<string[]>("ollama_list_local_models");
      setLocalOllamaTags(tags);
    } catch (e) {
      setLocalOllamaTags(null);
      setError(String(e));
    } finally {
      setLocalOllamaTagsLoading(false);
    }
  }, []);

  const refreshAnthropicModels = useCallback(async () => {
    try {
      setAnthropicModelsLoading(true);
      setError(null);
      const ids = await invoke<string[]>("anthropic_list_models");
      setAnthropicFetchedModels(ids);
    } catch (e) {
      setAnthropicFetchedModels(null);
      setError(String(e));
    } finally {
      setAnthropicModelsLoading(false);
    }
  }, []);

  const onProviderChange = async (id: string) => {
    try {
      setError(null);
      await invoke("provider_switch", { providerId: id });
      await refreshSettings();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <aside
      id="nova-settings-panel"
      aria-hidden={!open}
      className={
        open
          ? "h-full min-h-0 w-80 shrink-0 overflow-hidden border-l border-slate-800/80 bg-slate-900/35 shadow-[-12px_0_40px_rgba(0,0,0,0.35)] transition-[width,opacity] duration-200 ease-out"
          : "h-full min-h-0 w-0 shrink-0 overflow-hidden border-l border-transparent opacity-0 transition-[width,opacity] duration-200 ease-out"
      }
    >
      <div className="flex h-full w-80 flex-col" inert={!open}>
        <div className="flex flex-col gap-2 border-b border-slate-800/80 px-4 py-3">
          <div className="flex items-center gap-2">
            <SlidersHorizontal className="size-4 text-slate-400" aria-hidden />
            <h2 className="text-sm font-semibold text-white">Settings</h2>
          </div>
          <div className="flex gap-1 rounded-lg bg-slate-950/60 p-1">
            <button
              type="button"
              onClick={() => setSettingsTab("general")}
              className={
                settingsTab === "general"
                  ? "flex-1 rounded-md bg-slate-800 px-2 py-1.5 text-xs font-medium text-white shadow-sm"
                  : "flex-1 rounded-md px-2 py-1.5 text-xs font-medium text-slate-400 transition hover:text-slate-200"
              }
            >
              General
            </button>
            <button
              type="button"
              onClick={() => setSettingsTab("companion")}
              className={
                settingsTab === "companion"
                  ? "flex-1 rounded-md bg-indigo-900/50 px-2 py-1.5 text-xs font-medium text-white shadow-sm ring-1 ring-indigo-500/30"
                  : "flex-1 rounded-md px-2 py-1.5 text-xs font-medium text-slate-400 transition hover:text-slate-200"
              }
            >
              <span className="inline-flex items-center justify-center gap-1">
                <Heart className="size-3" aria-hidden />
                Companion
              </span>
            </button>
          </div>
        </div>

        <div
          className={
            settingsTab === "companion"
              ? "flex min-h-0 flex-1 flex-col overflow-hidden px-4 py-4"
              : "min-h-0 flex-1 space-y-6 overflow-y-auto px-4 py-4"
          }
        >
          {error ? (
            <p className="rounded-md border border-red-900/60 bg-red-950/40 px-2 py-1.5 text-xs text-red-200">
              {error}
            </p>
          ) : null}

          {settingsTab === "companion" ? (
            <CompanionPersonalitySection
              visible={open}
              chatActiveProfileId={chatActiveProfileId ?? "default"}
              onActiveProfileMemorySync={onCompanionActiveProfileChange}
            />
          ) : null}

          {settingsTab === "general" ? (
            <>
          <section className="space-y-2">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">
              Appearance
            </h3>
            <div className="flex items-center gap-3 rounded-lg border border-slate-800/80 bg-slate-950/50 px-3 py-2.5">
              <Moon className="size-4 text-indigo-300" aria-hidden />
              <div>
                <p className="text-sm font-medium text-white">Dark mode</p>
                <p className="text-xs text-slate-500">Default for Nova</p>
              </div>
            </div>
          </section>

          <section className="space-y-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">
              Provider
            </h3>
            <label className="block text-xs font-medium text-slate-400" htmlFor="provider-select">
              Active backend
            </label>
            <div className="relative">
              <Cpu
                className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-slate-500"
                aria-hidden
              />
              <select
                id="provider-select"
                value={settings?.selectedProvider ?? "placeholder"}
                disabled={!settings}
                onChange={(e) => void onProviderChange(e.target.value)}
                className="w-full appearance-none rounded-lg border border-slate-800/90 bg-slate-950/60 py-2.5 pl-10 pr-9 text-sm text-slate-200 outline-none focus:border-indigo-500/50 focus:ring-2 focus:ring-indigo-500/25 disabled:opacity-50"
              >
                {providers
                  .filter((p) => p.id !== "ollama" && p.id !== "ollama_cloud")
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.label}
                      {p.requiresApiKey ? " · API key required" : ""}
                    </option>
                  ))}
                {providers.some((p) => p.id === "ollama" || p.id === "ollama_cloud") ? (
                  <optgroup label="Ollama — local vs cloud">
                    {providers
                      .filter((p) => p.id === "ollama" || p.id === "ollama_cloud")
                      .map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.label}
                          {p.requiresApiKey ? " · API key required" : ""}
                        </option>
                      ))}
                  </optgroup>
                ) : null}
              </select>
            </div>
          </section>

          <section className="space-y-3 rounded-lg border border-slate-800/80 bg-slate-950/40 p-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">
              OpenAI
            </h3>
            <label className="block text-xs font-medium text-slate-400" htmlFor="openai-base">
              Base URL
            </label>
            <input
              id="openai-base"
              type="url"
              value={settings?.openaiBaseUrl ?? ""}
              disabled={!settings}
              onChange={(e) => {
                const v = e.target.value;
                setSettings((s) => (s ? { ...s, openaiBaseUrl: v } : s));
                schedulePatch({ openaiBaseUrl: v });
              }}
              className="w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 text-sm text-slate-200 outline-none focus:border-indigo-500/50"
            />
            {settings?.selectedProvider === "openai" ? (
              <p className="text-[11px] leading-relaxed text-slate-500">
                With <span className="font-medium text-slate-300">OpenAI</span> selected, use{" "}
                <span className="font-mono text-slate-400">Refresh Models</span> to pull ids from{" "}
                <span className="font-mono text-slate-400">/v1/models</span> (saved key + Base URL). Common models
                stay listed without a refresh.
              </p>
            ) : null}
            <ModelPickRow
              htmlFor="openai-model"
              label="Model"
              value={settings?.openaiModel ?? ""}
              optionIds={openaiModelOptions}
              disabled={!settings}
              loading={openaiModelsLoading}
              onChangeModel={(v) => void applyModelPatchImmediate({ openaiModel: v })}
              onRefresh={refreshOpenaiModels}
              refreshLabel="Refresh Models"
            />
            <details className="mt-2 rounded-md border border-slate-800/60 bg-slate-950/30 px-2 py-2">
              <summary className="cursor-pointer text-[11px] text-slate-500">Type model name…</summary>
              <input
                type="text"
                placeholder="Custom or preview model id"
                value={settings?.openaiModel ?? ""}
                disabled={!settings}
                onChange={(e) => {
                  const v = e.target.value;
                  setSettings((s) => (s ? { ...s, openaiModel: v } : s));
                  schedulePatch({ openaiModel: v });
                }}
                className="mt-2 w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 font-mono text-sm text-slate-200 outline-none focus:border-indigo-500/50"
              />
            </details>
            <div className="flex items-center gap-2 text-xs text-slate-500">
              <KeyRound className="size-3.5 shrink-0" aria-hidden />
              <span>
                API key:{" "}
                {settings?.hasOpenaiApiKey ? (
                  <span className="text-emerald-400/90">saved (encrypted)</span>
                ) : (
                  <span className="text-amber-400/90">not set</span>
                )}
              </span>
            </div>
            <input
              type="password"
              autoComplete="off"
              placeholder="sk-…"
              value={openaiKeyInput}
              onChange={(e) => setOpenaiKeyInput(e.target.value)}
              className="w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 font-mono text-sm text-slate-200 outline-none focus:border-indigo-500/50"
            />
            <button
              type="button"
              onClick={() => void saveOpenaiKey()}
              className="w-full rounded-lg bg-indigo-600 px-3 py-2 text-xs font-semibold text-white hover:bg-indigo-500"
            >
              Save OpenAI API key
            </button>
          </section>

          <section className="space-y-4 rounded-lg border border-slate-800/80 bg-slate-950/40 p-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">Ollama</h3>

            <div className="space-y-3 rounded-md border border-emerald-950/50 bg-emerald-950/10 p-3 ring-1 ring-emerald-900/25">
              <p className="text-[11px] font-semibold uppercase tracking-wider text-emerald-300/90">
                Ollama · Local
              </p>
              <p className="text-[11px] leading-relaxed text-slate-500">
                Uses your own Ollama install (default{" "}
                <span className="font-mono text-slate-400">http://127.0.0.1:11434</span>).
              </p>
              <label className="block text-xs font-medium text-slate-400" htmlFor="ollama-base">
                Base URL
              </label>
              <input
                id="ollama-base"
                type="url"
                value={settings?.ollamaBaseUrl ?? ""}
                disabled={!settings}
                onChange={(e) => {
                  const v = e.target.value;
                  setSettings((s) => (s ? { ...s, ollamaBaseUrl: v } : s));
                  schedulePatch({ ollamaBaseUrl: v });
                }}
                className="w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 text-sm text-slate-200 outline-none focus:border-indigo-500/50"
              />
              {settings?.selectedProvider !== "ollama_cloud" ? (
                <>
                  {settings?.selectedProvider === "ollama" ? (
                    <p className="text-[11px] leading-relaxed text-slate-500">
                      <span className="font-mono text-slate-400">Refresh Models</span> loads tags from your local
                      daemon (<span className="font-mono text-slate-400">/api/tags</span>).
                    </p>
                  ) : null}
                  <ModelPickRow
                    htmlFor="ollama-model-local"
                    label="Model"
                    value={settings?.ollamaModel ?? ""}
                    optionIds={localOllamaModelOptions}
                    disabled={!settings}
                    loading={localOllamaTagsLoading}
                    onChangeModel={(v) => void applyModelPatchImmediate({ ollamaModel: v })}
                    onRefresh={refreshLocalOllamaModels}
                    refreshLabel="Refresh Models"
                  />
                  <details className="mt-2 rounded-md border border-slate-800/60 bg-slate-950/30 px-2 py-2">
                    <summary className="cursor-pointer text-[11px] text-slate-500">Type model name…</summary>
                    <input
                      type="text"
                      placeholder="e.g. my.gguf:latest"
                      value={settings?.ollamaModel ?? ""}
                      disabled={!settings}
                      onChange={(e) => {
                        const v = e.target.value;
                        setSettings((s) => (s ? { ...s, ollamaModel: v } : s));
                        schedulePatch({ ollamaModel: v });
                      }}
                      className="mt-2 w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 font-mono text-sm text-slate-200 outline-none focus:border-indigo-500/50"
                    />
                  </details>
                </>
              ) : (
                <p className="text-[11px] leading-relaxed text-slate-500">
                  With <span className="font-medium text-slate-300">Ollama · Cloud</span> selected, set the model
                  name in the cloud panel below.
                </p>
              )}
            </div>

            <div className="space-y-3 rounded-md border border-sky-900/50 bg-sky-950/20 p-3 ring-1 ring-sky-800/35">
              <p className="text-[11px] font-semibold uppercase tracking-wider text-sky-300/95">Ollama · Cloud</p>
              {settings?.selectedProvider === "ollama_cloud" ? (
                <>
                  <p className="text-xs leading-relaxed text-slate-100">
                    Ollama Cloud runs models on Ollama&apos;s servers (not locally). Requires an Ollama API key from{" "}
                    <a
                      href={OLLAMA_CLOUD_KEYS_URL}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="font-medium text-sky-400 underline-offset-2 hover:text-sky-300 hover:underline"
                    >
                      https://ollama.com/settings/keys
                    </a>
                    .
                  </p>
                  <p className="text-[11px] leading-relaxed text-slate-500">
                    <span className="font-mono text-slate-400">Refresh Models</span> loads cloud tags from{" "}
                    <span className="font-mono text-slate-400">https://ollama.com/api/tags</span>. Preset{" "}
                    <span className="font-mono text-slate-400">{OLLAMA_CLOUD_MODEL_PLACEHOLDER}</span> entries stay
                    available without a refresh.
                  </p>
                  <ModelPickRow
                    htmlFor="ollama-cloud-model"
                    label="Model"
                    value={settings?.ollamaModel ?? ""}
                    optionIds={cloudOllamaModelOptions}
                    disabled={!settings}
                    loading={cloudTagsLoading}
                    onChangeModel={(v) => void applyModelPatchImmediate({ ollamaModel: v })}
                    onRefresh={refreshOllamaCloudModels}
                    refreshLabel="Refresh Models"
                  />
                  <details className="mt-2 rounded-md border border-slate-800/60 bg-slate-950/30 px-2 py-2">
                    <summary className="cursor-pointer text-[11px] text-slate-500">Type model name…</summary>
                    <input
                      type="text"
                      placeholder={OLLAMA_CLOUD_MODEL_PLACEHOLDER}
                      value={settings?.ollamaModel ?? ""}
                      disabled={!settings}
                      onChange={(e) => {
                        const v = e.target.value;
                        setSettings((s) => (s ? { ...s, ollamaModel: v } : s));
                        schedulePatch({ ollamaModel: v });
                      }}
                      className="mt-2 w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 font-mono text-sm text-slate-200 outline-none focus:border-sky-500/50"
                    />
                  </details>
                </>
              ) : (
                <p className="text-[11px] leading-relaxed text-slate-500">
                  Choose <span className="font-medium text-sky-200/90">Ollama · Cloud — models on ollama.com</span>{" "}
                  in the provider menu above to configure the cloud model, refresh the catalog from{" "}
                  <span className="font-mono text-slate-400">/api/tags</span>, and save your API key.
                </p>
              )}

              <div className="space-y-2 border-t border-slate-800/70 pt-3">
                <div className="flex items-center gap-2 text-xs text-slate-500">
                  <KeyRound className="size-3.5 shrink-0" aria-hidden />
                  <span>
                    Ollama Cloud API key:{" "}
                    {settings?.hasOllamaApiKey ? (
                      <span className="text-emerald-400/90">saved (encrypted)</span>
                    ) : (
                      <span className="text-amber-400/90">not set</span>
                    )}
                  </span>
                </div>
                <input
                  type="password"
                  autoComplete="off"
                  placeholder="Paste Ollama API key"
                  value={ollamaKeyInput}
                  onChange={(e) => setOllamaKeyInput(e.target.value)}
                  className="w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 font-mono text-sm text-slate-200 outline-none focus:border-sky-500/50"
                />
                <button
                  type="button"
                  onClick={() => void saveOllamaCloudKey()}
                  className="w-full rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-xs font-semibold text-slate-200 hover:bg-slate-800"
                >
                  Save Ollama Cloud API key
                </button>
              </div>
            </div>
          </section>

          <section className="space-y-3 rounded-lg border border-slate-800/80 bg-slate-950/40 p-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">
              Anthropic (Claude)
            </h3>
            {settings?.selectedProvider === "anthropic" ? (
              <p className="text-[11px] leading-relaxed text-slate-500">
                <span className="font-mono text-slate-400">Refresh Models</span> lists models your API key can access.
                Common Claude ids remain available without a refresh.
              </p>
            ) : null}
            <ModelPickRow
              htmlFor="anthropic-model"
              label="Model"
              value={settings?.anthropicModel ?? ""}
              optionIds={anthropicModelOptions}
              disabled={!settings}
              loading={anthropicModelsLoading}
              onChangeModel={(v) => void applyModelPatchImmediate({ anthropicModel: v })}
              onRefresh={refreshAnthropicModels}
              refreshLabel="Refresh Models"
            />
            <details className="mt-2 rounded-md border border-slate-800/60 bg-slate-950/30 px-2 py-2">
              <summary className="cursor-pointer text-[11px] text-slate-500">Type model name…</summary>
              <input
                type="text"
                placeholder="e.g. claude-3-5-sonnet-20241022"
                value={settings?.anthropicModel ?? ""}
                disabled={!settings}
                onChange={(e) => {
                  const v = e.target.value;
                  setSettings((s) => (s ? { ...s, anthropicModel: v } : s));
                  schedulePatch({ anthropicModel: v });
                }}
                className="mt-2 w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 font-mono text-sm text-slate-200 outline-none focus:border-indigo-500/50"
              />
            </details>
            <div className="flex items-center gap-2 text-xs text-slate-500">
              <KeyRound className="size-3.5 shrink-0" aria-hidden />
              <span>
                API key:{" "}
                {settings?.hasAnthropicApiKey ? (
                  <span className="text-emerald-400/90">saved (encrypted)</span>
                ) : (
                  <span className="text-amber-400/90">not set</span>
                )}
              </span>
            </div>
            <input
              type="password"
              autoComplete="off"
              placeholder="sk-ant-…"
              value={anthropicKeyInput}
              onChange={(e) => setAnthropicKeyInput(e.target.value)}
              className="w-full rounded-lg border border-slate-800/90 bg-slate-950/60 px-3 py-2 font-mono text-sm text-slate-200 outline-none focus:border-indigo-500/50"
            />
            <button
              type="button"
              onClick={() => void saveAnthropicKey()}
              className="w-full rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-xs font-semibold text-slate-200 hover:bg-slate-800"
            >
              Save Anthropic API key
            </button>
          </section>

          <section className="space-y-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">
              Generation
            </h3>
            <div className="space-y-2">
              <div className="flex items-center justify-between text-xs text-slate-400">
                <span className="inline-flex items-center gap-1.5">
                  <span>Temperature</span>
                  <button
                    type="button"
                    className="inline-flex size-5 items-center justify-center rounded-full border border-slate-600/80 bg-slate-900/80 text-[11px] font-semibold text-slate-400 hover:border-slate-500 hover:text-slate-200"
                    title={TEMPERATURE_INFO}
                    aria-label={TEMPERATURE_INFO}
                  >
                    i
                  </button>
                </span>
                <span className="font-mono text-slate-300">
                  {settings?.temperature?.toFixed(2) ?? "—"}
                </span>
              </div>
              <input
                type="range"
                min={0}
                max={2}
                step={0.05}
                value={settings?.temperature ?? 0.7}
                disabled={!settings}
                onChange={(e) => {
                  const t = Number(e.target.value);
                  setSettings((s) => (s ? { ...s, temperature: t } : s));
                  flushDebounce();
                  void (async () => {
                    try {
                      setError(null);
                      const next = await invoke<SettingsView>("settings_update", {
                        patch: { temperature: t },
                      });
                      setSettings(next);
                    } catch (err) {
                      setError(String(err));
                      await refreshSettings();
                    }
                  })();
                }}
                className="h-2 w-full cursor-pointer accent-indigo-500 disabled:opacity-50"
              />
            </div>
            <div className="flex items-start gap-3 rounded-lg border border-slate-800/70 bg-slate-950/35 px-3 py-2.5">
              <input
                id="agent-web-tools"
                type="checkbox"
                className="mt-0.5 size-4 shrink-0 cursor-pointer rounded border-slate-600 accent-indigo-500 disabled:cursor-not-allowed"
                checked={settings?.agentWebToolsEnabled ?? false}
                disabled={
                  !settings ||
                  !["openai", "ollama", "ollama_cloud", "anthropic"].includes(settings.selectedProvider)
                }
                onChange={(e) => {
                  const agentWebToolsEnabled = e.target.checked;
                  setSettings((s) => (s ? { ...s, agentWebToolsEnabled } : s));
                  flushDebounce();
                  void (async () => {
                    try {
                      setError(null);
                      const next = await invoke<SettingsView>("settings_update", {
                        patch: { agentWebToolsEnabled },
                      });
                      setSettings(next);
                    } catch (err) {
                      setError(String(err));
                      await refreshSettings();
                    }
                  })();
                }}
              />
              <div className="min-w-0 space-y-1">
                <label htmlFor="agent-web-tools" className="cursor-pointer text-xs font-medium text-slate-300">
                  Allow web tools for the assistant (OpenAI, Ollama, Anthropic)
                </label>
                <p className="text-[11px] leading-relaxed text-slate-500">
                  When enabled, the model may call built-in tools: public web search (DuckDuckGo) and fetching
                  http(s) pages you or it names. Requests are sent from this device; local and private URLs are
                  blocked. Ollama requires a tool-capable model. Off by default.
                </p>
                {settings &&
                !["openai", "ollama", "ollama_cloud", "anthropic"].includes(settings.selectedProvider) ? (
                  <p className="text-[11px] text-amber-400/90">
                    Switch provider to OpenAI, Ollama, or Anthropic to use this option.
                  </p>
                ) : null}
              </div>
            </div>
            <div className="flex items-start gap-3 rounded-lg border border-slate-800/70 bg-slate-950/35 px-3 py-2.5">
              <input
                id="agent-workspace-tools"
                type="checkbox"
                className="mt-0.5 size-4 shrink-0 cursor-pointer rounded border-slate-600 accent-indigo-500 disabled:cursor-not-allowed"
                checked={settings?.agentWorkspaceEnabled ?? false}
                disabled={
                  !settings ||
                  !["openai", "ollama", "ollama_cloud", "anthropic"].includes(settings.selectedProvider)
                }
                onChange={(e) => {
                  const agentWorkspaceEnabled = e.target.checked;
                  setSettings((s) => (s ? { ...s, agentWorkspaceEnabled } : s));
                  flushDebounce();
                  void (async () => {
                    try {
                      setError(null);
                      const next = await invoke<SettingsView>("settings_update", {
                        patch: { agentWorkspaceEnabled },
                      });
                      setSettings(next);
                    } catch (err) {
                      setError(String(err));
                      await refreshSettings();
                    }
                  })();
                }}
              />
              <div className="min-w-0 space-y-1">
                <label
                  htmlFor="agent-workspace-tools"
                  className="cursor-pointer text-xs font-medium text-slate-300"
                >
                  Allow workspace file tools for the assistant
                </label>
                <p className="text-[11px] leading-relaxed text-slate-500">
                  When enabled, the model may list, read, and write UTF-8 text files only inside the Nova
                  workspace folder (a subdirectory of your data directory). Paths are relative; parent
                  segments like <span className="font-mono text-slate-400">..</span> are rejected. Off by
                  default.
                </p>
                {dataPaths?.workspaceDirectory ? (
                  <p className="break-all font-mono text-[10px] text-slate-500" title={dataPaths.workspaceDirectory}>
                    Workspace: {dataPaths.workspaceDirectory}
                  </p>
                ) : null}
                {settings &&
                !["openai", "ollama", "ollama_cloud", "anthropic"].includes(settings.selectedProvider) ? (
                  <p className="text-[11px] text-amber-400/90">
                    Switch provider to OpenAI, Ollama, or Anthropic to use this option.
                  </p>
                ) : null}
              </div>
            </div>
            <label className="block text-xs font-medium text-slate-400" htmlFor="max-tokens-select">
              Max input tokens
            </label>
            <p className="text-[11px] leading-relaxed text-slate-500">
              Presets match common context sizes. This caps how many tokens the model may produce in its
              reply (generation budget).{" "}
              <span className="text-slate-400">
                <strong className="font-medium text-slate-300">Use model default</strong> lets Nova use this
                model&apos;s context window from the provider, then apply a safe per-API limit. Explicit values
                are clamped if the active model cannot honor them.
              </span>
            </p>
            <select
              id="max-tokens-select"
              disabled={!settings}
              value={maxTokensSelectValue(settings)}
              onChange={(e) => {
                const v = e.target.value;
                if (v.startsWith("legacy:")) return;
                const maxTokens = v === "default" ? null : Number.parseInt(v, 10);
                if (v !== "default" && Number.isNaN(maxTokens)) return;

                flushDebounce();
                setSettings((s) => (s ? { ...s, maxTokens } : s));
                void (async () => {
                  try {
                    setError(null);
                    const next = await invoke<SettingsView>("settings_update", {
                      patch: { maxTokens },
                    });
                    setSettings({ ...next, maxTokens: next.maxTokens ?? null });
                  } catch (err) {
                    setError(String(err));
                    await refreshSettings();
                  }
                })();
              }}
              className="w-full cursor-pointer rounded-lg border border-zinc-600 bg-zinc-900 py-2.5 pl-3 pr-8 text-sm text-zinc-100 outline-none [color-scheme:dark] focus:border-indigo-500/60 focus:ring-2 focus:ring-indigo-500/25 disabled:cursor-not-allowed disabled:opacity-50 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-100"
            >
              {MAX_TOKEN_SELECT_OPTIONS.map((o) => (
                <option
                  key={o.value}
                  value={o.value}
                  className="bg-zinc-900 text-zinc-100 dark:bg-zinc-800 dark:text-zinc-100"
                >
                  {o.label}
                </option>
              ))}
              {settings &&
              typeof settings.maxTokens === "number" &&
              !MAX_TOKEN_SELECT_OPTIONS.some((o) => o.tokens === settings.maxTokens) ? (
                <option
                  value={`legacy:${settings.maxTokens}`}
                  className="bg-zinc-900 text-zinc-400 dark:bg-zinc-800 dark:text-zinc-400"
                >
                  Saved value: {settings.maxTokens.toLocaleString()} (pick a preset to replace)
                </option>
              ) : null}
            </select>
          </section>

          <section className="space-y-3 rounded-lg border border-red-900/40 bg-red-950/12 p-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-red-300/90">
              Data
            </h3>
            <p className="text-[11px] leading-relaxed text-red-200/70">
              Wipe chat history and Memory Anchor data, or perform a full factory reset (includes settings
              and companions).
            </p>
            <button
              type="button"
              onClick={() => {
                setWipeConfirmInput("");
                setDestructiveModal("memory");
              }}
              className="w-full rounded-lg border border-red-600/90 bg-red-900/55 px-3 py-2.5 text-sm font-semibold text-red-50 shadow-sm hover:bg-red-800/70"
            >
              Wipe All Memories
            </button>
            <button
              type="button"
              onClick={() => {
                setWipeConfirmInput("");
                setDestructiveModal("factory");
              }}
              className="w-full rounded-md border border-red-950/80 bg-red-950/40 px-2 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-red-200/90 hover:bg-red-950/70"
            >
              Factory Reset
            </button>
          </section>

          <section className="space-y-2 rounded-lg border border-slate-800/80 bg-slate-950/40 p-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">
              Local data paths
            </h3>
            <p className="text-xs leading-relaxed text-slate-500">
              Chats, settings, and <code className="text-slate-400">personality.json</code> live here — not in your git
              checkout. On Linux the default is under{" "}
              <code className="text-slate-400">~/.local/share/</code> (XDG data home). Set{" "}
              <code className="text-slate-400">NOVA_DATA_DIR</code> to pin a visible folder (e.g. inside your project or a
              synced drive) so every machine uses the same files.
            </p>
            {dataPaths ? (
              <ul className="space-y-1.5 font-mono text-[10px] leading-relaxed text-slate-400 break-all">
                <li>
                  <span className="text-slate-600">Data directory · </span>
                  {dataPaths.dataDirectory}
                </li>
                <li>
                  <span className="text-slate-600">SQLite file · </span>
                  {dataPaths.databaseFile}
                </li>
                <li className="text-slate-500">
                  Profile: {dataPaths.sqliteProfile}
                  {dataPaths.novaDataDirEnv ? " · NOVA_DATA_DIR set" : ""}
                  {dataPaths.novaPortableEnv ? " · NOVA_PORTABLE set" : ""}
                </li>
              </ul>
            ) : (
              <p className="text-[11px] text-slate-600">Unavailable outside the Tauri desktop shell.</p>
            )}
            {revealPathError ? (
              <p className="text-[11px] text-amber-200/90">{revealPathError}</p>
            ) : null}
            <button
              type="button"
              disabled={!dataPaths}
              onClick={() => {
                setRevealPathError(null);
                void (async () => {
                  try {
                    await invoke("reveal_data_directory");
                  } catch (e) {
                    setRevealPathError(e instanceof Error ? e.message : String(e));
                  }
                })();
              }}
              className="inline-flex items-center gap-2 rounded-lg border border-slate-700/90 bg-slate-900/70 px-3 py-2 text-xs font-medium text-slate-200 transition hover:border-slate-600 hover:bg-slate-800/80 disabled:pointer-events-none disabled:opacity-40"
            >
              <FolderOpen className="size-3.5 text-slate-400" aria-hidden />
              Open data folder in file manager
            </button>
          </section>

          <section className="space-y-2 rounded-lg border border-slate-800/80 bg-slate-950/40 p-3">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-slate-500">
              About
            </h3>
            <p className="text-xs leading-relaxed text-slate-500">
              Settings and API keys are stored under your Nova data directory; keys are encrypted
              (AES-GCM) with material from the OS keychain when available.
            </p>
            <button
              type="button"
              onClick={() => void loadVersion()}
              className="mt-1 text-xs font-medium text-indigo-400 hover:text-indigo-300"
            >
              Read backend version
            </button>
            {backend ? (
              <p className="font-mono text-[11px] text-slate-400">{backend}</p>
            ) : null}
          </section>
            </>
          ) : null}
        </div>
      </div>

      {destructiveModal ? (
        <div
          className="fixed inset-0 z-[200] flex items-center justify-center bg-black/70 p-4"
          role="dialog"
          aria-modal="true"
          aria-labelledby="destructive-modal-warning"
        >
          <div className="max-h-[90vh] w-full max-w-md overflow-y-auto rounded-xl border border-red-900/60 bg-slate-950 p-4 shadow-2xl">
            <p
              id="destructive-modal-warning"
              className="whitespace-pre-line text-xs leading-relaxed text-slate-200"
            >
              {destructiveModal === "memory" ? MEMORY_WIPE_COPY : FACTORY_RESET_COPY}
            </p>
            <input
              id="destructive-confirm-input"
              type="text"
              autoComplete="off"
              value={wipeConfirmInput}
              onChange={(e) => setWipeConfirmInput(e.target.value)}
              placeholder="Type CONFIRM"
              aria-label="Confirmation: type CONFIRM"
              className="mt-4 w-full rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 font-mono text-sm text-slate-100 outline-none focus:border-red-500/60"
            />
            <div className="mt-4 flex gap-2">
              <button
                type="button"
                onClick={() => {
                  setDestructiveModal(null);
                  setWipeConfirmInput("");
                }}
                className="flex-1 rounded-lg border border-slate-600 bg-slate-900 px-3 py-2 text-sm font-medium text-slate-200 hover:bg-slate-800"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={wiping || wipeConfirmInput !== "CONFIRM"}
                onClick={() => {
                  if (wipeConfirmInput !== "CONFIRM") return;
                  void (async () => {
                    try {
                      setWiping(true);
                      setError(null);
                      if (destructiveModal === "memory") {
                        await invoke("database_wipe_memories");
                      } else {
                        await invoke("database_wipe_all");
                      }
                      setDestructiveModal(null);
                      setWipeConfirmInput("");
                      window.location.reload();
                    } catch (e) {
                      setError(String(e));
                    } finally {
                      setWiping(false);
                    }
                  })();
                }}
                className="flex-1 rounded-lg border border-red-700 bg-red-900/70 px-3 py-2 text-sm font-semibold text-white hover:bg-red-800 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {wiping
                  ? destructiveModal === "memory"
                    ? "Wiping…"
                    : "Resetting…"
                  : destructiveModal === "memory"
                    ? "Wipe"
                    : "Reset"}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </aside>
  );
}
