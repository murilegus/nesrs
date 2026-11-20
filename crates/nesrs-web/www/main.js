import init, { Emulator } from "./pkg/nesrs_web.js";

const FRAME_MS = 1000 / 60.0988;
const MAX_CATCH_UP_FRAMES = 3;
const VOLUME = 0.8;
const TOAST_MS = 1800;
const BATTERY_FLUSH_FRAMES = 600;

const BUTTON = { A: 1, B: 2, SELECT: 4, START: 8, UP: 16, DOWN: 32, LEFT: 64, RIGHT: 128 };
const KEYS = {
  KeyX: BUTTON.A,
  KeyZ: BUTTON.B,
  ShiftRight: BUTTON.SELECT,
  Tab: BUTTON.SELECT,
  Enter: BUTTON.START,
  ArrowUp: BUTTON.UP,
  ArrowDown: BUTTON.DOWN,
  ArrowLeft: BUTTON.LEFT,
  ArrowRight: BUTTON.RIGHT,
};

const $ = (id) => document.getElementById(id);
const canvas = $("canvas");
const context = canvas.getContext("2d");
const image = context.createImageData(canvas.width, canvas.height);
const screen = $("screen");
const overlay = $("overlay");
const toastElement = $("toast");
const slotSelect = $("slot");

let emulator = null;
let romKey = "";
let keyboardButtons = 0;
let touchButtons = 0;
let paused = false;
let muted = false;
let frames = 0;
let lastTime = 0;
let backlog = 0;
let savedRam = "";
let toastTimer = 0;

// --- Messages ---------------------------------------------------------------

function toast(message, isError = false) {
  toastElement.textContent = message;
  toastElement.classList.toggle("error", isError);
  toastElement.classList.add("visible");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastElement.classList.remove("visible"), TOAST_MS);
}

// --- Storage ----------------------------------------------------------------
// Saves live in localStorage, keyed by the ROM's name and size.

function encode(bytes) {
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(text);
}

function decode(text) {
  return Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
}

function store(name, value) {
  try {
    localStorage.setItem(`nesrs:${romKey}:${name}`, value);
    return true;
  } catch {
    return false; // storage full or blocked
  }
}

function fetchStored(name) {
  try {
    return localStorage.getItem(`nesrs:${romKey}:${name}`);
  } catch {
    return null;
  }
}

function flushBattery() {
  const ram = emulator?.save_ram();
  if (!ram) return;
  const encoded = encode(ram);
  if (encoded !== savedRam && store("sav", encoded)) {
    savedRam = encoded;
  }
}

function saveState() {
  if (!emulator) return;
  const slot = slotSelect.value;
  if (store(`state${slot}`, encode(emulator.save_state()))) {
    toast(`State ${slot} saved`);
  } else {
    toast("Cannot save state: browser storage is unavailable", true);
  }
}

function loadState() {
  if (!emulator) return;
  const slot = slotSelect.value;
  const stored = fetchStored(`state${slot}`);
  if (!stored) {
    toast(`State ${slot} is empty`);
    return;
  }
  try {
    emulator.load_state(decode(stored));
    toast(`State ${slot} loaded`);
  } catch (error) {
    toast(`Cannot load state: ${error.message}`, true);
  }
}

// --- Audio ------------------------------------------------------------------

const audio = {
  context: null,
  node: null,
  gain: null,
  queued: 0,
  starting: null,

  // Browsers only allow sound after a user gesture, so this is called from
  // input handlers and is safe to call repeatedly.
  start() {
    if (this.context) {
      if (this.context.state === "suspended") this.context.resume();
      return this.starting;
    }
    const AudioContextClass = window.AudioContext ?? window.webkitAudioContext;
    if (!AudioContextClass || !window.AudioWorkletNode) return Promise.resolve();
    this.context = new AudioContextClass({ latencyHint: "interactive" });
    this.starting = this.context.audioWorklet
      .addModule("audio-worklet.js")
      .then(() => {
        this.node = new AudioWorkletNode(this.context, "nes-audio", { outputChannelCount: [2] });
        this.node.port.onmessage = (event) => (this.queued = event.data);
        this.gain = this.context.createGain();
        this.gain.gain.value = muted ? 0 : VOLUME;
        this.node.connect(this.gain).connect(this.context.destination);
        emulator?.set_sample_rate(this.context.sampleRate);
      })
      .catch((error) => console.error("nesrs: sound disabled:", error));
    return this.starting;
  },

  get running() {
    return this.node !== null && this.context.state === "running";
  },

  // The sound card and the display run off different clocks. Nudging the
  // generated rate by up to 0.5% keeps the queue near 60 ms.
  steer() {
    const rate = this.context.sampleRate;
    const target = rate * 0.06;
    const error = Math.max(-1, Math.min(1, (target - this.queued) / target));
    emulator.set_sample_rate(Math.round(rate * (1 + error * 0.005)));
  },

  push(samples) {
    this.queued += samples.length;
    this.node.port.postMessage(samples, [samples.buffer]);
  },

  setMuted(value) {
    if (this.gain) this.gain.gain.value = value ? 0 : VOLUME;
  },
};

// --- Emulation loop ---------------------------------------------------------

function runFrame() {
  emulator.set_buttons(0, keyboardButtons | touchButtons);
  if (audio.running) audio.steer();
  emulator.run_frame();
  const samples = emulator.take_audio();
  if (audio.running) audio.push(samples);
  if (++frames % BATTERY_FLUSH_FRAMES === 0) flushBattery();
}

function draw() {
  image.data.set(emulator.frame());
  context.putImageData(image, 0, 0);
}

function tick(now) {
  requestAnimationFrame(tick);
  const elapsed = now - lastTime;
  lastTime = now;
  if (!emulator || paused) return;
  // Displays refresh at many rates; emulate as many NES frames as real time
  // calls for, but never try to make up for a long stall.
  backlog = Math.min(backlog + elapsed, FRAME_MS * MAX_CATCH_UP_FRAMES);
  let ran = false;
  while (backlog >= FRAME_MS) {
    runFrame();
    backlog -= FRAME_MS;
    ran = true;
  }
  if (ran) draw();
}

// --- Loading ROMs -----------------------------------------------------------

function loadRom(bytes, name) {
  let next;
  try {
    next = new Emulator(bytes);
  } catch (error) {
    toast(`Cannot open ${name}: ${error.message}`, true);
    return;
  }
  flushBattery();
  emulator?.free();
  emulator = next;
  romKey = `${name}:${bytes.length}`;
  savedRam = fetchStored("sav") ?? "";
  if (savedRam) emulator.load_save_ram(decode(savedRam));
  if (audio.context) emulator.set_sample_rate(audio.context.sampleRate);
  frames = 0;
  backlog = 0;
  setPaused(false);
  overlay.hidden = true;
  document.title = `${name.replace(/\.nes$/i, "")} - nesrs`;
  for (const id of ["pause", "reset", "save", "load"]) $(id).disabled = false;
}

async function loadFile(file) {
  if (!file) return;
  audio.start();
  loadRom(new Uint8Array(await file.arrayBuffer()), file.name);
}

async function loadUrl(url) {
  try {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const name = new URL(url, location.href).pathname.split("/").pop() || "rom.nes";
    loadRom(new Uint8Array(await response.arrayBuffer()), name);
  } catch (error) {
    toast(`Cannot fetch ${url}: ${error.message}`, true);
  }
}

// --- Controls ---------------------------------------------------------------

function setPaused(value) {
  paused = value;
  $("pause").textContent = paused ? "Resume" : "Pause";
  if (paused) toast("Paused");
}

function toggleMute() {
  muted = !muted;
  audio.setMuted(muted);
  $("mute").textContent = muted ? "Unmute" : "Mute";
  toast(muted ? "Sound off" : "Sound on");
}

function reset() {
  emulator?.reset();
  if (emulator) toast("Reset");
}

function selectSlot(slot) {
  slotSelect.value = String(slot);
  toast(`State slot ${slot}`);
}

function toggleFullscreen() {
  if (document.fullscreenElement) document.exitFullscreen();
  else screen.requestFullscreen?.();
}

window.addEventListener("keydown", (event) => {
  if (event.ctrlKey || event.metaKey || event.altKey) return;
  audio.start();
  const button = KEYS[event.code];
  if (button !== undefined) {
    keyboardButtons |= button;
    event.preventDefault();
    return;
  }
  if (event.repeat) return;
  if (/^Digit\d$/.test(event.code)) {
    selectSlot(event.code.slice(5));
  } else if (event.code === "F5") {
    saveState();
  } else if (event.code === "F8") {
    loadState();
  } else if (event.code === "KeyP") {
    if (emulator) setPaused(!paused);
  } else if (event.code === "KeyR") {
    reset();
  } else if (event.code === "KeyM") {
    toggleMute();
  } else {
    return;
  }
  event.preventDefault();
});

window.addEventListener("keyup", (event) => {
  const button = KEYS[event.code];
  if (button !== undefined) keyboardButtons &= ~button;
});

// Key releases are not delivered while the page is unfocused.
window.addEventListener("blur", () => {
  keyboardButtons = 0;
  touchButtons = 0;
  for (const held of document.querySelectorAll("#touch .held")) held.classList.remove("held");
});

for (const element of document.querySelectorAll("#touch [data-button]")) {
  const mask = Number(element.dataset.button);
  const set = (held) => (event) => {
    event.preventDefault();
    audio.start();
    touchButtons = held ? touchButtons | mask : touchButtons & ~mask;
    element.classList.toggle("held", held);
  };
  element.addEventListener("pointerdown", set(true));
  for (const type of ["pointerup", "pointercancel", "pointerleave"]) {
    element.addEventListener(type, set(false));
  }
  element.addEventListener("contextmenu", (event) => event.preventDefault());
}

$("rom-input").addEventListener("change", (event) => {
  loadFile(event.target.files[0]);
  event.target.value = ""; // allow picking the same file again
});
$("pause").addEventListener("click", () => setPaused(!paused));
$("reset").addEventListener("click", reset);
$("save").addEventListener("click", saveState);
$("load").addEventListener("click", loadState);
$("mute").addEventListener("click", toggleMute);
$("fullscreen").addEventListener("click", toggleFullscreen);
canvas.addEventListener("dblclick", toggleFullscreen);
// Clicking a toolbar button should not leave it focused, or Enter (Start)
// would press it again.
for (const control of document.querySelectorAll("header button, header select")) {
  control.addEventListener("click", () => control.blur());
  control.addEventListener("change", () => control.blur());
}

screen.addEventListener("dragover", (event) => {
  event.preventDefault();
  screen.classList.add("dragging");
});
screen.addEventListener("dragleave", () => screen.classList.remove("dragging"));
screen.addEventListener("drop", (event) => {
  event.preventDefault();
  screen.classList.remove("dragging");
  loadFile(event.dataTransfer.files[0]);
});

// Save battery RAM before the page goes away.
document.addEventListener("visibilitychange", () => {
  if (document.hidden) flushBattery();
});
window.addEventListener("pagehide", flushBattery);

// --- Start ------------------------------------------------------------------

for (let slot = 0; slot <= 9; slot++) {
  slotSelect.add(new Option(String(slot), String(slot), slot === 1, slot === 1));
}

await init();
requestAnimationFrame((now) => {
  lastTime = now;
  requestAnimationFrame(tick);
});

// `?rom=<url>` loads a ROM hosted alongside the page.
const romUrl = new URLSearchParams(location.search).get("rom");
if (romUrl) loadUrl(romUrl);
