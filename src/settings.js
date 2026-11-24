// Fast Mode — settings window.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
let settings = {}, platform = "macos", recording = false;

const MODS = ["Control", "Alt", "Shift", "Super"];
const MAC_SYM = { Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘" };
const PC_NAME = { Control: "Ctrl", Alt: "Alt", Shift: "Shift", Super: "Win" };

function keyName(code) {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  const map = { Space: "Space", Enter: "↩", Tab: "⇥", Backspace: "⌫", ArrowLeft: "←", ArrowRight: "→",
                ArrowUp: "↑", ArrowDown: "↓", Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]",
                Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/", Backslash: "\\", Backquote: "`" };
  return map[code] ?? code;
}

function label(accel) {
  if (!accel) return "";
  const parts = accel.split("+");
  const key = parts.pop();
  if (platform === "macos") return parts.map((m) => MAC_SYM[m] ?? m).join("") + keyName(key);
  return [...parts.map((m) => PC_NAME[m] ?? m), keyName(key)].join("+");
}

function render() {
  $("rec").textContent = recording ? "Type shortcut…" : (label(settings.shortcut) || "Record Shortcut");
  $("rec").classList.toggle("recording", recording);
  $("clear").hidden = recording || !settings.shortcut;
  $("model").value = settings.model ?? "";
  $("opacity").value = settings.inactiveOpacity;
  $("opval").textContent = Math.round(settings.inactiveOpacity * 100) + "%";
}

function showError(msg) {
  $("err").hidden = !msg;
  $("err").textContent = msg ? "This shortcut is taken by another app — pick a different one." : "";
  if (msg) $("err").title = msg;
}

async function save(accel) {
  try { await invoke("set_shortcut", { accel }); showError(""); }
  catch (e) { showError(String(e)); }
}

function stop(resume) {
  recording = false;
  if (resume) invoke("resume_shortcut").catch((e) => showError(String(e)));
  render();
}

$("rec").addEventListener("click", () => {
  if (recording) return stop(true);
  recording = true;
  invoke("pause_shortcut");
  render();
});
$("clear").addEventListener("click", () => save(""));

document.addEventListener("keydown", (e) => {
  if (!recording) return;
  e.preventDefault();
  if (["Meta", "Control", "Alt", "Shift"].includes(e.key)) return; // wait for the real key
  const noMods = !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey;
  if (e.key === "Escape" && noMods) return stop(true);
  if (e.key === "Backspace" && noMods) { stop(false); return save(""); }
  if (!e.metaKey && !e.ctrlKey && !e.altKey) return; // need ⌘, ⌥ or ⌃
  const mods = [e.ctrlKey && "Control", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  const accel = [...MODS.filter((m) => mods.includes(m)), e.code].join("+");
  stop(false);
  save(accel);
});

$("model").addEventListener("change", (e) => invoke("set_model", { model: e.target.value }));
$("opacity").addEventListener("input", (e) => {
  $("opval").textContent = Math.round(e.target.value * 100) + "%";
  invoke("set_opacity", { value: Number(e.target.value) });
});

// ── projects ──
const COLORS = ["", "#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#00c7be", "#0a84ff", "#5e5ce6", "#bf5af2", "#ff2d55", "#8e8e93"];
let projects = [], sel = null, saveTimer = null, delArmed = false;
function disarm() { delArmed = false; $("pdel").textContent = "−"; $("pdel").style.cssText = ""; }
const cur = () => projects.find((p) => p.id === sel);

function renderProjects() {
  if (!cur()) sel = (projects.find((p) => p.id === settings.activeProject) ?? projects[0])?.id;
  $("pitems").replaceChildren(...projects.map((p) => {
    const li = document.createElement("li");
    li.className = p.id === sel ? "sel" : "";
    li.innerHTML = '<span class="dot"></span><span class="name"></span><span class="on"></span>';
    li.querySelector(".dot").style.background = p.color || "transparent";
    li.querySelector(".name").textContent = p.name || "Untitled";
    li.querySelector(".on").textContent = p.id === settings.activeProject ? "✓" : "";
    li.addEventListener("click", () => { disarm(); sel = p.id; renderProjects(); renderEditor(); });
    return li;
  }));
  $("pdel").disabled = projects.length < 2;
}

function renderEditor() {
  const p = cur();
  if (!p) return;
  if (document.activeElement !== $("pname")) $("pname").value = p.name;
  if (document.activeElement !== $("pprompt")) $("pprompt").value = p.prompt;
  const custom = p.color && !COLORS.includes(p.color);
  $("pcolors").replaceChildren(
    ...COLORS.map((c) => {
      const b = document.createElement("button");
      b.className = "sw" + (c ? "" : " none") + (p.color === c ? " sel" : "");
      if (c) b.style.background = c;
      b.title = c || "No color";
      b.addEventListener("click", () => update({ color: c }));
      return b;
    }),
    Object.assign(document.createElement("input"), { type: "color", value: custom ? p.color : "#888888", title: "Custom color",
      className: custom ? "sel" : "", oninput: (e) => update({ color: e.target.value }) }),
  );
  const active = p.id === settings.activeProject;
  $("puse").hidden = active;
  $("pstate").textContent = active ? "Active project" : "";
}

function update(patch, rerenderEditor = true) {
  Object.assign(cur(), patch);
  renderProjects();
  if (rerenderEditor) renderEditor();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => invoke("save_projects", { projects }), 300);
}

$("pname").addEventListener("input", (e) => update({ name: e.target.value }, false));
$("pprompt").addEventListener("input", (e) => update({ prompt: e.target.value }, false));
$("padd").addEventListener("click", () => {
  const p = { id: crypto.randomUUID().toUpperCase(), name: "New Project", prompt: "", color: "" };
  projects.push(p);
  sel = p.id;
  update({});
  $("pname").focus();
  $("pname").select();
});
$("pdel").addEventListener("click", () => {
  const p = cur();
  if (!p || projects.length < 2) return;
  // Two clicks to delete (WebView has no confirm dialog).
  if (!delArmed) {
    delArmed = true;
    $("pdel").textContent = "Delete?";
    $("pdel").style.cssText = "width:auto;padding:0 8px;color:var(--red);font-size:12px";
    setTimeout(disarm, 2500);
    return;
  }
  disarm();
  projects = projects.filter((x) => x.id !== p.id);
  sel = null;
  clearTimeout(saveTimer);
  invoke("save_projects", { projects });
  renderProjects();
  renderEditor();
});
$("puse").addEventListener("click", () => invoke("set_active_project", { id: sel }));

(async () => {
  const snap = await invoke("get_state");
  settings = snap.settings;
  platform = snap.platform;
  projects = snap.projects;
  render();
  renderProjects();
  renderEditor();
  await listen("settings", (e) => {
    settings = e.payload;
    if (!recording) render();
    renderProjects();
    renderEditor();
  });
})();
