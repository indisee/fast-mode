# Fast Mode

```mermaid
flowchart LR
  A["⌘C"]:::a --> B["Hotkey (⌃⌥V)"]:::b --> C["claude -p {magic}"]:::c --> D["Clipboard"]:::d --> E["⌘V"]:::e
  classDef a fill:#e5e5ea,stroke:#8e8e93,color:#1d1d1f
  classDef b fill:#0a84ff,stroke:#0060df,color:#fff
  classDef c fill:#ff9f0a,stroke:#c77700,color:#fff
  classDef d fill:#34c759,stroke:#248a3d,color:#fff
  classDef e fill:#e5e5ea,stroke:#8e8e93,color:#1d1d1f
```

A small always-on-top panel for jobs you keep asking Claude to do the same way. Each **project** is a system prompt with a color: rewrite in polite English, write a commit message, summarize a thread, and so on.

<p align="center"><img src="docs/screenshots/typing.png" width="476" alt="Rough English typed into the panel"></p>

**`{magic}`:** the active project's system prompt. The default one is "Rewrite the text in natural, polite English. Translate it if it's in another language. Output only the result."

**Claude's reply**, already on your clipboard:

> Hi! Could you please take a look at my PR when you have a moment? I believe it's ready, but the tests are failing on Windows and I haven't been able to pin down why. Any insight would be much appreciated.

**History:**

<p align="center"><img src="docs/screenshots/result.png" width="476" alt="The same text rewritten in polished English"></p>

| <img src="docs/screenshots/compact.png" width="100"> | <img src="docs/screenshots/busy.png" width="100"> | <img src="docs/screenshots/copied.png" width="100"> |
| :-: | :-: | :-: |
| idle | working | copied |

## Use

- **A global hotkey** (you pick it in Settings) sends the clipboard from any app, and the reply replaces it. You never leave the app you are in.
- **Paste** into the empty panel to send right away, or type and press **⌘↩**.
- **Esc** cancels.
- **⚙** switches the project and model, and opens history and settings.

<p align="center">
  <img src="docs/screenshots/history.png" width="380" alt="History">
  <img src="docs/screenshots/settings.png" width="380" alt="Settings">
</p>

## Install

Needs [Claude Code](https://docs.claude.com/en/docs/claude-code) installed and signed in. Fast Mode runs `claude -p` with your account, so no API key is needed.

To build from source you also need Node.js and [Rust](https://tauri.app/start/prerequisites/).

```bash
npm install
```

```bash
npm run install:mac
```

On Windows or Linux use `npm run build`, and `npm run dev` while developing.
