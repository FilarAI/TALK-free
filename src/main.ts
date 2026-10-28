import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-shell";

interface Settings {
  microphone: string;
  engine: string;
  language: string;
  secondaryLanguage: string;
  hotkeyToggle: string;
  hotkeyPtt: string;
  overlayPosition: string;
  overlayAlwaysOnTop: boolean;
  theme: string;
  autoPaste: boolean;
}

interface MicDevice {
  name: string;
  is_default: boolean;
}

type Provider = "groq" | "openai";
// Backend only ever returns "…last4" hints, never the keys
type KeyStatus = Record<Provider, string | null>;

const PROVIDERS: Record<Provider, { name: string; url: string; placeholder: string }> = {
  groq: { name: "Groq", url: "https://console.groq.com/keys", placeholder: "Wklej klucz gsk_…" },
  openai: { name: "OpenAI", url: "https://platform.openai.com/api-keys", placeholder: "Wklej klucz sk-…" },
};

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const statusDot = $("status-dot");
const statusText = $("status-text");
const micSelect = $<HTMLSelectElement>("mic-select");
const micHint = $("mic-hint");
const languageSelect = $<HTMLSelectElement>("language-select");
const secondaryLanguageSelect = $<HTMLSelectElement>("secondary-language-select");
const alwaysOnTopCheck = $<HTMLInputElement>("always-on-top");
const autoPasteCheck = $<HTMLInputElement>("auto-paste");
const hotkeyToggleBtn = $("hotkey-toggle-btn");
const hotkeyPttBtn = $("hotkey-ptt-btn");
const keyBanner = $("key-banner");
const keyBannerText = $("key-banner-text");
const keyBannerLink = $("key-banner-link");
const engineBadge = $("engine-badge");
const saveBar = $("save-bar");
const saveBtn = $<HTMLButtonElement>("save-btn");
const discardBtn = $("discard-btn");
const toast = $("toast");
const positionBtns = document.querySelectorAll<HTMLButtonElement>(".pos-btn");
const themeBtns = document.querySelectorAll<HTMLButtonElement>(".theme-btn");
const engineBtns = document.querySelectorAll<HTMLButtonElement>(".engine-btn");
const keyRows = document.querySelectorAll<HTMLElement>(".key-row");

// saved = what the backend has; draft = what the user sees and edits
let saved: Settings;
let draft: Settings;
let keyStatus: KeyStatus = { groq: null, openai: null };
// Typed but unsaved keys; "" means "delete on save"
let pendingKeys: Partial<Record<Provider, string>> = {};
const editingKeys = new Set<Provider>();
let capturing = false;
let saving = false;

const clone = (s: Settings): Settings => JSON.parse(JSON.stringify(s));
const activeProvider = (engine: string): Provider => (engine === "groq" ? "groq" : "openai"); // legacy "cloud" = OpenAI

// ── Navigation & window dragging ──────────────────────

const navItems = document.querySelectorAll<HTMLElement>(".nav-item");
const sections = document.querySelectorAll(".content-section");

function showSection(name: string) {
  navItems.forEach((n) => n.classList.toggle("active", n.dataset.section === name));
  sections.forEach((s) => s.classList.toggle("active", s.id === `section-${name}`));
}

navItems.forEach((item) => {
  item.addEventListener("click", (e) => {
    e.preventDefault();
    showSection(item.dataset.section!);
  });
});

const appWindow = getCurrentWindow();
for (const el of [$("titlebar"), $("sidebar")]) {
  el.addEventListener("mousedown", (e) => {
    if ((e.target as HTMLElement).closest("button, select, input, a, label")) return;
    appWindow.startDragging();
  });
}

// ── Rendering ─────────────────────────────────────────

function hotkeyParts(raw: string): string[] {
  const names: Record<string, string> = { CmdOrCtrl: "⌘", Shift: "⇧", Alt: "⌥", Space: "Spacja" };
  return raw.split("+").map((k) => names[k] ?? k);
}

function renderHotkey(btn: HTMLElement, raw: string) {
  btn.replaceChildren(
    ...hotkeyParts(raw).map((k) => {
      const kbd = document.createElement("kbd");
      kbd.textContent = k;
      return kbd;
    }),
  );
  btn.setAttribute("aria-label", `Skrót: ${hotkeyParts(raw).join(" ")}. Kliknij, aby zmienić`);
}

function setRadio(btns: NodeListOf<HTMLButtonElement>, attr: string, value: string) {
  btns.forEach((b) => {
    const on = b.dataset[attr] === value;
    b.classList.toggle("active", on);
    b.setAttribute("aria-checked", String(on));
  });
}

function button(label: string, onClick: () => void, extra = ""): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = `small-btn ${extra}`.trim();
  b.textContent = label;
  b.addEventListener("click", onClick);
  return b;
}

function renderKey(row: HTMLElement) {
  const p = row.dataset.provider as Provider;
  const control = row.querySelector<HTMLElement>(".key-control")!;
  const pending = pendingKeys[p];
  const hint = keyStatus[p];

  if (pending === "" && hint) {
    const chip = document.createElement("span");
    chip.className = "key-saved pending-delete";
    chip.innerHTML = `<span class="dot"></span>Zostanie usunięty`;
    control.replaceChildren(chip, button("Cofnij", () => { delete pendingKeys[p]; render(); }));
    return;
  }

  if (hint && !editingKeys.has(p)) {
    const chip = document.createElement("span");
    chip.className = "key-saved";
    const mask = document.createElement("span");
    mask.className = "mask";
    mask.textContent = hint;
    chip.append(Object.assign(document.createElement("span"), { className: "dot" }), "Zapisany", mask);
    control.replaceChildren(
      chip,
      button("Zmień", () => { editingKeys.add(p); render(); control.querySelector("input")?.focus(); }),
      button("Usuń", () => { pendingKeys[p] = ""; render(); }, "danger"),
    );
    return;
  }

  const input = document.createElement("input");
  input.type = "password";
  input.placeholder = PROVIDERS[p].placeholder;
  input.autocomplete = "off";
  input.spellcheck = false;
  input.value = pending ?? "";
  input.setAttribute("aria-label", `Klucz ${PROVIDERS[p].name}`);
  input.addEventListener("input", () => {
    const v = input.value.trim();
    if (v) pendingKeys[p] = v;
    else delete pendingKeys[p];
    renderBanner();
    renderSaveBar();
  });
  const children: HTMLElement[] = [input];
  if (hint) children.push(button("Anuluj", () => { editingKeys.delete(p); delete pendingKeys[p]; render(); }));
  control.replaceChildren(...children);
}

function hasKey(p: Provider): boolean {
  const pending = pendingKeys[p];
  return pending !== undefined ? pending !== "" : !!keyStatus[p];
}

function renderBanner() {
  const p = activeProvider(draft.engine);
  const missing = !hasKey(p);
  keyBanner.hidden = !missing;
  engineBadge.hidden = !missing;
  keyBannerText.textContent = `Brak klucza ${PROVIDERS[p].name}. Bez niego dyktowanie nie zadziała.`;
  keyBannerLink.textContent = p === "groq" ? "Pobierz darmowy klucz →" : "Pobierz klucz →";
}

function isDirty(): boolean {
  return JSON.stringify(draft) !== JSON.stringify(saved) || Object.keys(pendingKeys).length > 0;
}

function renderSaveBar() {
  const dirty = isDirty();
  saveBar.hidden = !dirty;
  if (dirty) toast.hidden = true;
}

function applyTheme(theme: string) {
  document.documentElement.setAttribute("data-theme", theme);
}

function render() {
  micSelect.value = draft.microphone;
  languageSelect.value = draft.language;
  secondaryLanguageSelect.value = draft.secondaryLanguage;
  alwaysOnTopCheck.checked = draft.overlayAlwaysOnTop;
  autoPasteCheck.checked = draft.autoPaste;
  setRadio(themeBtns, "theme", draft.theme);
  setRadio(engineBtns, "engine", activeProvider(draft.engine));
  positionBtns.forEach((b) => b.classList.toggle("active", b.dataset.pos === draft.overlayPosition));
  if (!capturing) {
    renderHotkey(hotkeyToggleBtn, draft.hotkeyToggle);
    renderHotkey(hotkeyPttBtn, draft.hotkeyPtt);
  }
  keyRows.forEach(renderKey);
  applyTheme(draft.theme);
  renderBanner();
  renderSaveBar();
}

// ── Toast & status ────────────────────────────────────

let toastTimer: number | undefined;

function showToast(message: string, isError = false) {
  clearTimeout(toastTimer);
  const span = document.createElement("span");
  span.textContent = message;
  toast.replaceChildren(span);
  toast.classList.toggle("error", isError);
  toast.hidden = false;
  toastTimer = window.setTimeout(() => (toast.hidden = true), isError ? 6000 : 2000);
}

function humanError(e: string): string {
  if (/API key not set/i.test(e)) return "Brak klucza API. Dodaj go w zakładce Silnik.";
  if (/\(401/.test(e)) return "Nieprawidłowy klucz API. Sprawdź go w zakładce Silnik.";
  if (/\(429/.test(e)) return "Przekroczony limit zapytań. Spróbuj za chwilę.";
  if (/request failed/i.test(e)) return "Brak połączenia z internetem.";
  if (/no audio/i.test(e)) return "Nagranie było za krótkie.";
  if (/not found|no default input/i.test(e)) return "Nie znaleziono mikrofonu. Wybierz inny w zakładce Ogólne.";
  return e;
}

let statusTimer: number | undefined;

function setStatus(kind: "ready" | "recording" | "transcribing" | "error", text: string) {
  clearTimeout(statusTimer);
  statusDot.className = kind;
  statusText.textContent = text;
  if (kind === "error") statusTimer = window.setTimeout(() => setStatus("ready", "Gotowy"), 6000);
}

listen<string>("recording-state", (event) => {
  const state = event.payload;
  if (state === "Recording") setStatus("recording", "Nagrywam…");
  else if (state === "Transcribing") setStatus("transcribing", "Zamieniam na tekst…");
  else if (statusDot.className !== "error") setStatus("ready", "Gotowy");
});

listen<string>("recording-error", (event) => {
  const message = humanError(event.payload);
  setStatus("error", "Błąd");
  showToast(message, true);
});

// ── Save / discard ────────────────────────────────────

async function save() {
  if (saving || !isDirty()) return;
  saving = true;
  saveBtn.disabled = true;
  try {
    for (const [p, key] of Object.entries(pendingKeys) as [Provider, string][]) {
      await invoke("set_api_key", { provider: p, key });
    }
    const hotkeysChanged = draft.hotkeyToggle !== saved.hotkeyToggle || draft.hotkeyPtt !== saved.hotkeyPtt;
    const overlayChanged =
      draft.overlayPosition !== saved.overlayPosition || draft.overlayAlwaysOnTop !== saved.overlayAlwaysOnTop;

    await invoke("save_settings", { settings: draft });
    saved = clone(draft);
    pendingKeys = {};
    editingKeys.clear();
    keyStatus = await invoke<KeyStatus>("get_key_status");

    if (overlayChanged) await invoke("update_overlay_position");
    let shortcutError = "";
    if (hotkeysChanged) {
      try {
        await invoke("update_shortcuts");
      } catch (e) {
        shortcutError = String(e);
      }
    }
    render();
    if (shortcutError) showToast("Zapisano, ale skrót jest zajęty przez inną aplikację. Wybierz inny.", true);
    else showToast("Zapisano");
  } catch (e) {
    showToast(`Nie udało się zapisać: ${e}`, true);
  } finally {
    saving = false;
    saveBtn.disabled = false;
  }
}

function discard() {
  draft = clone(saved);
  pendingKeys = {};
  editingKeys.clear();
  render();
}

saveBtn.addEventListener("click", save);
discardBtn.addEventListener("click", discard);
window.addEventListener("keydown", (e) => {
  if (capturing) return;
  if (e.metaKey && e.key.toLowerCase() === "s") {
    e.preventDefault();
    save();
  }
});

// ── Inputs → draft ────────────────────────────────────

function edit(change: (d: Settings) => void) {
  change(draft);
  render();
}

micSelect.addEventListener("change", () => edit((d) => (d.microphone = micSelect.value)));
languageSelect.addEventListener("change", () => edit((d) => (d.language = languageSelect.value)));
secondaryLanguageSelect.addEventListener("change", () =>
  edit((d) => (d.secondaryLanguage = secondaryLanguageSelect.value)),
);
alwaysOnTopCheck.addEventListener("change", () => edit((d) => (d.overlayAlwaysOnTop = alwaysOnTopCheck.checked)));
autoPasteCheck.addEventListener("change", () => edit((d) => (d.autoPaste = autoPasteCheck.checked)));
themeBtns.forEach((b) => b.addEventListener("click", () => edit((d) => (d.theme = b.dataset.theme!))));
engineBtns.forEach((b) => b.addEventListener("click", () => edit((d) => (d.engine = b.dataset.engine!))));
positionBtns.forEach((b) => b.addEventListener("click", () => edit((d) => (d.overlayPosition = b.dataset.pos!))));
keyBannerLink.addEventListener("click", () => open(PROVIDERS[activeProvider(draft.engine)].url));

// ── Hotkey capture ────────────────────────────────────

function codeToTauri(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  const map: Record<string, string> = {
    Backquote: "`", Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\",
    Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/",
  };
  return map[code] ?? code;
}

const MODIFIER_CODES = new Set([
  "MetaLeft", "MetaRight", "ControlLeft", "ControlRight", "ShiftLeft", "ShiftRight", "AltLeft", "AltRight",
]);

function captureHotkey(btn: HTMLElement, onCapture: (combo: string) => void) {
  if (capturing) return;
  capturing = true;
  btn.textContent = "Naciśnij skrót…";
  btn.classList.add("capturing");
  const held = new Set<string>();

  const finish = () => {
    window.removeEventListener("keydown", onDown, true);
    window.removeEventListener("keyup", onUp, true);
    btn.classList.remove("capturing");
    capturing = false;
  };

  const onDown = (e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      finish();
      render();
      return;
    }
    held.add(e.code);
    const nonMods = [...held].filter((c) => !MODIFIER_CODES.has(c));
    if (nonMods.length === 0) return; // still waiting for a non-modifier key

    const parts: string[] = [];
    if (held.has("MetaLeft") || held.has("MetaRight") || held.has("ControlLeft") || held.has("ControlRight")) parts.push("CmdOrCtrl");
    if (held.has("ShiftLeft") || held.has("ShiftRight")) parts.push("Shift");
    if (held.has("AltLeft") || held.has("AltRight")) parts.push("Alt");
    nonMods.forEach((c) => parts.push(codeToTauri(c)));

    finish();
    onCapture(parts.join("+"));
  };

  const onUp = (e: KeyboardEvent) => held.delete(e.code);

  window.addEventListener("keydown", onDown, true);
  window.addEventListener("keyup", onUp, true);
}

hotkeyToggleBtn.addEventListener("click", () =>
  captureHotkey(hotkeyToggleBtn, (combo) => edit((d) => (d.hotkeyToggle = combo))),
);
hotkeyPttBtn.addEventListener("click", () =>
  captureHotkey(hotkeyPttBtn, (combo) => edit((d) => (d.hotkeyPtt = combo))),
);

// ── Load ──────────────────────────────────────────────

async function load() {
  saved = await invoke<Settings>("get_settings");
  draft = clone(saved);
  keyStatus = await invoke<KeyStatus>("get_key_status");

  const mics = await invoke<MicDevice[]>("list_microphones");
  micSelect.replaceChildren();
  if (mics.length === 0) {
    micSelect.append(new Option("Nie znaleziono mikrofonu", draft.microphone));
    micSelect.disabled = true;
    micHint.textContent = "Podłącz mikrofon i uruchom aplikację ponownie";
    micHint.classList.add("error");
  } else {
    micSelect.append(new Option("Domyślny systemowy", "default"));
    mics.forEach((m) => micSelect.append(new Option(m.name, m.name)));
    if (draft.microphone !== "default" && !mics.some((m) => m.name === draft.microphone)) {
      micSelect.append(new Option(`${draft.microphone} (niepodłączony)`, draft.microphone));
    }
  }

  render();
}

load();
