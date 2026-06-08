# IronCRM

**The Portable CRM. Your customer data, in a file you control.**

IronCRM is a local-first desktop CRM for small businesses that want useful relationship management without SaaS lock-in. It stores contacts, accounts, deals, notes, relationships, custom fields, and consent evidence locally, then exports everything in a complete `.ironcrm` bundle that can be restored into a fresh IronCRM instance.

## What it does

- **Core CRM modules** — Accounts, Contacts, Leads, Opportunities, and Cases.
- **Local-first engine** — SQLite-backed Rust backend with a Tauri desktop shell.
- **SuiteCRM migration** — maps SuiteCRM records, relationships, and custom fields into IronCRM's canonical model.
- **CSV / JSON import path** — external data flows through a shared mapping layer.
- **Complete exports** — one-click `.ironcrm` bundle with records, schemas, relationships, activities, custom fields, consent records, and manifest metadata.
- **Consent-aware records** — contacts can carry legal-basis/consent evidence and expiry status.
- **Optional modules** — scanner, calendar, email, and other features should extend the core without bloating v1.

## Product focus

The v1 promise is practical portability:

> Import data, manage it locally, export it completely, and restore it somewhere clean.

Advanced identity, signed credentials, CRDT sync, decentralized storage, and local AI scanning are valuable future layers, but they should not block the core CRM/export/restore loop.

## Quick start

```sh
# Run the HTTP API (port 3000)
cargo run --features http-server

# Import from SuiteCRM MySQL
DATABASE_URL=mysql://suitecrm:suitecrm@localhost:3306/suitecrm cargo run -- import

# Desktop app (Tauri)
cd src-tauri && cargo run
```

Open `http://localhost:3000` when running the HTTP server; the frontend is served from the same process.

## Architecture

```mermaid
graph TD
    subgraph App["Tauri Desktop App"]
        FE["Frontend<br/>HTML/CSS/JS"]
        BE["Rust Backend<br/>Axum + sqlx"]
    end

    FE --> BE
    BE --> SQLite

    subgraph SQLite["SQLite Database"]
        direction TB
        T1["canonical tables<br/>accounts, contacts, leads,<br/>opportunities, cases"]
        T2["relationship links"]
        T3["module definitions<br/>schema registry"]
        T4["activity log"]
        T5["consent records"]
        T6["source metadata<br/>raw import fields"]
        T7["optional module tables"]
    end

    BE --> Importers["Importers<br/>SuiteCRM / CSV / JSON"]
    BE --> Bundle[".ironcrm<br/>export/import"]
    BE --> Modules["Optional modules<br/>scanner / calendar / email"]
```

## Product principles

1. **Data outlasts the tool.** CRM data should remain meaningful after it leaves the app.
2. **Export is a core workflow.** Export must be complete, one-click, documented, and tested by restore.
3. **Local-first before networked.** IronCRM should be useful with no account, no server, and no internet connection.
4. **Boring CRM beats clever infrastructure.** Contacts, accounts, notes, search, and pipeline management come first.
5. **Consent is operational.** Users should know which contacts have consent/legal-basis evidence and what is expiring.
6. **Modules extend the core.** Scanner and other extras should feed the same canonical schema and export format.

## Docs

- [Product Requirements](./PRD.md)
- [Roadmap](./ROADMAP.md)

## License

MIT
