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

## Forced auto-update

The app uses Tauri's **signed updater**. On every launch it checks the release
endpoint; if a newer version exists it **downloads, installs and relaunches
automatically — the user gets no "skip" or "later" option**. A full-screen
overlay blocks use while the update installs.

It **fails open**: if the check can't reach the network (or isn't configured
yet), the error is logged and the app continues, so research still works
offline. To make updates *mandatory even offline* (hard-block), change
`check_and_force_update` in `src/main.rs` to surface the error to the UI and
refuse to continue instead of returning `Ok(())`.

### One-time setup (required before updates work)

Updates must be cryptographically signed; the **private key stays secret** (only
you hold it). Never commit it.

1. **Generate a keypair** (needs the Tauri CLI):
   ```bash
   cargo tauri signer generate -w ~/.tauri/curia.key
   ```
   This prints a **public key** and writes the private key to `~/.tauri/curia.key`.

2. **Add the public key** to `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`,
   replacing `REPLACE_WITH_YOUR_TAURI_UPDATER_PUBLIC_KEY`.

3. **Confirm the endpoint** in the same block points at where you publish releases
   (default: this repo's GitHub Releases `latest.json`).

### Releasing an update

1. Bump `version` in `tauri.conf.json` (and `Cargo.toml`).
2. Build with the signing key in the environment so artifacts are signed:
   ```bash
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/curia.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<the password you set>"
   cargo tauri build
   ```
   This produces the installer plus a `.sig` file and update artifacts under
   `src-tauri/target/release/bundle/`.
3. Publish a GitHub Release and upload the installer, its `.sig`, and a
   `latest.json` manifest:
   ```json
   {
     "version": "0.1.1",
     "notes": "What changed",
     "pub_date": "2026-06-22T12:00:00Z",
     "platforms": {
       "windows-x86_64": {
         "signature": "<contents of the .sig file>",
         "url": "https://github.com/zmobariz/Curia/releases/download/v0.1.1/Curia_0.1.1_x64-setup.exe"
       }
     }
   }
   ```
   Existing installs will pick it up and force-update on their next launch.

> **Tip:** the official `tauri-apps/tauri-action` GitHub Action builds the app,
> signs it, and generates `latest.json` for you on each tagged release — the
> recommended way to run this hands-off. Store the private key and its password
> as repository secrets.

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
