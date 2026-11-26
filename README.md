# Curia Desktop

A small, **portable Windows desktop app** for researching **UK immigration law** (and UK law
generally) — built for people on **locked-down machines who can't install software or use
admin rights**.

- **Assistant** — ask a research question; Curia searches the Immigration Rules, Home Office
  guidance, UK legislation and case law live, then answers with **numbered citations that link
  to the exact paragraph** of each source. Follow-up questions keep the context. Uses **your
  own** AI key (Anthropic, OpenAI, any OpenAI-compatible endpoint or local Ollama).
- **Guidance & Rules** — search Home Office / UKVI caseworker guidance and every Part and
  Appendix of the Immigration Rules; read the passages relevant to your query, with deep links.
- **Case law** — relevance-ranked search of The National Archives' Find Case Law (Upper Tribunal
  IAC, Court of Appeal, Supreme Court, Admin Court, Privy Council by default); read the relevant
  paragraphs of any judgment.
- **Legislation** — Acts and SIs via the i.AI Lex API, with full text, amendments and
  explanatory notes.
- **Updates** — the latest Home Office/UKVI guidance changes on GOV.UK with their change notes;
  **watch** any guidance or Rules section and Curia keeps a snapshot and shows a tracked-changes
  diff when it changes, with an optional AI summary.
- **No admin, no installer, no subscription, no server.** Single `.exe`; uses the WebView2
  runtime already on Windows 10/11. Everything is fetched live over HTTPS.

> Research aid, not legal advice. Sources are experimental/beta services and may be
> rate-limited; always open and verify the linked primary source. Don't enter
> client-identifying or sensitive personal data.

## What it talks to

| Source | Used for | Licence |
|---|---|---|
| [i.AI Lex](https://lex.lab.i.ai.gov.uk) | Legislation search, section search, full text, amendments, explanatory notes | OGL |
| GOV.UK Search & Content APIs | Immigration Rules, Home Office/UKVI guidance, change notes | OGL v3.0 |
| [Find Case Law](https://caselaw.nationalarchives.gov.uk) | Judgment search and text | Open Justice Licence |
| Your AI provider (optional) | Assistant answers, Explain, change summaries | Your provider's terms |

Curia only runs the searches you (or your Assistant) ask for and reads judgments one at a
time. It does **no bulk download or computational analysis** of Find Case Law, which would
need a separate (free) licence from The National Archives. Historical versions of the
Immigration Rules are not available from any official API, so Curia shows current text plus
GOV.UK change notes and diffs of documents you watch.

## Layout

```
  ui/                 static frontend (HTML/CSS/JS — no bundler, no Node needed)
  src-tauri/          Rust (Tauri v2) backend
    src/lex.rs        Lex API client (legislation)
    src/govuk.rs      GOV.UK Search/Content client (guidance, Immigration Rules)
    src/caselaw.rs    Find Case Law client (Atom search, LegalDocML judgments)
    src/passages.rs   picks relevant passages (BM25) with deep-link anchors
    src/assistant.rs  tool-using research assistant with numbered citations
    src/tracker.rs    Updates tracker: watch list, snapshots, diffs
    src/llm.rs        bring-your-own-key LLM client (Anthropic + OpenAI-compatible, tool calling)
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

This repo ships a GitHub Actions workflow — [`.github/workflows/release.yml`](.github/workflows/release.yml)
— that builds on Windows, signs the artifacts, generates `latest.json`, and
publishes a GitHub Release. **The recommended flow is hands-off:**

1. Add the two repository secrets (Settings → Secrets and variables → Actions):
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
2. Bump `version` in `tauri.conf.json` (and `Cargo.toml`), commit.
3. Tag and push:
   ```bash
   git tag v0.1.1 && git push origin v0.1.1
   ```
   The workflow publishes the release; existing installs force-update on next
   launch. (A manual "Run workflow" build instead uploads a testable installer
   as a downloadable artifact.)

### Releasing by hand (alternative)

If you'd rather build locally, sign with the key in the environment:
```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/curia.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="<the password you set>"
cargo tauri build
```
This produces the installer plus a `.sig` file under
`src-tauri/target/release/bundle/`. Publish a GitHub Release and upload the
installer, its `.sig`, and a `latest.json` manifest:
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

## Notes & roadmap

- Your AI provider API key is stored in Windows Credential Manager, not in the
  settings file, and is never sent back to the UI. Keys saved by v0.1.0 in plain
  text are migrated automatically on first launch of v0.1.1.
- "Open on legislation.gov.uk" links only open `https://` legislation.gov.uk
  (and ollama.com) addresses; Lex and AI provider URLs must use `https://`
  (plain `http://` is allowed only for localhost, e.g. Ollama).
- Cross-signing the `.exe` (code-signing certificate) will avoid SmartScreen
  warnings on first run; optional for internal distribution.
- Possible follow-ups: amendments & explanatory-notes tabs (endpoints already
  supported by Lex), OSCOLA-formatted export, and reusing the repo's `caselaw-mcp`
  for case law.

## Licence

Part of Curia — see the repository's [`LICENSE`](LICENSE) and
[`LICENSING.md`](LICENSING.md). Free for everyone while Lex is in beta.
