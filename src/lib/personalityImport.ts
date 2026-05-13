/**
 * Import Nova `personality.json` exports and OpenClaw-style markdown identity files.
 *
 * OpenClaw workspace identity layer (typical files):
 * - SOUL.md — agent personality, communication, values, expertise
 * - IDENTITY.md — name, creature type, visual description, vibe
 * - USER.md — context about the human user
 * - JOURNAL.md / MEMORY.md — running notes or memory text (layouts vary)
 * - TOOLS.md — optional tool notes (imported into special instructions)
 */

import type { PersonalityFile, PersonalityProfile } from "@/lib/personalityPrompt";

function asRecord(v: unknown): Record<string, unknown> | null {
  return v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
}

function pickStr(o: Record<string, unknown>, ...keys: string[]): string {
  for (const k of keys) {
    const v = o[k];
    if (typeof v === "string") return v;
  }
  return "";
}

function newId(): string {
  return typeof crypto !== "undefined" && crypto.randomUUID ? crypto.randomUUID() : `p-${Date.now()}`;
}

function normalizeProfile(raw: Record<string, unknown>, fallbackId: string): PersonalityProfile {
  return {
    id: pickStr(raw, "id", "profileId") || fallbackId,
    profileName: pickStr(raw, "profileName", "profile_name", "name") || "Imported profile",
    companionName: pickStr(raw, "companionName", "companion_name") || "Nova",
    corePersonality: pickStr(raw, "corePersonality", "core_personality"),
    toneOfVoice: pickStr(raw, "toneOfVoice", "tone_of_voice"),
    backgroundStory: pickStr(raw, "backgroundStory", "background_story"),
    coreValues: pickStr(raw, "coreValues", "core_values"),
    relationshipStyle: pickStr(raw, "relationshipStyle", "relationship_style"),
    specialInstructions: pickStr(raw, "specialInstructions", "special_instructions"),
    avatarDescription: (() => {
      const v = raw.avatarDescription ?? raw.avatar_description;
      if (v === null || v === undefined) return null;
      if (typeof v === "string") return v.trim() === "" ? null : v;
      return null;
    })(),
  };
}

function profileLooksNonEmpty(p: PersonalityProfile): boolean {
  return (
    p.profileName.trim() !== "" ||
    p.companionName.trim() !== "" ||
    p.corePersonality.trim() !== "" ||
    p.toneOfVoice.trim() !== "" ||
    p.backgroundStory.trim() !== "" ||
    p.coreValues.trim() !== "" ||
    p.relationshipStyle.trim() !== "" ||
    p.specialInstructions.trim() !== ""
  );
}

export type PersonalityJsonImport =
  | { kind: "file"; file: PersonalityFile }
  | { kind: "profiles"; profiles: PersonalityProfile[]; suggestedActiveId?: string };

/**
 * Parse JSON: full `PersonalityFile`, or `{ "profiles": [...] }`, or a single profile object.
 */
export function parsePersonalityJson(text: string): PersonalityJsonImport {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text) as unknown;
  } catch {
    throw new Error("Invalid JSON — could not parse the file.");
  }
  const root = asRecord(parsed);
  if (!root) throw new Error("JSON root must be an object.");

  const profilesRaw = root.profiles;
  if (Array.isArray(profilesRaw)) {
    const profiles = profilesRaw
      .map((p, i) => {
        const r = asRecord(p);
        if (!r) throw new Error(`profiles[${i}] must be an object.`);
        const id = pickStr(r, "id", "profileId") || newId();
        return normalizeProfile(r, id);
      })
      .filter(profileLooksNonEmpty);

    if (profiles.length === 0) throw new Error("No valid profiles found in JSON.");

    const hasFileShape =
      pickStr(root, "activeProfileId", "active_profile_id").length > 0 || typeof root.version === "number";

    if (hasFileShape) {
      const file: PersonalityFile = {
        version: typeof root.version === "number" ? root.version : 1,
        profiles,
        activeProfileId:
          pickStr(root, "activeProfileId", "active_profile_id") || profiles[0]?.id || "default",
      };
      if (!profiles.some((p) => p.id === file.activeProfileId)) {
        file.activeProfileId = profiles[0]!.id;
      }
      return { kind: "file", file };
    }

    return { kind: "profiles", profiles, suggestedActiveId: profiles[0]?.id };
  }

  if (
    "companionName" in root ||
    "companion_name" in root ||
    "corePersonality" in root ||
    "core_personality" in root ||
    "profileName" in root ||
    "profile_name" in root
  ) {
    const id = pickStr(root, "id", "profileId") || newId();
    const p = normalizeProfile(root, id);
    if (!profileLooksNonEmpty(p)) throw new Error("Single-profile JSON has no recognizable fields.");
    return { kind: "profiles", profiles: [p], suggestedActiveId: p.id };
  }

  throw new Error(
    "Unrecognized JSON shape. Expected Nova personality.json (with profiles[]) or a single profile object.",
  );
}

export type OpenclawBundle = Partial<Record<"soul" | "identity" | "user" | "journal" | "memory" | "tools", string>>;

function openclawStem(fileName: string): keyof OpenclawBundle | null {
  const base = fileName.replace(/^.*[/\\]/, "").trim().toLowerCase();
  if (!base.endsWith(".md") && !base.endsWith(".markdown")) return null;
  const stem = base.replace(/\.(md|markdown)$/i, "");
  if (stem === "soul" || stem === "identity" || stem === "user" || stem === "journal" || stem === "memory" || stem === "tools") {
    return stem;
  }
  return null;
}

/** Extract first sensible agent name from OpenClaw IDENTITY.md */
export function extractIdentityName(markdown: string): string {
  const m = markdown.match(/^##\s*Name\s*$/im);
  if (m?.index !== undefined) {
    const rest = markdown.slice(m.index + m[0].length);
    const line = rest
      .split("\n")
      .map((l) => l.trim())
      .find((l) => l.length > 0 && !l.startsWith("#"));
    if (line) return line.replace(/^[-*]\s*/, "").trim();
  }
  const lineMatch = markdown.match(/^\s*[-*]?\s*Name:\s*(.+)$/im);
  if (lineMatch?.[1]) return lineMatch[1].trim();
  const h1 = markdown.match(/^#\s+(.+)$/m);
  if (h1?.[1] && !/^identity\.?md$/i.test(h1[1].trim())) return h1[1].trim();
  return "";
}

function extractSection(md: string, heading: string): string {
  const re = new RegExp(`^##\\s*${heading.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*$`, "im");
  const m = md.match(re);
  if (!m?.index) return "";
  const start = m.index + m[0].length;
  const tail = md.slice(start);
  const next = tail.search(/\n##\s+/);
  const block = next === -1 ? tail : tail.slice(0, next);
  return block.trim();
}

function escapeReg(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Remove `## Heading` blocks from markdown (multiline). */
function stripMarkdownHeadings(md: string, headings: string[]): string {
  let out = md;
  for (const h of headings) {
    const re = new RegExp(
      `^##\\s*${escapeReg(h)}\\s*\\n[\\s\\S]*?(?=\\n##\\s|$)`,
      "im",
    );
    out = out.replace(re, "\n");
  }
  return out.replace(/\n{3,}/g, "\n\n").trim();
}

/**
 * Build one {@link PersonalityProfile} from uploaded OpenClaw markdown files (any subset).
 */
export function openclawFilesToProfile(files: { fileName: string; text: string }[]): PersonalityProfile {
  const bundle: OpenclawBundle = {};
  for (const { fileName, text } of files) {
    const stem = openclawStem(fileName);
    if (!stem) continue;
    switch (stem) {
      case "soul":
        bundle.soul = text;
        break;
      case "identity":
        bundle.identity = text;
        break;
      case "user":
        bundle.user = text;
        break;
      case "journal":
        bundle.journal = text;
        break;
      case "memory":
        bundle.memory = text;
        break;
      case "tools":
        bundle.tools = text;
        break;
      default:
        break;
    }
  }

  if (!bundle.soul && !bundle.identity && !bundle.user && !bundle.journal && !bundle.memory && !bundle.tools) {
    throw new Error(
      "No recognized OpenClaw files. Use names like SOUL.md, IDENTITY.md, USER.md, JOURNAL.md, MEMORY.md, TOOLS.md (case-insensitive).",
    );
  }

  const nameFromIdentity = bundle.identity ? extractIdentityName(bundle.identity) : "";
  const visual = bundle.identity ? extractSection(bundle.identity, "Visual Description") : "";
  const vibe = bundle.identity ? extractSection(bundle.identity, "Vibe") : "";

  let corePersonality = "";
  let toneOfVoice = "";
  let coreValues = "";
  if (bundle.soul) {
    toneOfVoice = extractSection(bundle.soul, "Communication Style");
    coreValues = extractSection(bundle.soul, "Values");
    const stripped = stripMarkdownHeadings(bundle.soul, ["Communication Style", "Values"]);
    corePersonality = stripped.trim();
  }

  const backgroundParts: string[] = [];
  if (bundle.identity?.trim()) backgroundParts.push(bundle.identity.trim());
  if (vibe) backgroundParts.push(`## Vibe\n${vibe}`);

  const extraBlocks: string[] = [];
  if (bundle.user?.trim()) {
    extraBlocks.push(
      "## User context (from OpenClaw USER.md)\n\nThe following describes the human you are assisting:\n\n" +
        bundle.user.trim(),
    );
  }
  if (bundle.journal?.trim()) {
    extraBlocks.push("## Journal (from OpenClaw JOURNAL.md)\n\n" + bundle.journal.trim());
  }
  if (bundle.memory?.trim()) {
    extraBlocks.push("## Memory notes (from OpenClaw MEMORY.md)\n\n" + bundle.memory.trim());
  }
  if (bundle.tools?.trim()) {
    extraBlocks.push("## Tools (from OpenClaw TOOLS.md)\n\n" + bundle.tools.trim());
  }

  return {
    id: newId(),
    profileName: `OpenClaw · ${nameFromIdentity || "import"}`,
    companionName: nameFromIdentity || "Companion",
    corePersonality,
    toneOfVoice,
    backgroundStory: backgroundParts.join("\n\n---\n\n").trim(),
    coreValues,
    relationshipStyle:
      "OpenClaw import: USER / JOURNAL / MEMORY / TOOLS text is in special instructions unless you reorganize fields.",
    specialInstructions: extraBlocks.join("\n\n---\n\n").trim(),
    avatarDescription: visual.trim() ? visual.trim() : null,
  };
}

/** Append imported profiles; regenerates ids when they collide with existing ones. */
export function appendImportedProfiles(base: PersonalityFile, incoming: PersonalityProfile[]): PersonalityFile {
  const used = new Set(base.profiles.map((p) => p.id));
  const appended: PersonalityProfile[] = [];
  for (const p of incoming) {
    let id = p.id;
    let profileName = p.profileName;
    if (!id || used.has(id)) {
      id = newId();
      if (!p.profileName.includes("import")) {
        profileName = `${p.profileName} (imported)`;
      }
    }
    used.add(id);
    appended.push({ ...p, id, profileName });
  }
  const profiles = [...base.profiles, ...appended];
  const last = appended[appended.length - 1];
  return {
    ...base,
    profiles,
    activeProfileId: last?.id ?? base.activeProfileId,
  };
}
