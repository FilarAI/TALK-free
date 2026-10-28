import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Settings {
  microphone: string;
  engine: string;
  openaiApiKey: string;
  hotkeyToggle: string;
  hotkeyPtt: string;
  overlayPosition: string;
  overlayAlwaysOnTop: boolean;
  theme: string;
}

interface MicDevice {
  name: string;
  is_default: boolean;
}

const statusDot = document.getElementById("status-dot")!;
const statusText = document.getElementById("status-text")!;
const micSelect = document.getElementById("mic-select") as HTMLSelectElement;
const openaiKey = document.getElementById("openai-key") as HTMLInputElement;
const hotkeyToggleBtn = document.getElementById("hotkey-toggle-btn")!;
const hotkeyPttBtn = document.getElementById("hotkey-ptt-btn")!;
const positionBtns = document.querySelectorAll<HTMLButtonElement>(".pos-btn");
const alwaysOnTopCheck = document.getElementById("always-on-top") as HTMLInputElement;
const themeBtns = document.querySelectorAll<HTMLButtonElement>(".theme-btn");

const navItems = document.querySelectorAll(".nav-item");
const sections = document.querySelectorAll(".content-section");

navItems.forEach((item) => {
  item.addEventListener("click", () => {
    const target = item.getAttribute("data-section");
    navItems.forEach((n) => n.classList.remove("active"));
    sections.forEach((s) => s.classList.remove("active"));
    item.classList.add("active");
    document.getElementById(`section-${target}`)?.classList.add("active");
  });
});

const titlebar = document.getElementById("titlebar")!;
const sidebar = document.getElementById("sidebar")!;
const appWindow = getCurrentWindow();

titlebar.addEventListener("mousedown", (e) => {
  if ((e.target as HTMLElement).closest("button, select, input, a, .nav-item")) return;
  appWindow.startDragging();
});

sidebar.addEventListener("mousedown", (e) => {
  if ((e.target as HTMLElement).closest("button, select, input, a, .nav-item")) return;
  appWindow.startDragging();
});

let currentSettings: Settings;

function formatHotkey(raw: string): string {
  return raw.replace("CmdOrCtrl", "⌘").replace("Shift", "⇧").replace("Alt", "⌥").replace(/\+/g, " ");
}

function codeToTauri(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code === "Space") return "Space";
  if (code === "Backquote") return "`";
  if (code === "Minus") return "-";
  if (code === "Equal") return "=";
  if (code === "BracketLeft") return "[";
  if (code === "BracketRight") return "]";
  if (code === "Backslash") return "\\";
  if (code === "Semicolon") return ";";
  if (code === "Quote") return "'";
  if (code === "Comma") return ",";
  if (code === "Period") return ".";
  if (code === "Slash") return "/";
  if (code.startsWith("F") && !isNaN(Number(code.slice(1)))) return code;
  return code;
}

function captureHotkey(btn: HTMLElement, onCapture: (combo: string) => void) {
  const original = btn.textContent!;
  btn.textContent = "Press keys…";
  btn.classList.add("capturing");

  const held = new Set<string>();

  const MODIFIER_CODES = new Set([
    "MetaLeft", "MetaRight", "ControlLeft", "ControlRight",
    "ShiftLeft", "ShiftRight", "AltLeft", "AltRight",
  ]);

  const cleanup = () => {
    window.removeEventListener("keydown", onDown, true);
    window.removeEventListener("keyup", onUp, true);
  };

  const onDown = (e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      cleanup();
      btn.textContent = original;
      btn.classList.remove("capturing");
      return;
    }

    held.add(e.code);

    const nonMods = [...held].filter(c => !MODIFIER_CODES.has(c));
    if (nonMods.length === 0) return; // still waiting for non-modifier

    const parts: string[] = [];
    const hasCmdCtrl = held.has("MetaLeft") || held.has("MetaRight") || held.has("ControlLeft") || held.has("ControlRight");
    const hasShift   = held.has("ShiftLeft") || held.has("ShiftRight");
    const hasAlt     = held.has("AltLeft") || held.has("AltRight");

    if (hasCmdCtrl) parts.push("CmdOrCtrl");
    if (hasShift)   parts.push("Shift");
    if (hasAlt)     parts.push("Alt");
    nonMods.forEach(c => parts.push(codeToTauri(c)));

    const combo = parts.join("+");
    btn.textContent = formatHotkey(combo);
    btn.classList.remove("capturing");
    cleanup();
    onCapture(combo);
  };

  const onUp = (e: KeyboardEvent) => {
    held.delete(e.code);
  };

  window.addEventListener("keydown", onDown, true);
  window.addEventListener("keyup", onUp, true);
}

async function loadSettings() {
  currentSettings = await invoke<Settings>("get_settings");

  const mics = await invoke<MicDevice[]>("list_microphones");
  micSelect.innerHTML = "";
  mics.forEach((mic) => {
    const option = document.createElement("option");
    option.value = mic.name;
    option.textContent = mic.name + (mic.is_default ? " (default)" : "");
    micSelect.appendChild(option);
  });
  micSelect.value = currentSettings.microphone;

  openaiKey.value = currentSettings.openaiApiKey;
  hotkeyToggleBtn.textContent = formatHotkey(currentSettings.hotkeyToggle);
  hotkeyPttBtn.textContent = formatHotkey(currentSettings.hotkeyPtt);
  setActivePosition(currentSettings.overlayPosition);
  alwaysOnTopCheck.checked = currentSettings.overlayAlwaysOnTop;
  applyTheme(currentSettings.theme);
}

function setActivePosition(pos: string) {
  positionBtns.forEach(btn => btn.classList.toggle("active", btn.dataset.pos === pos));
}

function applyTheme(theme: string) {
  document.documentElement.setAttribute("data-theme", theme);
  themeBtns.forEach(btn => btn.classList.toggle("active", btn.dataset.theme === theme));
}

async function saveSettings() {
  currentSettings.microphone = micSelect.value;
  currentSettings.openaiApiKey = openaiKey.value;
  await invoke("save_settings", { settings: currentSettings });
}

async function saveAndReloadShortcuts() {
  await saveSettings();
  await invoke("update_shortcuts");
}

micSelect.addEventListener("change", () => saveSettings());
openaiKey.addEventListener("change", () => saveSettings());

positionBtns.forEach(btn => {
  btn.addEventListener("click", async () => {
    const pos = btn.dataset.pos!;
    currentSettings.overlayPosition = pos;
    setActivePosition(pos);
    await saveSettings();
    await invoke("update_overlay_position");
  });
});

alwaysOnTopCheck.addEventListener("change", async () => {
  currentSettings.overlayAlwaysOnTop = alwaysOnTopCheck.checked;
  await saveSettings();
  await invoke("update_overlay_position");
});

themeBtns.forEach(btn => {
  btn.addEventListener("click", async () => {
    const theme = btn.dataset.theme!;
    currentSettings.theme = theme;
    applyTheme(theme);
    await saveSettings();
  });
});

hotkeyToggleBtn.addEventListener("click", () => {
  captureHotkey(hotkeyToggleBtn, async (combo) => {
    currentSettings.hotkeyToggle = combo;
    await saveAndReloadShortcuts();
  });
});

hotkeyPttBtn.addEventListener("click", () => {
  captureHotkey(hotkeyPttBtn, async (combo) => {
    currentSettings.hotkeyPtt = combo;
    await saveAndReloadShortcuts();
  });
});

listen<string>("recording-state", (event) => {
  const state = event.payload;
  statusDot.className = "";
  if (state === "Recording") {
    statusDot.classList.add("recording");
    statusText.textContent = "Recording...";
  } else if (state === "Transcribing") {
    statusDot.classList.add("transcribing");
    statusText.textContent = "Transcribing...";
  } else {
    statusDot.classList.add("ready");
    statusText.textContent = "Ready";
  }
});

loadSettings();
