"use strict";

// Tauri bridge with plain-browser mock fallback. In mock mode (opening
// index.html directly) commands log to console and mutate a local copy.
// Inside the Tauri webview the real bridge is required: mock data must
// never mask a broken backend, or the status card sticks on Loading.
function tauriInvokeFn() {
  const t = window.__TAURI__;
  if (t && t.core && typeof t.core.invoke === "function") {
    return (cmd, args) => t.core.invoke(cmd, args || {});
  }
  if (t && typeof t.invoke === "function") {
    return (cmd, args) => t.invoke(cmd, args || {});
  }
  return null;
}

function runningInTauriWebview() {
  const p = window.location.protocol;
  return p === "tauri:" || p === "https:" && window.location.hostname.endsWith("tauri.localhost");
}

const tauriInvoke = tauriInvokeFn();

const mockState = {
  preset: "brown",
  volume: 0.02,
  carrier_hz: 200,
  keepalive_mode: "continuous",
  pulse_interval_sec: 55,
  playing: true,
  autoplay: true,
  launch_at_startup: false,
  check_for_updates: true,
  device_name: "WH-1000XM4",
  status: "playing",
  version: "2.0.0",
  update: null,
  update_error: null,
};

async function invokeWith(cmd, args, ms) {
  if (tauriInvoke) {
    return withTimeout(tauriInvoke(cmd, args || {}), ms, cmd);
  }
  if (runningInTauriWebview()) {
    throw new Error("Tauri bridge missing for " + cmd);
  }
  console.log("[mock] invoke", cmd, args || {});
  await new Promise((r) => setTimeout(r, 10));
  const s = mockState;
  switch (cmd) {
    case "get_state": break;
    case "set_preset": s.preset = args.preset; break;
    case "set_volume": s.volume = args.volume; break;
    case "set_pulse": s.keepalive_mode = args.enabled ? "pulse" : "continuous"; break;
    case "set_carrier": s.carrier_hz = args.carrier_hz; break;
    case "set_playing":
      s.playing = args.playing;
      s.status = args.playing ? "playing" : "paused";
      break;
    case "set_startup": s.launch_at_startup = args.enabled; break;
    case "set_autoplay": s.autoplay = args.enabled; break;
    case "set_check_updates": s.check_for_updates = args.enabled; break;
    case "reset_settings":
      Object.assign(s, {
        preset: "brown", volume: 0.02, carrier_hz: 200,
        keepalive_mode: "continuous", playing: true, autoplay: true,
        launch_at_startup: false, check_for_updates: true,
        status: "playing", update: null,
      });
      break;
    case "check_updates":
      s.update = { version: "v2.1.0", notes: "Fixes and improvements." };
      s.update_error = null;
      break;
    case "install_update":
      s.version = (s.update && s.update.version.replace(/^v/, "")) || s.version;
      s.update = null;
      s.update_error = null;
      break;
    case "open_logs": return { ok: true };
    default: return { ok: false, error: "unknown command: " + cmd };
  }
  return { ok: true, state: structuredClone(s) };
}

async function invoke(cmd, args) {
  return invokeWith(cmd, args, 8000);
}

// Log-volume mapping: slider 0..1000 <-> gain = 0.0001 * (10000 ^ (pos/1000)).
// 0.01% sits at pos 0, 100% at pos 1000, 2% near pos 575.
function sliderToGain(pos) {
  return 0.0001 * Math.pow(10000, pos / 1000);
}
function gainToSlider(gain) {
  const g = Math.min(1, Math.max(0.0001, gain));
  return Math.round(1000 * Math.log(g / 0.0001) / Math.log(10000));
}
function formatPercent(gain) {
  const pct = gain * 100;
  if (pct < 1) return String(Math.round(pct * 100) / 100);
  return String(Math.round(pct * 10) / 10);
}

const PRESET_NAMES = {
  white: "white", pink: "pink", brown: "brown", blue: "blue",
  violet: "violet", binaural40: "40 Hz binaural",
  binaural10: "10 Hz binaural", binaural6: "6 Hz binaural",
};

const el = (id) => document.getElementById(id);
let state = null;
let checking = false;
let installing = false;

function withTimeout(promise, ms, cmd) {
  let timer = null;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(cmd + " timed out")), ms);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

function bootFailed(msg) {
  el("status-text").textContent = "Could not load state.";
  showError(msg);
}

if (typeof window !== "undefined") {
  window.addEventListener("error", (e) => {
    if (!state) bootFailed(String((e && e.message) || e));
  });
}

function showError(msg) {
  el("error-text").textContent = msg;
  el("error-line").hidden = false;
}
function clearError() {
  el("error-line").hidden = true;
  el("error-text").textContent = "";
}

function applyEnvelope(res) {
  if (res && res.ok) {
    clearError();
    if (res.state) {
      state = res.state;
      render();
    }
    return true;
  }
  showError((res && res.error) || "Command failed");
  return false;
}

async function run(cmd, args) {
  try {
    const res = await invoke(cmd, args);
    applyEnvelope(res);
  } catch (e) {
    showError(String((e && e.message) || e));
  }
}

function render() {
  if (!state) return;
  const status = state.status || (state.playing ? "playing" : "paused");
  const dot = el("status-dot");
  dot.className = "dot " + (status === "playing" ? "playing" : status === "error" ? "error" : "paused");
  const label = status === "playing" ? "Playing" : status === "error" ? "Error" : "Paused";
  const pct = formatPercent(state.volume);
  const kind = state.keepalive_mode === "pulse" ? "pulse" : "noise";
  const what = (PRESET_NAMES[state.preset] || state.preset) + " " + kind + " " + pct + "%";
  el("status-text").textContent = state.device_name
    ? label + " " + what + " on " + state.device_name
    : label + " " + what;

  const hasUpdate = !!state.update;
  el("update-banner").hidden = !hasUpdate;
  if (!hasUpdate) installing = false;
  if (hasUpdate) {
    el("update-text").textContent = "Update " + state.update.version + " available";
    const notes = state.update.notes || "";
    el("update-details").hidden = !notes;
    if (notes) el("update-notes").textContent = notes;
    const status = el("update-status");
    if (installing) {
      status.hidden = false;
      status.textContent = "Installing " + state.update.version + ". The app will restart.";
    } else if (state.update_error) {
      status.hidden = false;
      status.textContent = "Update failed: " + state.update_error;
    } else {
      status.hidden = true;
      status.textContent = "";
    }
    const installBtn = el("banner-install");
    installBtn.disabled = installing;
    installBtn.textContent = installing ? "Installing…" : "Install update";
  }

  document.querySelectorAll(".preset[data-preset]").forEach((b) => {
    b.setAttribute("aria-pressed", String(b.dataset.preset === state.preset));
  });
  el("pulse-preset").setAttribute("aria-pressed", String(state.keepalive_mode === "pulse"));

  const pos = gainToSlider(state.volume);
  const slider = el("volume-slider");
  if (document.activeElement !== slider) slider.value = String(pos);
  el("volume-label").textContent = pct + "%";
  const exact = el("volume-exact");
  if (document.activeElement !== exact) exact.value = pct;

  const isBinaural = state.preset.startsWith("binaural");
  el("carrier-row").hidden = !isBinaural;
  if (isBinaural) {
    document.querySelectorAll('input[name="carrier"]').forEach((r) => {
      r.checked = Number(r.value) === state.carrier_hz;
    });
  }

  el("play-toggle").textContent = state.playing ? "Pause" : "Play";
  el("startup-check").checked = !!state.launch_at_startup;
  el("autoplay-check").checked = !!state.autoplay;
  el("updates-check").checked = !!state.check_for_updates;
  el("version-line").textContent = "Version " + state.version;

  const checkBtn = el("check-now");
  checkBtn.disabled = checking;
  checkBtn.textContent = checking ? "Checking…" : "Check for updates now";
  checkBtn.setAttribute("aria-busy", String(checking));
}

async function installUpdate() {
  if (installing || !state || !state.update) return;
  installing = true;
  render();
  try {
    // A slow download can outlast the usual 8 s command budget.
    const res = await invokeWith("install_update", {}, 60000);
    // On success the process relaunches into the new version and
    // this line never runs.
    installing = false;
    applyEnvelope(res);
  } catch (e) {
    installing = false;
    const msg = String((e && e.message) || e);
    showError(msg.includes("timed out")
      ? "Taking longer than a minute. If the app does not restart, check app.log."
      : msg);
  } finally {
    render();
  }
}

async function checkForUpdates() {
  if (checking) return;
  checking = true;
  render();
  try {
    const res = await invoke("check_updates", {});
    applyEnvelope(res);
  } catch (e) {
    showError(String((e && e.message) || e));
  } finally {
    checking = false;
    render();
  }
}

function bind() {
  document.querySelectorAll(".preset[data-preset]").forEach((b) => {
    b.addEventListener("click", () => run("set_preset", { preset: b.dataset.preset }));
  });
  el("pulse-preset").addEventListener("click", () => {
    if (!state) return;
    run("set_pulse", { enabled: state.keepalive_mode !== "pulse" });
  });

  const slider = el("volume-slider");
  // Live label on input, persist to backend only on change (settled value).
  slider.addEventListener("input", () => {
    el("volume-label").textContent = formatPercent(sliderToGain(Number(slider.value))) + "%";
  });
  slider.addEventListener("change", () => {
    run("set_volume", { volume: sliderToGain(Number(slider.value)) });
  });

  el("volume-apply").addEventListener("click", () => {
    const pct = Number(el("volume-exact").value);
    if (!Number.isFinite(pct) || pct < 0.01 || pct > 100) {
      showError("Enter a percent between 0.01 and 100.");
      return;
    }
    run("set_volume", { volume: pct / 100 });
  });

  document.querySelectorAll(".chip").forEach((c) => {
    c.addEventListener("click", () => run("set_volume", { volume: Number(c.dataset.volume) / 100 }));
  });

  document.querySelectorAll('input[name="carrier"]').forEach((r) => {
    r.addEventListener("change", () => {
      if (r.checked) run("set_carrier", { carrier_hz: Number(r.value) });
    });
  });

  el("play-toggle").addEventListener("click", () => {
    if (!state) return;
    run("set_playing", { playing: !state.playing });
  });

  el("startup-check").addEventListener("change", (e) => {
    run("set_startup", { enabled: e.target.checked });
  });
  el("autoplay-check").addEventListener("change", (e) => {
    run("set_autoplay", { enabled: e.target.checked });
  });
  el("updates-check").addEventListener("change", (e) => {
    run("set_check_updates", { enabled: e.target.checked });
  });

  el("check-now").addEventListener("click", checkForUpdates);
  el("banner-install").addEventListener("click", installUpdate);
  el("open-logs").addEventListener("click", async () => {
    try {
      const res = await invoke("open_logs", {});
      if (!(res && res.ok)) showError((res && res.error) || "Could not open logs");
      else clearError();
    } catch (e) {
      showError(String((e && e.message) || e));
    }
  });
  el("reset-settings").addEventListener("click", () => run("reset_settings", {}));
  el("error-dismiss").addEventListener("click", clearError);
}

document.addEventListener("DOMContentLoaded", async () => {
  bind();
  try {
    const res = await invoke("get_state", {});
    if (!applyEnvelope(res)) bootFailed((res && res.error) || "Command failed");
  } catch (e) {
    bootFailed(String((e && e.message) || e));
  }
});
