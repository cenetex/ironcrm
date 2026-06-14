#!/usr/bin/env node

import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const root = new URL('..', import.meta.url).pathname;
const port = String(3300 + Math.floor(Math.random() * 500));
const tempDir = await mkdtemp(join(tmpdir(), 'ironcrm-smoke-'));
const dbPath = join(tempDir, 'ironcrm.db');
const base = `http://127.0.0.1:${port}`;
let server;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function request(path, options = {}) {
  const response = await fetch(base + path, options);
  const text = await response.text();
  let body = {};
  if (text) {
    try {
      body = JSON.parse(text);
    } catch (error) {
      throw new Error(`${path} returned non-JSON: ${text}`);
    }
  }
  if (!response.ok) {
    const error = new Error(`${path} failed with ${response.status}`);
    error.response = response;
    error.body = body;
    throw error;
  }
  return body;
}

async function waitForHealth() {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    try {
      const health = await request('/health');
      if (health.status === 'ok') return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error('HTTP server did not become healthy');
}

async function assertFrontendContract() {
  const [index, app] = await Promise.all([
    readFile(join(root, 'frontend/index.html'), 'utf8'),
    readFile(join(root, 'frontend/app.js'), 'utf8'),
  ]);
  assert(!/onclick=|onsubmit=/.test(index + app), 'inline generated handlers should stay out of the frontend');
  assert(app.includes('data-action="open-detail"'), 'delegated action contract should exist');
  assert(app.includes('previewBundleSummary'), 'restore preview should be wired');
  assert(app.includes('link-type-select'), 'relationship labels should be selectable');
}

async function runSmoke() {
  await assertFrontendContract();

  server = spawn('cargo', ['run', '--features', 'http-server', '--', 'server'], {
    cwd: root,
    env: {
      ...process.env,
      LOCAL_DB_PATH: dbPath,
      PORT: port,
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  server.stdout.on('data', (chunk) => process.stdout.write(chunk));
  server.stderr.on('data', (chunk) => process.stderr.write(chunk));

  await waitForHealth();

  const modules = await request('/api/v1/modules');
  const contactDef = modules.definitions.find((definition) => definition.module === 'contacts');
  assert(contactDef, 'contacts definition should be returned');
  assert(Array.isArray(contactDef.primary_fields), 'module definitions should expose primary_fields');
  assert(contactDef.primary_fields.includes('email1'), 'contact primary fields should include email');

  const contact = await request('/api/v1/contacts', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      first_name: 'Grace',
      last_name: 'Hopper',
      email1: 'grace@example.com',
    }),
  });
  const account = await request('/api/v1/accounts', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      name: 'Compiler Labs',
      industry: 'Software',
    }),
  });

  await request(`/api/v1/contacts/${contact.id}/notes`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ note: 'Ask about COBOL migration timeline' }),
  });

  const link = await request(`/api/v1/contacts/${contact.id}/links`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      target_module: 'accounts',
      target_id: account.id,
      relationship_type: 'employer',
    }),
  });
  assert(link.id, 'link should return an id');

  const detail = await request(`/api/v1/contacts/${contact.id}`);
  assert(detail.relationships.length === 1, 'linked account should appear in detail');
  assert(detail.relationships[0]._relationship_type === 'employer', 'relationship label should round-trip');

  const search = await request('/api/v1/search?q=COBOL&limit=10');
  assert(search.total === 1, 'note text should be searchable');

  const bundle = await request('/api/v1/export');
  assert(bundle.kind === 'ironcrm.workspace', 'export should identify IronCRM bundles');
  assert(bundle.version === 1, 'export should include supported bundle version');
  assert(bundle.records.length === 2, 'export should include created records');
  assert(bundle.record_links.length === 1, 'export should include record links');

  let rejectedInvalidBundle = false;
  try {
    await request('/api/v1/restore', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ records: [] }),
    });
  } catch (error) {
    rejectedInvalidBundle = error.response && error.response.status === 400;
  }
  assert(rejectedInvalidBundle, 'restore should reject non-IronCRM bundles with HTTP 400');

  const restore = await request('/api/v1/restore', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(bundle),
  });
  assert(restore.records === 2, 'restore should report record count');
  assert(restore.record_links === 1, 'restore should report link count');

  console.log(JSON.stringify({
    ok: true,
    port,
    contact: contact.id,
    account: account.id,
    link: link.id,
    searchTotal: search.total,
    restoredRecords: restore.records,
  }, null, 2));
}

try {
  await runSmoke();
} finally {
  if (server) {
    server.kill('SIGINT');
  }
  await rm(tempDir, { recursive: true, force: true });
}
