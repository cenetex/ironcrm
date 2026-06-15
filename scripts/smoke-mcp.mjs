#!/usr/bin/env node

import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const root = new URL('..', import.meta.url).pathname;
const tempDir = await mkdtemp(join(tmpdir(), 'ironcrm-mcp-smoke-'));
const dbPath = join(tempDir, 'ironcrm.db');
let nextId = 1;

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const server = spawn('cargo', ['run', '--quiet', '--', 'mcp'], {
  cwd: root,
  env: {
    ...process.env,
    LOCAL_DB_PATH: dbPath,
    RUST_LOG: 'warn',
  },
  stdio: ['pipe', 'pipe', 'pipe'],
});

server.stderr.on('data', (chunk) => process.stderr.write(chunk));

const lines = createInterface({ input: server.stdout });
const pending = new Map();

lines.on('line', (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch (error) {
    throw new Error(`MCP server wrote non-JSON to stdout: ${line}`);
  }
  const waiter = pending.get(message.id);
  if (waiter) {
    pending.delete(message.id);
    waiter.resolve(message);
  }
});

function request(method, params = {}) {
  const id = nextId++;
  const payload = JSON.stringify({ jsonrpc: '2.0', id, method, params });
  server.stdin.write(payload + '\n');
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Timed out waiting for ${method}`));
    }, 20_000);
    pending.set(id, {
      resolve: (message) => {
        clearTimeout(timeout);
        if (message.error) reject(new Error(message.error.message));
        else resolve(message.result);
      },
    });
  });
}

function toolText(result) {
  assert(Array.isArray(result.content), 'tool result should include content array');
  const text = result.content.find((item) => item.type === 'text')?.text;
  assert(text, 'tool result should include text content');
  return JSON.parse(text);
}

try {
  const init = await request('initialize', {
    protocolVersion: '2024-11-05',
    capabilities: {},
    clientInfo: { name: 'ironcrm-smoke', version: '0.0.0' },
  });
  assert(init.serverInfo.name === 'ironcrm', 'initialize should return serverInfo');

  const listed = await request('tools/list');
  const toolNames = listed.tools.map((tool) => tool.name);
  assert(toolNames.includes('ironcrm_search_records'), 'search tool should be listed');
  assert(toolNames.includes('ironcrm_create_record'), 'create tool should be listed');

  const created = toolText(await request('tools/call', {
    name: 'ironcrm_create_record',
    arguments: {
      module: 'contacts',
      fields: {
        first_name: 'Ada',
        last_name: 'Lovelace',
        email1: 'ada@example.com',
      },
    },
  }));
  assert(created.id, 'create should return a record id');

  const search = toolText(await request('tools/call', {
    name: 'ironcrm_search_records',
    arguments: { query: 'Lovelace', limit: 5 },
  }));
  assert(search.total === 1, 'search should find the created contact');

  const detail = toolText(await request('tools/call', {
    name: 'ironcrm_get_record',
    arguments: { module: 'contacts', id: created.id },
  }));
  assert(detail.data.email1 === 'ada@example.com', 'detail should return record fields');

  const resource = await request('resources/read', { uri: 'ironcrm://workspace/summary' });
  assert(resource.contents?.[0]?.text, 'workspace summary resource should return text');

  console.log(JSON.stringify({
    ok: true,
    tools: toolNames.length,
    contact: created.id,
  }, null, 2));
} finally {
  server.stdin.end();
  server.kill('SIGTERM');
  await rm(tempDir, { recursive: true, force: true });
}
