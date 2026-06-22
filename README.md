# Curia Desktop

A small, **portable Windows desktop app** for searching **UK legislation** through
the UK Government's [i.AI **Lex**](https://lex.lab.i.ai.gov.uk) API — built for
people on **locked-down machines who can't install software or use admin rights**.

- **No admin, no installer needed.** Ships as a single `.exe` that runs from the
  user's own folder. Uses the **WebView2** runtime that's already on Windows 10/11.
- **No subscription, no bundled AI.** Plain search returns official results with a
  link to `legislation.gov.uk` for every item — verify at the source.
- **Optional AI "Explain"** uses **your own** provider key (OpenAI, Anthropic,
  any OpenAI-compatible endpoint) or a **free local model via [Ollama](https://ollama.com)**
  (which also installs without admin). Keys are stored only on your PC.

> Research aid, not legal advice. Lex is an **experimental/beta** government
> service and is rate-limited; the app degrades gracefully if it's unavailable.
> Don't enter client-identifying or sensitive personal data.

## What it talks to

The app calls the Lex REST API directly over HTTPS — no Docker, no Matrix, no
server to run. Endpoints used (no authentication required):

| Action | Endpoint |
|---|---|
| Search Acts & SIs | `POST /legislation/search` |
| Full text of an item | `POST /legislation/text` |
| Service status | `GET /healthcheck` |

Case law is **not** currently served by the hosted Lex API, so this app focuses
on legislation; case-law search remains available via the repo's `caselaw-mcp`.

## Layout

```
desktop/
  ui/                 static frontend (HTML/CSS/JS — no bundler, no Node needed)
  src-tauri/          Rust (Tauri v2) backend
    src/lex.rs        Lex API client
    src/llm.rs        bring-your-own-key LLM client (OpenAI-compatible + Anthropic)
    src/settings.rs   per-user settings (JSON in %APPDATA%)
    src/main.rs       Tauri commands + wiring
```

## Build (produces the portable `.exe`)

Prerequisites (the *developer's* machine — end users need none of this):

1. **Rust** — https://rustup.rs
2. **Tauri CLI** — `cargo install tauri-cli --version "^2"`
3. On Windows: the **WebView2** runtime (preinstalled on Win10/11) and the MSVC
   build tools.

Then, from `desktop/`:

```bash
cargo tauri build
```

Outputs:

- **Portable executable:** `src-tauri/target/release/curia-desktop.exe` — copy
  this single file to the controlled machine and run it. No admin, no install.
- An NSIS per-user installer is also produced under
  `src-tauri/target/release/bundle/` if you prefer a Start-menu entry (still no
  admin — it installs into the user profile).

To develop with live reload: `cargo tauri dev`.

## Notes & roadmap

- API keys currently sit in the per-user settings file in plain text — fine for a
  single-user device; moving them to the Windows Credential Manager (OS keyring)
  is a planned hardening step.
- Cross-signing the `.exe` (code-signing certificate) will avoid SmartScreen
  warnings on first run; optional for internal distribution.
- Possible follow-ups: amendments & explanatory-notes tabs (endpoints already
  supported by Lex), OSCOLA-formatted export, and reusing the repo's `caselaw-mcp`
  for case law.

## Licence

Part of Curia — see the repository's [`LICENSE`](../LICENSE) and
[`LICENSING.md`](../LICENSING.md). Free for everyone while Lex is in beta.
