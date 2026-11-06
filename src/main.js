// Fast Mode — main floating panel.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;
const { Menu, Submenu, MenuItem, CheckMenuItem, PredefinedMenuItem } = window.__TAURI__.menu;

const $ = (id) => document.getElementById(id);
const card = $("card");
const input = $("input");
const statusEl = $("status");
const copyBtn = $("copy");
const errorEl = $("error");
const historyEl = $("history");

const MODELS = [["Default", ""], ["Haiku", "haiku"], ["Sonnet", "sonnet"], ["Opus", "opus"]];
const LINE = 16;
const MAX_LINES = 12;

const ICON = {
  check: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="11" fill="var(--green)"/><path d="M7 12.5l3.2 3.2L17 9" fill="none" stroke="#fff" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  warn: '<svg viewBox="0 0 24 24"><path d="M12 2.5 1.5 21h21L12 2.5Z" fill="var(--red)"/><path d="M12 9v5" stroke="#fff" stroke-width="2.2" stroke-linecap="round"/><circle cx="12" cy="17.3" r="1.3" fill="#fff"/></svg>',
  copy: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="8" y="8" width="12" height="13" rx="2"/><path d="M16 8V5a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h2"/></svg>',
  done: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  trash: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>',
};

const state = {
  status: "idle", // idle | running | copied | error
  error: "",
  lastResponse: null,
  history: [],
  settings: { model: "", inactiveOpacity: 0.7, shortcut: "" },
  showHistory: false,
  focused: true,
};
let resetTimer = null;

// ───────── rendering ─────────

function expanded() {
  return input.value.length > 0 || state.showHistory || state.status === "error";
}

function render() {
  const wide = expanded();
  card.classList.toggle("compact", !wide);
  card.classList.toggle("expanded", wide);

  switch (state.status) {
    case "running":
      statusEl.innerHTML = '<div class="spinner" title="Thinking… Click or press Esc to cancel"></div>';
      break;
    case "copied":
      statusEl.innerHTML = ICON.check;
      statusEl.title = "Copied";
      break;
    case "error":
      statusEl.innerHTML = ICON.warn;
      statusEl.title = state.error;
      break;
    default:
      statusEl.innerHTML = "";
      statusEl.title = "";
  }

  copyBtn.hidden = !state.lastResponse;
  errorEl.hidden = state.status !== "error";
  errorEl.textContent = state.error;
  historyEl.hidden = !state.showHistory;
  if (state.showHistory) renderHistory();

  autosize();
}

function autosize() {
  if (card.classList.contains("compact")) {
    input.style.height = "";
    return;
  }
  input.style.height = "auto";
  input.style.height = Math.min(Math.max(input.scrollHeight, LINE), LINE * MAX_LINES) + "px";
}

function renderHistory() {
  historyEl.replaceChildren();
  if (!state.history.length) {
    historyEl.innerHTML = '<div class="empty">No history yet</div>';
    return;
  }
  for (const item of state.history) {
    const row = document.createElement("div");
    row.className = "row";
    row.innerHTML = `
      <div class="row-head">
        <span class="date"></span>
        <button class="icon del" title="Delete">${ICON.trash}</button>
      </div>
      <div class="section request"><div class="body"><div class="label">REQUEST</div><div class="text"></div></div>
        <button class="icon cp" title="Copy request">${ICON.copy}</button></div>
      <div class="divider"></div>
      <div class="section response"><div class="body"><div class="label">RESPONSE</div><div class="text"></div></div>
        <button class="icon cp" title="Copy response">${ICON.copy}</button></div>`;
    row.querySelector(".date").textContent = new Date(item.date).toLocaleString(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
    });
    row.querySelector(".request .text").textContent = item.request;
    row.querySelector(".response .text").textContent = item.response;

    const [cpReq, cpRes] = row.querySelectorAll(".cp");
    wireCopy(cpReq, item.request);
    wireCopy(cpRes, item.response);
    row.querySelector(".del").addEventListener("click", async (e) => {
      e.stopPropagation();
      state.history = await invoke("delete_history", { id: item.id });
      render();
    });
    row.addEventListener("click", () => row.classList.toggle("open"));
    historyEl.append(row);
  }
}

function wireCopy(btn, text) {
  btn.addEventListener("click", async (e) => {
    e.stopPropagation();
    await invoke("copy_text", { text });
    btn.innerHTML = ICON.done;
    btn.classList.add("done");
    setTimeout(() => {
      btn.innerHTML = ICON.copy;
      btn.classList.remove("done");
    }, 1200);
  });
}

function setStatus(status, error = "") {
  clearTimeout(resetTimer);
  state.status = status;
  state.error = error;
  if (status === "copied") {
    resetTimer = setTimeout(() => {
      if (state.status === "copied") setStatus("idle");
    }, 2500);
  }
  render();
}

// ───────── actions ─────────

/** Send `override` (paste / hotkey) or the current input. */
async function submit(override) {
  if (state.status === "running") return;
  const snapshot = input.value;
  const text = (override ?? snapshot).trim();
  if (!text) return;
  setStatus("running");
  try {
    const res = await invoke("ask", { text });
    state.lastResponse = res.response;
    state.history = res.history;
    // Don't wipe something typed while waiting.
    if (override === undefined && input.value === snapshot) input.value = "";
    setStatus("copied");
  } catch (e) {
    const msg = String(e);
    if (msg === "cancelled") return setStatus("idle");
    if (!input.value) input.value = text; // keep the request so it isn't lost
    setStatus("error", msg);
  }
}

function cancel() {
  if (state.status === "running") invoke("cancel");
}

function toggleHistory() {
  state.showHistory = !state.showHistory;
  render();
}

async function showMenu() {
  const sep = () => PredefinedMenuItem.new({ item: "Separator" });
  const models = await Promise.all(
    MODELS.map(([text, value]) =>
      CheckMenuItem.new({
        text,
        checked: state.settings.model === value,
        action: () => invoke("set_model", { model: value }),
      }),
    ),
  );
  const menu = await Menu.new({
    items: [
      await Submenu.new({ text: "Model", items: models }),
      await sep(),
      await MenuItem.new({
        text: state.showHistory ? "Hide History" : `History (${state.history.length})`,
        action: toggleHistory,
      }),
      await sep(),
      await MenuItem.new({ text: "Settings…", action: () => invoke("open_settings") }),
      await MenuItem.new({ text: "Edit System Prompt…", action: () => invoke("edit_prompt") }),
      await MenuItem.new({
        text: "Clear History",
        action: async () => {
          await invoke("clear_history");
          state.history = [];
          render();
        },
      }),
      await sep(),
      await MenuItem.new({ text: "Quit Fast Mode", action: () => invoke("quit") }),
    ],
  });
  await menu.popup();
}

function applyOpacity() {
  document.body.style.opacity = state.focused ? "1" : String(state.settings.inactiveOpacity);
}

// ───────── events ─────────

input.addEventListener("input", () => {
  if (state.status === "error") setStatus("idle");
  else render();
});

input.addEventListener("keydown", (e) => {
  // Enter = newline; ⌘↩ / Ctrl+↩ = send.
  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
    e.preventDefault();
    submit();
  }
});

// Pasting into an empty field sends straight away.
input.addEventListener("paste", (e) => {
  if (input.value !== "" || state.status === "running") return;
  const text = e.clipboardData?.getData("text/plain") ?? "";
  if (!text.trim()) return;
  e.preventDefault();
  submit(text);
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") cancel();
});

statusEl.addEventListener("click", cancel);
$("gear").addEventListener("click", showMenu);
copyBtn.addEventListener("click", async () => {
  if (!state.lastResponse) return;
  await invoke("copy_text", { text: state.lastResponse });
  setStatus("copied");
});

// Keep the native window exactly the size of the card.
let lastSize = "";
new ResizeObserver(() => {
  const r = card.getBoundingClientRect();
  const w = Math.ceil(r.width);
  const h = Math.ceil(r.height);
  const key = `${w}x${h}`;
  if (key === lastSize) return;
  lastSize = key;
  invoke("resize", { width: w, height: h });
}).observe(card);

(async () => {
  const snap = await invoke("get_state");
  state.settings = snap.settings;
  state.history = snap.history;
  render();
  applyOpacity();
  input.focus();

  await listen("hotkey-send", (e) => submit(e.payload));
  await listen("settings", (e) => {
    state.settings = e.payload;
    applyOpacity();
  });
  await getCurrentWindow().onFocusChanged(({ payload }) => {
    state.focused = payload;
    applyOpacity();
    if (payload) input.focus();
  });
})();
