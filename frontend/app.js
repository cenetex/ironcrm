const API = 'http://localhost:3000/api/v1';

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
    document.getElementById('db-status').innerHTML = '● disconnected';
    document.getElementById('db-status').style.color = 'var(--danger)';
  }
}

// ── Sidebar ──
function renderSidebar() {
  const list = document.getElementById('module-list');
  list.innerHTML = modules.map(m => `
    <div class="module-item" data-module="${m}" onclick="selectModule('${m}')">
      <span>${iconFor(m)}</span>
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
  document.getElementById('search-input').disabled = false;
  document.getElementById('empty-state').classList.add('hidden');
  document.getElementById('loading').classList.remove('hidden');

  try {
    const res = await fetch(`${API}/${module}?limit=50`);
    const data = await res.json();
    renderTable(data.data);
  } catch (e) {
    document.getElementById('loading').textContent = 'Failed to load data';
  }
}

function capitalize(s) {
  return s.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase());
}

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

  // Pick display columns: skip internal ones
  const skipCols = ['id', 'date_entered', 'date_modified', 'modified_user_id', 'created_by', 'assigned_user_id', 'deleted', 'description', 'id_c'];
  const cols = Object.keys(rows[0]).filter(k => !skipCols.includes(k) && rows[0][k] !== null);
  const displayCols = cols.slice(0, 6);

  head.innerHTML = '<tr>' + displayCols.map(c => `<th>${c.replace(/_/g, ' ')}</th>`).join('') + '</tr>';
  body.innerHTML = rows.map(row => `
    <tr onclick="openDetail('${currentModule}', '${row.id}')" style="cursor:pointer">
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

// ── Detail view ──
async function openDetail(module, id) {
  try {
    const res = await fetch(`${API}/${module}/${id}`);
    const data = await res.json();
    showDetailPanel(module, data.data, data.relationships);
  } catch (e) {
    console.error('Failed to load detail', e);
  }
}

function showDetailPanel(module, record, relationships) {
  // Remove existing overlay
  document.getElementById('detail-overlay')?.remove();

  const overlay = document.createElement('div');
  overlay.id = 'detail-overlay';
  overlay.onclick = (e) => { if (e.target === overlay) overlay.remove(); };

  const name = record.first_name
    ? `${record.first_name} ${record.last_name || ''}`
    : (record.name || 'Record');

  const fields = Object.entries(record)
    .filter(([k, v]) => !['id', 'date_entered', 'date_modified', 'modified_user_id', 'created_by', 'deleted', 'id_c'].includes(k) && v !== null)
    .slice(0, 12)
    .map(([k, v]) => `<div class="detail-field"><span class="label">${k.replace(/_/g, ' ')}</span><span class="value">${formatVal(v)}</span></div>`)
    .join('');

  const relatedHtml = relationships?.length ? `
    <div class="related-section">
      <h4>Related (${relationships.length})</h4>
      ${relationships.map(r => {
        const n = r.first_name ? `${r.first_name} ${r.last_name || ''}` : (r.name || 'Related');
        const m = r.first_name ? 'contacts' : 'accounts';
        return `<div class="related-item" onclick="openDetail('${m}', '${r.id}')">${n}</div>`;
      }).join('')}
    </div>
  ` : '';

  overlay.innerHTML = `
    <div id="detail-panel">
      <button class="close-btn" onclick="this.closest('#detail-overlay').remove()">✕</button>
      <h3>${name}</h3>
      <div class="detail-fields">${fields}</div>
      ${relatedHtml}
    </div>
  `;

  document.body.appendChild(overlay);
}

// ── Start ──
init();
