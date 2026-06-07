const API = '/api/v1';
let modules = [];
let currentModule = null;

// ── Init ──
async function init() {
  try {
    const res = await fetch(`${API}/modules`);
    const data = await res.json();
    modules = data.modules.filter(m => !m.startsWith('_') && !m.includes('_contacts') && !m.includes('_opportunities') && !m.includes('_cases'));
    renderSidebar();
  } catch (e) {
    document.getElementById('db-status').innerHTML = '● offline';
    document.getElementById('db-status').style.color = 'var(--danger)';
  }
}

// ── Sidebar ──
function renderSidebar() {
  const list = document.getElementById('module-list');
  list.innerHTML = modules.map(m => `
    <div class="module-item" data-module="${m}" onclick="selectModule('${m}')">
      <span class="icon">${iconFor(m)}</span>
      <span>${m}</span>
    </div>
  `).join('');
}

function iconFor(m) {
  const icons = { accounts: '🏢', contacts: '👤', opportunities: '💼', leads: '🎯', cases: '🎫' };
  return icons[m] || '📋';
}

// ── Module selection ──
async function selectModule(module) {
  currentModule = module;
  document.querySelectorAll('.module-item').forEach(el => el.classList.remove('active'));
  document.querySelector(`[data-module="${module}"]`)?.classList.add('active');
  document.getElementById('view-title').textContent = capitalize(module);
  document.getElementById('empty-state').classList.add('hidden');
  document.getElementById('loading').classList.remove('hidden');
  try {
    const res = await fetch(`${API}/${module}?limit=50`);
    const data = await res.json();
    renderTable(data.data);
  } catch (e) {
    document.getElementById('loading').textContent = 'Failed to load';
  }
}

function capitalize(s) { return s.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase()); }

// ── Table ──
function renderTable(rows) {
  document.getElementById('loading').classList.add('hidden');
  if (!rows.length) {
    document.getElementById('empty-state').classList.remove('hidden');
    document.getElementById('data-table').classList.add('hidden');
    return;
  }
  const table = document.getElementById('data-table');
  const head = document.getElementById('table-head');
  const body = document.getElementById('table-body');
  const skipCols = ['id','date_entered','date_modified','modified_user_id','created_by','assigned_user_id','deleted','description','id_c'];
  const cols = Object.keys(rows[0]).filter(k => !skipCols.includes(k) && rows[0][k] !== null);
  const displayCols = cols.slice(0, 5);
  head.innerHTML = '<tr>' + displayCols.map(c => `<th>${c.replace(/_/g,' ')}</th>`).join('') + '</tr>';
  body.innerHTML = rows.map(row => `
    <tr onclick="openDetail('${currentModule}','${row.id}')">
      ${displayCols.map(c => `<td>${formatVal(row[c])}</td>`).join('')}
    </tr>
  `).join('');
  table.classList.remove('hidden');
  document.getElementById('empty-state').classList.add('hidden');
}

function formatVal(v) {
  if (v === null || v === undefined) return '';
  if (typeof v === 'number') return v.toLocaleString();
  return String(v);
}

// ── Detail ──
async function openDetail(module, id) {
  try {
    const res = await fetch(`${API}/${module}/${id}`);
    const data = await res.json();
    showDetailPanel(module, data.data, data.relationships);
  } catch (e) {}
}

function showDetailPanel(module, record, relationships) {
  document.getElementById('detail-overlay')?.remove();
  const overlay = document.createElement('div');
  overlay.id = 'detail-overlay';
  overlay.onclick = (e) => { if (e.target === overlay) overlay.remove(); };
  const name = record.first_name ? `${record.first_name} ${record.last_name||''}` : (record.name||'Record');
  const fields = Object.entries(record)
    .filter(([k,v]) => !['id','date_entered','date_modified','modified_user_id','created_by','deleted','id_c'].includes(k) && v !== null)
    .slice(0, 10)
    .map(([k,v]) => `<div class="detail-field"><span class="label">${k.replace(/_/g,' ')}</span><span class="value">${formatVal(v)}</span></div>`).join('');
  const relatedHtml = relationships?.length ? `
    <div class="related-section"><h4>Related (${relationships.length})</h4>
    ${relationships.map(r => {
      const n = r.first_name ? `${r.first_name} ${r.last_name||''}` : (r.name||'Related');
      const m = r.first_name ? 'contacts' : 'accounts';
      return `<div class="related-item" onclick="openDetail('${m}','${r.id}')">${n}</div>`;
    }).join('')}</div>` : '';
  overlay.innerHTML = `<div id="detail-panel">
    <button class="close-btn" onclick="this.closest('#detail-overlay').remove()">✕</button>
    <h3>${name}</h3><div class="detail-fields">${fields}</div>${relatedHtml}</div>`;
  document.body.appendChild(overlay);
}

// ── Photo Scan ──
function triggerScan() {
  document.getElementById('scan-input').click();
}

async function handleScan(file) {
  if (!file) return;
  const overlay = document.getElementById('scan-overlay');
  const status = document.getElementById('scan-status');
  const result = document.getElementById('scan-result');
  const spinner = overlay.querySelector('.spinner');

  overlay.classList.remove('hidden');
  result.style.display = 'none';
  spinner.style.display = 'block';
  status.textContent = 'Analyzing image...';

  const form = new FormData();
  form.append('image', file);

  try {
    const res = await fetch(`${API}/scan`, { method: 'POST', body: form });
    const data = await res.json();
    spinner.style.display = 'none';

    if (data.error) { status.textContent = data.error; return; }

    document.getElementById('scan-name').textContent = data.name || '—';
    document.getElementById('scan-company').textContent = data.company || '—';
    document.getElementById('scan-phone').textContent = data.phone || '—';
    document.getElementById('scan-email').textContent = data.email || '—';
    document.getElementById('scan-title').textContent = data.title || '—';
    result.style.display = 'flex';
    status.textContent = '';

    // Store parsed data for save/discard
    result._data = data;
    result._module = data.suggested_module || 'contacts';
  } catch (e) {
    spinner.style.display = 'none';
    status.textContent = 'Scan failed. Is llama.cpp installed?';
  }
}

function saveScan() {
  const result = document.getElementById('scan-result');
  const data = result._data;
  const module = result._module;
  if (!data) return;

  fetch(`${API}/${module}/import`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data)
  }).then(r => r.json()).then(d => {
    document.getElementById('scan-overlay').classList.add('hidden');
    if (currentModule === module) selectModule(module);
  }).catch(e => alert('Save failed'));
}

function discardScan() {
  document.getElementById('scan-overlay').classList.add('hidden');
}

// ── Start ──
init();
