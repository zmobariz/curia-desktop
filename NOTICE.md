# Notice, disclaimer & attributions

## Not legal advice
Curia is a **research aid, not legal advice**, and creates no lawyer–client relationship. It
retrieves from official sources, but retrieval may be incomplete and some sources are
experimental. **Always open the linked primary source and verify before relying on
anything.** A qualified person must review all output. Don't put client-identifying or other
sensitive personal data into the assistant.

## Third-party software
- **Hermes Agent** — NousResearch (the messaging agent runtime).
- **LiteLLM** — BerriAI (model proxy).
- **FastMCP**, **httpx**, **BeautifulSoup**, **pypdf**, **lxml** — their respective licences.
- **i.AI Lex** — github.com/i-dot-ai/lex (MIT), a **UK Government** project (the
  Incubator for AI, i.AI). Curia uses its hosted **beta / experimental** legislation
  API/MCP; Lex's own code and service remain owned and governed by i.AI.

## Data sources & their licences
You are responsible for complying with each source's terms, and for polite, rate-limited use.

- **legislation.gov.uk** and **GOV.UK** (CPINs, Immigration Rules, Home Office statistics,
  caseworker guidance) — Open Government Licence v3.0.
- **Find Case Law** (The National Archives) — the **Open Justice Licence**. Note that bulk or
  programmatic use ("computational analysis") of Find Case Law records requires a **free
  licence application to The National Archives** — obtain it before using `search_judgments`
  at scale, or restrict use to retrieval-by-citation.
- **tribunalsdecisions.service.gov.uk** (UTIAC) — Crown copyright / OGL.
- **EUAA** (EU Agency for Asylum) — EU institutional content; reusable with attribution;
  confirm terms before republishing extracts.
- **OHCHR / UNHCR** (Istanbul Protocol, guidance) — attribution; educational/non-commercial.
- **BAILII** — accessed by **link only** (the site blocks automated access). Do not scrape it.
- **HUDOC** (European Court of Human Rights) — Council of Europe; case-law texts are public.

Attribution requirements of the underlying sources are not waived by Curia's own licence
(see [`LICENSE`](LICENSE) / [`LICENSING.md`](LICENSING.md) — currently free for everyone,
including commercial users, while Lex is in beta). Curia's licence governs Curia's own
code only; it does not relicense the third-party software above (e.g. i.AI Lex remains MIT)
or grant any rights over the underlying data sources. Curia labels source provenance in its
answers and links back to the original.
