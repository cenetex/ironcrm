# IronCRM

**Your relationships, your keys. A local-first CRM with portable identity.**

IronCRM stores your contacts, deals, and activity on your machine. Every record carries a decentralized identifier that survives any export or migration. No accounts, no servers, no vendor lock-in.

## What it does

- **5 core modules** — Accounts, Contacts, Leads, Opportunities, Cases — with table and kanban views
- **Local-first engine** — SQLite-backed, Tauri shell, sub-150MB memory
- **SuiteCRM import** — point at a MySQL instance, IronCRM maps modules, relationships, and custom fields into its canonical schema
- **AI scanner** — photograph a business card, get a structured Contact (local llama.cpp vision, no cloud)
- **Portable identity** — every record gets a `did:key` or `did:ens` on creation; consent receipts travel with the contact forever
- **Self-describing exports** — one-click `.ironcrm` bundle: data + schema + relationships + consent, hash-verified on restore

## Quick start

```
# Run the HTTP API (port 3000)
cargo run --features http-server

# Import from SuiteCRM MySQL
DATABASE_URL=mysql://suitecrm:suitecrm@localhost:3306/suitecrm cargo run -- import

# Desktop app (Tauri)
cd src-tauri && cargo run
```

Open `http://localhost:3000` — the frontend is served from the same process.

## Architecture

```
┌─ Tauri Desktop Shell ───────────────────────────────┐
│  Frontend (HTML/CSS/JS)     Rust Backend (Axum)       │
│  ─ Table + Kanban views     ─ CRUD + import engine   │
│  ─ Search + detail panel    ─ DID / VC issuance       │
│  ─ Scanner UI               ─ llama.cpp vision        │
│                              ─ Bundle export/import   │
│                                                       │
│  ┌─ SQLite ─────────────────────────────────────┐     │
│  │  typed tables (accounts, contacts, …)         │     │
│  │  records blob store (junction tables)         │     │
│  │  module_definitions (schema registry)         │     │
│  │  activity_log (audit trail)                   │     │
│  │  did_registry (DID → record mapping)          │     │
│  │  consent_receipts (Verifiable Credentials)    │     │
│  └──────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────┘
```

## Product principles

1. **Data outlasts the tool.** Every record has a persistent, portable identifier. Export always works, always complete.
2. **No lock-in. Not even soft.** The database is a file. Export is one click. The schema is self-describing.
3. **Consent is a first-class object.** Every contact carries a Verifiable Credential proving the legal basis for holding it. GDPR/CCPA audit-ready.
4. **Local AI, not cloud AI.** Scanner runs llama.cpp on-device. Sensitive documents never leave the machine.
5. **Desktop-native, small footprint.** Tauri + Rust, < 2s startup, < 150MB RAM.

## License

MIT
