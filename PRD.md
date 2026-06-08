# IronCRM PRD — The Portable CRM

**Status:** Draft v1
**Product:** IronCRM (rebrand: The Portable CRM)
**Author:** Rati
**Date:** 2026-06-06

---

## 1. Executive Summary

IronCRM is a local-first, desktop CRM for small and mid-size businesses that makes customer data portable by default. Every contact, deal, and case is anchored to a persistent identifier that survives CRM migrations. Every export includes schema, relationships, and consent receipts — not just rows.

The CRM industry monetizes lock-in. IronCRM monetizes trust: a business that knows it can leave is a business that chooses to stay.

## 2. Problem Statement

**For traditional SMBs:** Switching CRMs means losing relationship history, custom field definitions, pipeline stages, and activity context. CSV exports are lossy. Migrations routinely cost $5k–$50k in services and data cleanup. The friction is designed — incumbents profit from it.

**For the regulatory environment:** GDPR (Art. 20), CCPA, and the EU Data Act increasingly mandate data portability. CRM vendors comply with grudging, incomplete export buttons. No CRM on the market treats portability as a first-class feature.

**For IronCRM specifically:** The current product is a proof-of-concept — a SuiteCRM-compatible viewer with an AI business-card scanner. It lacks write paths, pipeline management, and a reason to use it day-to-day. Repositioning around portability gives it a moat no incumbent can replicate without rewriting their data architecture.

## 3. Target Users

### Primary: Small business owners (1–25 employees)

Who they are: Freelance consultants, boutique law firms, insurance agents, real estate teams, local service businesses.

What they share:
- Currently use spreadsheets, SuiteCRM, or a free HubSpot/Zoho tier
- Have been burned by data lock-in at least once
- Care about ownership but don't have technical staff
- Will not pay per-seat SaaS pricing

### Secondary: SuiteCRM escapees

Organizations that adopted SuiteCRM for cost reasons, have outgrown its PHP maintenance burden, and want a migration path that preserves their data and customizations intact.

### Tertiary: Privacy-sensitive professionals

Doctors, therapists, lawyers, and accountants who must keep client data local for compliance (HIPAA, attorney-client privilege) but still need CRM functionality.

## 4. Value Proposition

> **Your customer relationships belong to you. Switch CRMs in one click. Prove consent to auditors instantly.**

| Pillar | What it means |
|--------|---------------|
| **Portable by default** | Every record gets a persistent DID. Exports are self-describing bundles, not lossy CSVs. |
| **Local-first, always** | Data lives on your device. No cloud dependency. Works offline. |
| **Consent-aware** | GDPR/CCPA consent receipts are issued per-contact and travel with the record. Audit-ready. |
| **AI-assisted entry** | Scan business cards, invoices, and contracts into structured records using local vision models. |

## 5. Competitive Landscape

| Competitor | Lock-in mechanism | Portability story | Local-first |
|------------|-------------------|-------------------|-------------|
| Salesforce | Custom objects, workflows, integrations | CSV export, paid data recovery | No |
| HubSpot | Free tier gravity, marketing integrations | API export (rate-limited) | No |
| SuiteCRM | Self-hosted PHP, MySQL dependency | Database dump (DIY) | Kind of |
| Zoho CRM | Module ecosystem | CSV per module | No |
| Airtable / Notion | Ecosystem lock-in, no CRM schema | CSV / JSON export | No |
| **IronCRM** | **None — design goal** | **Self-describing bundles with DIDs and consent receipts** | **Yes** |

## 6. Product Principles

1. **Data outlasts the tool.** A customer's CRM data should survive any vendor switch intact and semantically meaningful.
2. **No lock-in. Not even soft.** Export is always one click, always complete, always in an open format.
3. **Consent is a first-class object.** Every contact record carries proof of the legal basis for holding it.
4. **Local AI, not cloud AI.** Business cards and documents are scanned on-device. Sensitive data never leaves the machine.
5. **Desktop-native feel.** Built with Tauri for low memory, fast startup, and native OS integration — not an Electron wrapper.

## 7. Functional Scope

### 7.1 Core CRM (v1)

| Feature | Detail |
|---------|--------|
| Accounts | CRUD, custom fields, billing/shipping addresses, industry classification |
| Contacts | CRUD, name, title, phones, emails, accounts linkage |
| Leads | CRUD, status pipeline, lead source, conversion tracking |
| Opportunities | CRUD, kanban view, sales stages, probability, amounts, close dates |
| Cases | CRUD, priority, status, resolution, linked to accounts/contacts |
| Relationships | Many-to-many between any module pair, navigable from any record |
| Search | Full-text across all modules, filterable by field |
| Activity log | Timestamped notes, calls, emails per record |

### 7.2 Portability Engine (v1)

| Feature | Detail |
|---------|--------|
| DID anchoring | Every contact and account gets a `did:key` on creation. Stored locally, exportable. |
| Self-describing exports | Export bundle: data + module schemas + relationship graph + consent receipts. JSON-based, gzipped. |
| Self-describing imports | Ingest a bundle from any compliant CRM and reconstruct modules, relationships, and custom fields. |
| Consent receipts | Per-contact VC (`ConsentCredential`): legal basis, scope, expiration. Stays with the record forever. |
| Snapshot archives | One-click content-addressed snapshot of full DB state. Hash-verified on restore. |

### 7.3 AI Scanner (v1)

| Feature | Detail |
|---------|--------|
| Business card → Contact | Photograph → structured `Contact` with name, title, phone, email, company |
| Document → Opportunity | Photograph an invoice or contract → `Opportunity` with amount, counterparty, date |
| Fully local | Uses bundled llama.cpp + vision model. No cloud inference. No telemetry. |

### 7.4 Deferred (v2+)

- IPFS/Arweave pinning of snapshot archives (verifiable off-site backup)
- DID document resolution for cross-business contact sharing
- Multi-user with CRDT-based sync (no central server)
- Email/calendar integration
- Web-based access (self-hosted companion service)
- Mobile companion app

## 8. User Journeys

### Journey A: New business sets up their CRM

1. **Ana** runs a boutique PR agency with 3 employees. She downloads IronCRM.
2. On first launch, she sees an empty kanban and a prompt: "Add your first contact" with options: *Create manually*, *Import from SuiteCRM*, *Import from CSV*, or *Scan a business card*.
3. She imports a CSV of 80 media contacts. IronCRM detects columns, maps them to Contact fields, and creates the records — each with a DID generated automatically.
4. She opens a contact. The detail panel shows the record, a relationship tab (linked accounts), an activity log, and a **consent badge**: "No consent recorded." She clicks it, sets "Legitimate interest — journalist outreach" with a 2-year expiry. IronCRM issues a `ConsentCredential` and attaches it.
5. She creates her first Opportunity from the contact: "Q3 media buy — TechCrunch" with a $15k amount and "Proposal" stage. It appears on her kanban board.
6. She closes the app. Everything is local. No account signup. No sync latency.

### Journey B: SuiteCRM migration

1. **Marcus** runs a manufacturing rep firm on SuiteCRM. He has 2,000 accounts, 5,000 contacts, and 400 opportunities across 6 years. SuiteCRM's PHP version is EOL and he's tired of patching it.
2. He points IronCRM at his MySQL instance: `ironcrm import`.
3. IronCRM discovers all SuiteCRM modules, imports records with relationships intact, and maps custom fields into the local schema.
4. During import, every contact, account, and opportunity is assigned a DID. The relationship graph (accounts_contacts, accounts_opportunities) is reconstructed.
5. Marcus opens IronCRM. His data is there. His custom fields are there. His pipeline stages are there.
6. He exports a snapshot bundle for safekeeping. The bundle is a single `.ironcrm` file: his entire CRM, verifiable and portable.

### Journey C: Regulatory audit

1. **Dr. Chen** runs a private therapy practice. A client submits a GDPR Subject Access Request.
2. She opens IronCRM, searches for the client by name. The contact detail shows all stored fields, the activity log, and the **consent receipt**: "Explicit consent — therapy relationship — expires 2028-03-14." The receipt includes a timestamp and the legal basis.
3. She clicks "Export record." IronCRM produces a signed JSON bundle containing the contact, all related case notes, and the consent credential.
4. She sends the bundle to the client. The client or their representative can verify the consent credential independently — no need to trust Dr. Chen's word or her CRM.
5. The audit request is resolved in minutes. No legal consultation needed.

### Journey D: Switching away from IronCRM

1. **Priya** has used IronCRM for 2 years. She decides to switch to another CRM.
2. She clicks "Export all." IronCRM produces a `.ironcrm` bundle.
3. The bundle contains:
   - All records per module (accounts, contacts, leads, opportunities, cases)
   - Module schemas (field names, types, custom field definitions)
   - Full relationship graph
   - Consent receipts for every contact
   - Pipeline stage definitions
4. Her new CRM (if it supports the IronCRM Portable format — or via an open-spec converter) ingests the bundle. Her contacts have the same DIDs they were assigned in IronCRM. The audit trail is intact.
5. Priya's migration is complete in under 5 minutes. No data cleaning. No lost context.

### Journey E: Field use — scanning at a conference

1. **James** is at a trade show. He meets a potential supplier and gets their business card.
2. He opens IronCRM on his laptop, clicks "Scan," and holds the card to his webcam.
3. The local vision model extracts: name, company, title, phone, email. IronCRM suggests creating a Contact linked to a new Account for the supplier's company.
4. James confirms. Both records are created with DIDs. The contact gets a consent receipt auto-generated as "Legitimate interest — business contact — 3 years."
5. Back at his hotel, he attaches the supplier to an opportunity in his pipeline. The relationship is preserved.

## 9. Architecture

```
┌─────────────────────────────────────────┐
│              Tauri Desktop Shell          │
│  ┌─────────────┐  ┌───────────────────┐  │
│  │  Frontend    │  │  Rust Backend     │  │
│  │  (HTML/CSS/JS)│  │  (Axum + SQLite) │  │
│  │              │  │                   │  │
│  │  - Kanban    │  │  - CRUD API       │  │
│  │  - Tables    │  │  - DID generation │  │
│  │  - Scanner   │  │  - VC issuance    │  │
│  │  - Export UI │  │  - llama.cpp      │  │
│  └─────────────┘  │  - Bundle export   │  │
│                    └───────────────────┘  │
│                                           │
│  ┌─────────────────────────────────────┐  │
│  │           SQLite Database            │  │
│  │  records(module, id, data)           │  │
│  │  did_registry(record_id, did, doc)   │  │
│  │  consent_receipts(contact_id, vc)    │  │
│  └─────────────────────────────────────┘  │
└─────────────────────────────────────────┘
```

## 10. Success Metrics

| Metric | v1 Target | Rationale |
|--------|-----------|-----------|
| Time to first contact | < 30 seconds | Must feel instant for walk-up users |
| SuiteCRM import success rate | > 95% of modules | Migration is the primary acquisition channel |
| Scanner accuracy (business cards) | > 85% field-level | Must be reliable enough to trust |
| Export bundle completeness | 100% records + relationships + consent | Portability promise is absolute |
| App startup time | < 2 seconds | Desktop-native feel |
| Memory usage | < 150 MB | Sub-Electron baseline |

## 11. Risks and Open Questions

| Risk | Mitigation |
|------|------------|
| "Portable CRM" is a feature, not a purchase driver | Lead with "never lose your data" rather than tech specs. Demo the migration-from-competitor story in onboarding. |
| SuiteCRM as sole import source is narrow | Add CSV import + generic JSON import in v1. Broaden connectors over time. |
| Local-only limits collaboration | Defer multi-user. Solo operators are a large enough beachhead. |
| DID/VC complexity leaks into UX | Abstract entirely. Users see "portable contact" and "consent record," not DIDs and VCs. |
| llama.cpp bundling is heavy | Ship a minimal vision model (~2 GB). Make scanner skippable on first-run if storage is tight. |
| Competitor response | Salesforce/HubSpot can't retrofit portability without architectural change. Moat is structural. |

## 12. Milestones

| Milestone | Scope | Outcome |
|-----------|-------|---------|
| **M1 — Foundation** (current) | SuiteCRM import, read-only viewer, scanner scaffold | Proof-of-concept complete ✓ |
| **M2 — CRM Core** | CRUD for all modules, kanban board, search, activity log | Daily-drivable for a solo operator |
| **M3 — Portable Engine** | DID anchoring, consent receipts, self-describing export/import | Portability promise delivered |
| **M4 — Scanner Polish** | Bundled llama.cpp model, business card + document pipelines, >85% accuracy | Scanner as primary data-entry path |
| **M5 — Launch** | Landing page, download flow, docs, SuiteCRM migration guide | Public availability |

