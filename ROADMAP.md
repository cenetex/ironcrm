# IronCRM Roadmap

## Phase 1 — Write Paths & Canonical Schema (current)

IronCRM moves from read-only viewer to daily-drivable CRM. The import pipeline gets a mapping layer — external schemas (SuiteCRM, CSV, JSON) resolve into IronCRM's opinionated canonical model. Every record is deserialized into a Rust struct before it hits disk.

### Core

- CRUD for all 5 modules (create, read, update, delete)
- Canonical struct schema: each module has one primary phone, one primary email, one address; overflow lands in `extra_phones`, `extra_emails`, `custom_fields` columns
- Activity log: timestamped notes per record, stored as append-only rows with author DID
- Field-level audit trail: every mutation writes a signed delta to the log; full history replayable

### Import

- SuiteCRM import resolves through mapping layer: `phone_work` → `phone`, `phone_mobile` → `extra_phones`, `account_type` → `custom_fields`
- CSV import with column detection and mapping UI
- Generic JSON import for any source that bundles schema + data

### Scanner

- Bundled llama.cpp + vision model (llava-v1.6 or quantized equivalent, ~2GB)
- Business card → Contact pipeline (name, title, phone, email, company)
- Document → Opportunity pipeline (counterparty, amount, date)
- Scanner output feeds mapping layer same as imports — no special path

## Phase 2 — Portable Identity

Every record is a self-sovereign entity. You can move a Contact from IronCRM to another system and it's still the same Contact.

### DID Anchoring

- Every record gets a `did:key` on creation (Ed25519 keypair generated locally)
- DID documents are deterministic: `did:key:z6Mk…` → stable JSON document served from local resolver
- Optional `did:ens` support: link a record to an ENS name for human-readable portability
- Export bundles include DID documents; import verifies DIDs

### Consent Engine

- Per-contact `ConsentCredential` (W3C Verifiable Credential): legal basis (`gdpr_consent`, `legitimate_interest`, `contractual`), scope, expiry, issuer DID
- Consent receipts are signed by the IronCRM instance's key and travel with the record
- Audit export produces a single signed bundle of all consent receipts — GDPR Article 20 compliance in one click
- Expiry dashboard: "5 contacts need consent renewal in 30 days"

### Snapshot Exports

- Content-addressed archive (blake3): `ironcrm-2026-06-07-a3f2b.ironcrm`
- Contains: all records, module definitions, relationship graph, DID registry, consent receipts, activity log
- Hash-verified on restore; tamper-evident by construction

## Phase 3 — Social & Calendar

IronCRM stops being a database and starts being a relationship workspace. The contact detail panel shows a timeline of every interaction. Calendar, email, and call log entries are first-class records with their own DIDs.

### Social Feed

- Per-contact timeline: "Met at DevCon — 2026-05-12," "Sent proposal — 2026-05-14," "Follow-up call — 2026-05-20"
- Activity types: note, call, email, meeting, deal-stage-change, consent-renewal
- Global feed: reverse-chronological across all contacts, filterable by module and activity type
- Every activity is a signed record — the feed is an audit log you actually want to read

### Calendar Module

- Calendar view (day, week, month) populated from: meeting activities, opportunity close dates, consent expiry, task deadlines
- `did:ens` calendar sharing: publish an ICS feed for a contact or account, resolvable via ENS text record
- External calendar subscription (Google, iCloud) — pull external events into the timeline as read-only entries
- Scan a paper calendar or schedule card → structured activities (same llama.cpp pipeline)

### ENS Integration

- `did:ens` resolution: if a Contact has an ENS name (`rati.eth`), resolve their DID document and consent receipts from the name
- Contact discovery: given an ENS name, query for any published CRM records the owner has made available
- Optional ENS text records: `com.ironcrm.did`, `com.ironcrm.consent`, `com.ironcrm.calendar` — machine-readable pointers
- ETH address linkage: an Account can carry an Ethereum address; activity log can reference on-chain events (payment received, contract deployed)

## Phase 4 — Decentralized Sync

Multiple devices, same data, no central server. IronCRM instances discover each other and negotiate sync with conflict-free replicated data types.

### CRDT Sync

- CRDT-based (Automerge or custom state-based CRDT) for records, activities, and consent receipts
- Sync over local network (mDNS discovery) or relay server (user-hosted)
- No central coordinator; each instance is a full peer
- Conflicts surface in the UI rather than being silently resolved

### Backup & Archive

- IPFS pinning of snapshot archives: publish `.ironcrm` bundle to IPFS; pin via user-configured pinning service
- Arweave permanent archival: pay once, store forever — ideal for regulatory compliance records
- Encrypted backup: snapshot encrypted to a recovery key before upload; zero-knowledge from the storage provider

### Multi-Tenant

- Multiple organizations in one instance: switch workspaces
- Workspace-level DID (the organization's identity, separate from operator identity)
- Shared contacts: two organizations that do business together can mutually attest a Contact DID — "we both know this person"

## Phase 5 — Ecosystem

IronCRM becomes a platform that other tools connect to, not a walled garden.

### Plugin System

- Module plugins: add new module types (Invoices, Contracts, Projects) with their own Rust struct + `ModuleSchema` impl + frontend card
- Import connectors: any external system with a table or API can be a source — write a `SchemaResolver` trait impl
- Export formatters: `.ironcrm` bundle is the default; community plugins for Salesforce Data Loader, HubSpot API, QuickBooks

### Web Companion

- Self-hosted web service: same binary, different mode — serve the UI over HTTPS
- Optional OAuth for web access; data stays on the host machine
- Mobile companion app (iOS/Android via Tauri Mobile): read-only access to synced workspace

### Public Registries

- DID-based contact directory: publish a contact record with "public" consent for discovery by other IronCRM instances
- Reputation graph: signed attestations between businesses — "Acme Corp paid on time for 12 consecutive invoices"
- Open-source, open-spec: the IronCRM Portable Format is a standalone spec; any CRM can implement import/export
