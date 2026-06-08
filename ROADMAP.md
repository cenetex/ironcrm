# IronCRM Roadmap

IronCRM's roadmap is portability-first: make the CRM useful every day, then prove that every bit of useful data can leave and be restored elsewhere. Advanced identity, signed credentials, scanner AI, sync, and decentralized storage are valuable only after that core loop is reliable.

## Phase 1 - Daily CRM Loop

**Goal:** IronCRM becomes a usable local CRM instead of a read-only viewer.

### Core CRM

- CRUD for Accounts, Contacts, Opportunities, Leads, and Cases.
- Prioritize Accounts, Contacts, and Opportunities first; Leads and Cases may start thinner.
- Opportunity kanban board using canonical stage definitions.
- Record detail pages with linked records and activity timeline.
- Append-only notes/activity log per record.
- Basic search across core modules and activities.
- Soft delete with restore path where practical.

### Canonical schema

- Define IronCRM-native structs for the five core modules.
- Keep simple primary fields: primary email, primary phone, primary address, display name/title, etc.
- Preserve overflow data in structured containers such as `extra_phones`, `extra_emails`, `custom_fields`, and `source_fields`.
- Store import provenance: `source_system`, `source_module`, `source_id`, `imported_at`, and optional raw source payload.
- Avoid making SuiteCRM's schema the permanent product schema.

### Audit baseline

- Every create/update/delete writes an append-only activity/audit row.
- Store actor/workspace ID, timestamp, module, record ID, operation, and changed fields.
- Do not block Phase 1 on cryptographic signatures or replayable event sourcing.

### Exit criteria

- A new user can create contacts/accounts/opportunities, link them, add notes, search, and reopen the app with data intact.
- Imported SuiteCRM core data is editable through the same UI as native records.

## Phase 2 - Import Mapping Layer

**Goal:** External data lands cleanly in IronCRM's canonical model.

### SuiteCRM import

- Map common SuiteCRM fields to canonical fields:
  - `phone_work` / `phone_mobile` / `phone_home` -> primary or extra phones.
  - `email1` / `email2` -> primary or extra emails.
  - `account_type`, industry, lead source, and custom fields -> canonical/custom fields.
- Reconstruct core relationships:
  - Account <-> Contact.
  - Account/Contact <-> Opportunity.
  - Account/Contact <-> Case.
- Preserve unmapped SuiteCRM fields as source/custom metadata.
- Provide import summary: counts, skipped rows, unmapped fields, relationship warnings.

### CSV import

- Column detection and mapping UI for contacts, accounts, and opportunities.
- Save/reuse mapping presets.
- Preview before import.
- Duplicate detection using email, name/company, and optional source IDs.

### Generic JSON import

- Document a simple JSON input format with module name, schema, records, and relationships.
- Treat scanner and future modules as producers of the same mapping pipeline, not special write paths.

### Exit criteria

- SuiteCRM and CSV imports produce normal editable IronCRM records.
- Import summaries are clear enough for a non-technical user to trust the result.

## Phase 3 - Portable Bundle MVP

**Goal:** Deliver the central product promise: complete export and restore.

### `.ironcrm` bundle format

A bundle contains:

- `manifest.json`: format version, app version, created time, workspace metadata, counts, hashes, feature flags.
- `schemas/`: module definitions, field definitions, custom field definitions, stage definitions.
- `records/`: canonical records by module.
- `relationships/`: graph/link records.
- `activities/`: notes, timeline events, and audit rows.
- `consent/`: legal-basis/consent records.
- `sources/`: import provenance and optional raw source metadata.
- `modules/`: optional module manifests/data when installed.

### Export

- One-click export all.
- Per-record export for audit/support workflows.
- Hash/checksum manifest to detect corruption.
- Human-readable JSON inside an archive where practical.

### Import/restore

- Restore a bundle into a fresh database.
- Validate manifest version and hashes before import.
- Preserve record IDs and source IDs.
- Show restore summary and warnings.

### Tests

- Golden bundle fixtures.
- Export -> delete database -> import -> compare record/relationship/activity counts.
- Schema compatibility tests across bundle versions.

### Exit criteria

- A user can export a `.ironcrm` bundle, move it to a clean IronCRM instance, import it, and see equivalent records, links, notes, schemas, custom fields, and consent records.

## Phase 4 - Consent and Audit UX

**Goal:** Make consent/legal-basis evidence operational for small businesses and privacy-sensitive professionals.

### Consent records

- Per-contact consent/legal-basis record.
- Fields: legal basis, scope/purpose, evidence/source note, issued date, expiry date, created/updated timestamps.
- Status model: missing, valid, expiring soon, expired.
- Consent history appears in contact activity timeline.

### Audit workflows

- Contact detail consent badge.
- Expiry dashboard.
- Per-contact audit export containing contact, related records, activities, and consent evidence.
- Subject-access style export with readable JSON and/or HTML summary.

### Guardrails

- Use language like "supports audit and portability workflows" rather than guaranteeing legal compliance.
- Keep W3C VC/signature details out of v1 UX.

### Exit criteria

- Users can quickly identify contacts with missing/expired consent evidence and export a complete record package for one contact.

## Phase 5 - Stable Identity and Signing

**Goal:** Add verifiability without complicating the early CRM experience.

### Stable portable IDs

- Every record has an immutable IronCRM portable ID.
- Imports preserve external IDs and map them to portable IDs.
- Bundle restore preserves portable IDs.

### Workspace identity

- Generate a local workspace signing key.
- Use workspace identity to sign exports and consent receipts in later iterations.
- Keep key backup/recovery UX explicit before relying on signatures.

### DID/VC exploration

- Evaluate DID support after stable IDs and bundles are proven.
- Prefer workspace-level DID first; record-level DIDs only if a real interoperability use case appears.
- Evaluate W3C Verifiable Credentials for consent receipts once the consent data model and UX are stable.

### Exit criteria

- Exported bundles can optionally be signed by the workspace identity and verified on restore.

## Phase 6 - Launch Hardening

**Goal:** Prepare IronCRM for public users.

### Product

- First-run onboarding with create/import/export paths.
- Sample workspace/demo data.
- SuiteCRM migration guide.
- CSV import guide.
- Backup/export reminders.
- Error handling for corrupt databases, failed imports, and incompatible bundles.

### Packaging

- Tauri desktop builds for macOS first, then Windows/Linux as feasible.
- Keep generated artifacts out of git.
- Optional module downloads should not bloat the default installer.

### Quality

- App startup under 2 seconds on target hardware.
- Memory under 150 MB without optional modules.
- Automated smoke test for create/import/export/restore.
- Manual migration test against representative SuiteCRM fixtures.

### Exit criteria

- A non-developer can install IronCRM, import data, use it for daily CRM basics, export a backup, and restore that backup.

## Phase 7 - Optional Modules

**Goal:** Extend IronCRM without bloating the core product or weakening portability.

Optional modules must write data through documented module schemas and export their data through `.ironcrm` bundles.

### Scanner module

- Business card image/webcam input to Contact/Account draft.
- Optional local model download or user-configured local inference command.
- Scanner output enters the same mapping/confirmation flow as CSV/JSON imports.
- Document/invoice extraction remains experimental until business-card scanning is reliable.

### Calendar module

- Activities and opportunity close dates shown in day/week/month views.
- Import ICS files or subscribe to external calendars as read-only activity sources.
- Calendar data exports as module data and activities.

### Email module

- Attach email metadata/content to contacts and accounts.
- Start with manual import or local mailbox parsing before cloud OAuth integrations.
- Preserve email activities in the portable bundle.

### Module SDK direction

- Module manifest declares schemas, views, migrations, and export/import participation.
- Modules should not bypass canonical records or portability guarantees.

### Exit criteria

- Scanner can be enabled/disabled independently, and records created by the scanner are indistinguishable from records created manually or by import.

## Future: Sync, Decentralized Storage, Ecosystem

These remain interesting, but they should not lead the product until the local CRM and bundle promise are proven.

### Decentralized/local-first sync

- CRDT-based sync for records, relationships, activities, and consent records.
- Local-network discovery first; user-hosted relay later.
- Conflict UI rather than silent overwrites.

### Backup and archive

- Encrypted off-site backup.
- Optional IPFS/Arweave archival for users who need it.
- Hash-verified restores remain the baseline.

### Ecosystem

- Open IronCRM Portable Format specification.
- Community converters for Salesforce, HubSpot, Zoho, QuickBooks, and spreadsheets.
- Web companion mode and mobile companion after sync foundations exist.
