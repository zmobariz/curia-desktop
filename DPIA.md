# Data Protection / DPIA — template

*A lightweight data-protection assessment template for self-hosting Curia. **Not legal
advice.** Whoever deploys Curia is the data controller and should adapt this into their own
records of processing.*

## Purpose
Curia helps researchers find **UK law** (legislation, case law, Home Office policy/statistics,
country evidence). It is a **research aid, not advice**, and is not a system of record for
any client matter.

## What data flows where
- **User ↔ Curia (Matrix room / DMs):** can be end-to-end encrypted (Megolm) so message
  content is readable only by room members' devices and the bot — not the homeserver operator.
- **Bot → your model provider:** the **prompt text** (the user's question + Curia's persona +
  tool results) is sent to your chosen model provider's API to generate each reply. **This is
  the main egress of any personal data** — choose a provider whose data-retention/training
  terms you accept, and record that decision.
- **Bot → source APIs:** i.AI Lex (legislation), The National Archives Find Case Law,
  tribunalsdecisions.service.gov.uk, GOV.UK, EUAA, OHCHR, HUDOC. These receive only the
  **search terms / document IDs** Curia sends — drive them with legal terms, not client facts.
- **At rest:** secrets in `chmod 600` env files (gitignored); the Matrix E2EE crypto store
  kept out of version control.

## Special-category data — control it at the input
Immigration/asylum subject matter (asylum, trafficking, torture, children) is special-category.
The **primary, proportionate control is not to input client personal data into the assistant
at all** — use it only for **legal research framed around the legal issue** (statute, case law,
country conditions, policy), not client facts. The assistant does not need client identifiers
to do its job, and all output should be **verified by a qualified person**.

## Key risks & mitigations
| Risk | Mitigation |
|---|---|
| Sensitive prompt content egresses to the model provider | Data minimisation at the input (above); choose a provider with acceptable retention/no-training terms and record it; a **local model** can keep prompts on your own hardware. |
| Incorrect law relied on | Tools retrieve primary sources and auto-paginate (complete results + count); every answer attributes its **source with a reference/link**, flagging own-knowledge vs retrieved, so a qualified person verifies each authority at source. Human verification is the controlling safeguard. |
| Loss of the E2EE identity / config | `scripts/backup.sh` backs up the config + crypto store; keep the Matrix recovery key in your secrets manager. Wire an encrypted offsite target. |
| Unauthorised access | Private, invite-only room; federation off on the homeserver; `require_mention`; explicit allow-list of users. |

## Outstanding actions (for the controller)
1. Choose your model provider and **record its data-retention / model-training terms**.
2. Decide whether the most sensitive use should run on a **local model** instead.
3. Wire an **encrypted offsite backup** target.
4. Confirm reuse terms for any source whose extracts you republish (EUAA; UNHCR/OHCHR are attribution-based).

*Source licences: GOV.UK & legislation = OGL v3; Find Case Law = Open Justice Licence
(programmatic/bulk use needs TNA's free computational-analysis licence); UNHCR/OHCHR =
attribution. See `NOTICE.md`.*
