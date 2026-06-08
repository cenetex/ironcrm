var statusEl = document.getElementById('db-status');
var modules = [];
var moduleCounts = {};
var moduleDefs = {};   // module -> ModuleDefinition from API
var currentModule = null;
var currentView = 'table';
var currentRows = [];
var selectedRecordId = null;
var searchQuery = '';
var sidebarCollapsed = false;
var detailOpen = false;
var currentBoardStage = null;
var collapsedStages = {};

var OVERVIEW_ITEMS = [
  { id: 'recent', label: 'Recent', description: 'Quick access to active work' },
  { id: 'pipeline', label: 'Pipeline', description: 'Revenue in motion' },
  { id: 'inbox', label: 'Needs attention', description: 'Stale or unassigned records' }
];

var isTauri = !!(window.__TAURI_INTERNALS__);
var invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;

function show(msg, color) {
  statusEl.innerHTML = msg;
  if (color) statusEl.style.color = color;
}

function log(level, msg, obj) {
  if (isTauri && invoke) {
    var text = msg + (obj !== undefined ? ' ' + JSON.stringify(obj) : '');
    invoke('log_frontend', { level: level, message: text }).catch(function(){});
  } else {
    console.log('[IronCRM]', level, msg, obj !== undefined ? obj : '');
  }
}

function toast(message) {
  var region = document.getElementById('toast-region');
  var el = document.createElement('div');
  el.className = 'toast';
  el.textContent = message;
  region.appendChild(el);
  setTimeout(function() { el.classList.add('show'); }, 10);
  setTimeout(function() {
    el.classList.remove('show');
    setTimeout(function() { if (el.parentNode) el.parentNode.removeChild(el); }, 180);
  }, 2200);
}

var api = {
  get: async function(path) {
    if (isTauri && invoke) {
      log('debug', 'tauri ' + path);
      if (path === '/api/v1/modules') return invoke('get_modules');
      var query = path.includes('?') ? path.split('?')[1] : '';
      var clean = path.split('?')[0];
      var parts = clean.split('/');
      if (parts.length === 4 && parts[1] === 'api' && parts[2] === 'v1') {
        var params = new URLSearchParams(query);
        return invoke('get_module_records', {
          module: parts[3],
          limit: parseInt(params.get('limit'), 10) || 50,
          offset: parseInt(params.get('offset'), 10) || 0
        });
      }
      if (parts.length === 5 && parts[1] === 'api' && parts[2] === 'v1') {
        return invoke('get_module_record', { module: parts[3], id: parts[4] });
      }
      throw new Error('Unsupported path ' + path);
    }
    var res = await fetch(path);
    return res.json();
  }
};

function getMeta(module) {
  var def = moduleDefs[module];
  if (def) {
    var views = ['table'];
    if (def.board_field) views.unshift('board');
    return {
      icon: def.icon || '\u2022',
      title: def.label || titleize(module),
      subtitle: def.label_plural || 'Records',
      defaultView: def.default_view || 'table',
      views: views,
      columns: def.list_columns || [],
      summary: def.detail_summary || [],
      boardField: def.board_field || null
    };
  }
  return {
    icon: '\u2022',
    title: titleize(module),
    subtitle: 'Records',
    defaultView: 'table',
    views: ['table'],
    columns: [],
    summary: [],
    boardField: null
  };
}

function titleize(value) {
  return String(value || '').replace(/_/g, ' ').replace(/\b\w/g, function(c) { return c.toUpperCase(); });
}

function formatValue(key, value, row) {
  if (value === null || value === undefined || value === '') return '\u2014';
  if (key === 'full_name') {
    var name = [row.first_name, row.last_name].filter(Boolean).join(' ').trim();
    return name || 'Unnamed contact';
  }
  if (key === 'amount') {
    var num = Number(value);
    if (!isNaN(num) && value !== '') return '$' + num.toLocaleString();
  }
  if (key === 'account_id') {
    if (row.account_name) return row.account_name;
    return value;
  }
  return String(value);
}

function getCellValue(row, key) {
  if (key === 'full_name') {
    return [row.first_name, row.last_name].filter(Boolean).join(' ').trim();
  }
  return row[key];
}

function getRecordLabel(module, row) {
  if (!row) return 'Record';
  if (module === 'contacts' || module === 'leads') {
    var full = [row.first_name, row.last_name].filter(Boolean).join(' ').trim();
    return full || row.email1 || row.id || 'Record';
  }
  return row.name || row.account_name || row.id || 'Record';
}

function getRecordSubtitle(module, row) {
  var meta = getMeta(module);
  return meta.summary
    .map(function(key) { return formatValue(key, getCellValue(row, key), row); })
    .filter(function(v) { return v && v !== '\u2014'; })
    .slice(0, 3)
    .join(' \u2022 ');
}

function getStatusTone(module, row) {
  var value = (row.sales_stage || row.status || row.priority || '').toLowerCase();
  if (value.indexOf('won') >= 0 || value.indexOf('closed won') >= 0 || value.indexOf('high') >= 0) return 'success';
  if (value.indexOf('new') >= 0 || value.indexOf('assigned') >= 0 || value.indexOf('proposal') >= 0) return 'info';
  if (value.indexOf('negotiation') >= 0 || value.indexOf('in process') >= 0) return 'warning';
  if (value.indexOf('lost') >= 0 || value.indexOf('closed lost') >= 0) return 'danger';
  return 'neutral';
}

function stageId(module, name) {
  return module + '-' + String(name || 'unspecified').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
}

function getBoardGroups(module, rows) {
  var meta = getMeta(module);
  var field = meta.boardField;
  var groups = {};
  rows.forEach(function(row) {
    var key = row[field] || 'Unspecified';
    if (!groups[key]) groups[key] = [];
    groups[key].push(row);
  });
  var def = moduleDefs[module];
  var order = (def && def.board_stages) || [];
  return Object.keys(groups).sort(function(a, b) {
    var ai = order.indexOf(a);
    var bi = order.indexOf(b);
    if (ai === -1 && bi === -1) return a.localeCompare(b);
    if (ai === -1) return 1;
    if (bi === -1) return -1;
    return ai - bi;
  }).map(function(key) {
    var items = groups[key];
    var totalAmount = items.reduce(function(sum, item) {
      var amount = Number(item.amount || 0);
      return isNaN(amount) ? sum : sum + amount;
    }, 0);
    return {
      id: stageId(module, key),
      name: key,
      items: items,
      totalAmount: totalAmount
    };
  });
}

function ensureBoardStage(module, groups) {
  if (!groups.length) {
    currentBoardStage = null;
    return null;
  }
  var ids = groups.map(function(group) { return group.id; });
  if (!currentBoardStage || ids.indexOf(currentBoardStage) === -1) {
    currentBoardStage = groups[0].id;
  }
  return currentBoardStage;
}

function stageCollapseKey(module, stageIdValue) {
  return module + ':' + stageIdValue;
}

function updateStageStripActive(stageIdValue) {
  var pills = document.querySelectorAll('.stage-pill');
  for (var i = 0; i < pills.length; i += 1) {
    pills[i].classList.toggle('active', pills[i].getAttribute('data-stage-id') === stageIdValue);
  }
}

function bindBoardScroll() {
  var boardView = document.getElementById('board-view');
  if (!boardView) return;
  boardView.onscroll = function() {
    var sections = boardView.querySelectorAll('.stage-section');
    var bestId = null;
    var bestDelta = Infinity;
    var offset = boardView.scrollTop + (boardView.querySelector('.stage-strip') || {}).offsetHeight || 40 + 20;
    for (var i = 0; i < sections.length; i += 1) {
      var section = sections[i];
      var delta = Math.abs(section.offsetTop - offset);
      if (delta < bestDelta) {
        bestDelta = delta;
        bestId = section.getAttribute('data-stage-id');
      }
    }
    if (bestId && bestId !== currentBoardStage) {
      currentBoardStage = bestId;
      updateStageStripActive(bestId);
    }
  };
}

function filterRows(rows) {
  if (!searchQuery) return rows.slice();
  var q = searchQuery.toLowerCase();
  return rows.filter(function(row) {
    return Object.keys(row).some(function(key) {
      var value = getCellValue(row, key);
      return value !== null && value !== undefined && String(value).toLowerCase().indexOf(q) >= 0;
    });
  });
}

function renderOverview() {
  document.getElementById('sidebar-overview').innerHTML = OVERVIEW_ITEMS.map(function(item) {
    return '<button class="overview-item" type="button" onclick="toast(\'' + item.label + ' view coming next\')">' +
      '<span class="overview-title">' + item.label + '</span>' +
      '<span class="overview-desc">' + item.description + '</span>' +
      '</button>';
  }).join('');
}

function renderSidebar() {
  document.getElementById('module-list').innerHTML = modules.map(function(module) {
    var meta = getMeta(module);
    var count = moduleCounts[module] || 0;
    return '<button class="module-item' + (module === currentModule ? ' active' : '') + '" type="button" data-module="' + module + '" onclick="selectModule(\'' + module + '\')">' +
      '<span class="module-icon">' + meta.icon + '</span>' +
      '<span class="module-copy"><span class="module-name">' + meta.title + '</span><span class="module-subtitle">' + meta.subtitle + '</span></span>' +
      '<span class="module-count">' + count + '</span>' +
      '</button>';
  }).join('');
}

function renderToolbar(module) {
  var meta = getMeta(module);
  document.getElementById('view-eyebrow').textContent = module ? 'Module' : 'Workspace';
  document.getElementById('view-title').textContent = module ? meta.title : 'Dashboard';
  document.getElementById('view-meta').innerHTML = module
    ? '<div class="meta-chip count-chip">' + (moduleCounts[module] || 0) + ' records</div>'
    : '<div class="meta-chip count-chip">Local-first workspace</div>';

  document.getElementById('view-switcher').innerHTML = meta.views.map(function(view) {
    return '<button type="button" class="segmented-btn' + (view === currentView ? ' active' : '') + '" onclick="changeView(\'' + view + '\')">' + titleize(view) + '</button>';
  }).join('');

  document.getElementById('active-context').textContent = module ? meta.title : 'Dashboard';
  document.getElementById('active-summary').textContent = module
    ? (searchQuery ? 'Filtered by "' + searchQuery + '"' : meta.subtitle)
    : 'Overview coming next';
}

function syncShellState() {
  var app = document.getElementById('app');
  app.classList.toggle('sidebar-collapsed', sidebarCollapsed);
  app.classList.toggle('detail-open', detailOpen);
  document.getElementById('sidebar-toggle').setAttribute('aria-label', sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar');
}

function toggleSidebar(forceValue) {
  sidebarCollapsed = typeof forceValue === 'boolean' ? forceValue : !sidebarCollapsed;
  syncShellState();
}

function closeDetailPane() {
  detailOpen = false;
  selectedRecordId = null;
  document.getElementById('detail-pane').classList.remove('hidden');
  document.getElementById('detail-empty').classList.remove('hidden');
  document.getElementById('detail-content').classList.add('hidden');
  syncShellState();
}

function renderDashboard() {
  var dashboard = document.getElementById('dashboard');
  var cards = modules.map(function(module) {
    var meta = getMeta(module);
    var count = moduleCounts[module] || 0;
    return '<button type="button" class="dashboard-card" onclick="selectModule(\'' + module + '\')">' +
      '<div class="dashboard-card-top"><span class="dashboard-icon">' + meta.icon + '</span><span class="dashboard-count">' + count + '</span></div>' +
      '<div class="dashboard-title">' + meta.title + '</div>' +
      '<div class="dashboard-subtitle">' + meta.subtitle + '</div>' +
      '</button>';
  }).join('');

  dashboard.innerHTML = '<section class="dashboard-grid">' + cards + '</section>' +
    '<section class="dashboard-band">' +
    '<div class="dashboard-panel"><h3>Pipeline posture</h3><p>' + (moduleCounts.opportunities || 0) + ' opportunities and ' + (moduleCounts.leads || 0) + ' leads loaded locally.</p></div>' +
    '<div class="dashboard-panel"><h3>Capture</h3><p>Business-card scan is wired into the shell and ready for richer parsing.</p></div>' +
    '<div class="dashboard-panel"><h3>Data ownership</h3><p>SQLite-backed, local-first, with the HTTP surface already feature-gated as optional.</p></div>' +
    '</section>';
}

function setMainState(state) {
  document.getElementById('dashboard').classList.toggle('hidden', state !== 'dashboard');
  document.getElementById('table-view').classList.toggle('hidden', state !== 'table');
  document.getElementById('board-view').classList.toggle('hidden', state !== 'board');
  document.getElementById('empty-state').classList.toggle('hidden', state !== 'empty');
}

function renderTable(rows) {
  if (!rows.length) {
    setMainState('empty');
    return;
  }
  setMainState('table');

  var meta = getMeta(currentModule);
  var columns = meta.columns.slice();
  document.getElementById('table-head').innerHTML = '<tr>' + columns.map(function(key) {
    return '<th>' + titleize(key === 'full_name' ? 'name' : key) + '</th>';
  }).join('') + '</tr>';

  document.getElementById('table-body').innerHTML = rows.map(function(row) {
    var label = getRecordLabel(currentModule, row);
    var subtitle = getRecordSubtitle(currentModule, row);
    var selected = row.id === selectedRecordId ? ' selected' : '';
    return '<tr class="data-row' + selected + '" onclick="openDetail(\'' + currentModule + '\',\'' + row.id + '\')">' + columns.map(function(key, index) {
      var value = formatValue(key, getCellValue(row, key), row);
      if (index === 0) {
        return '<td><div class="primary-cell"><div class="primary-title">' + value + '</div><div class="primary-subtitle">' + (subtitle || label) + '</div></div></td>';
      }
      return '<td>' + value + '</td>';
    }).join('') + '</tr>';
  }).join('');
}

function renderBoard(rows) {
  if (!rows.length) {
    setMainState('empty');
    return;
  }
  setMainState('board');

  var groups = getBoardGroups(currentModule, rows);
  var activeStage = ensureBoardStage(currentModule, groups);
  var html = '<div class="stage-strip">' + groups.map(function(group) {
    return '<button type="button" class="stage-pill' + (group.id === activeStage ? ' active' : '') + '" data-stage-id="' + group.id + '" onclick="scrollToBoardStage(\'' + group.id + '\')">' +
      '<span class="stage-pill-label">' + group.name + '</span>' +
      '<span class="stage-pill-count">' + group.items.length + '</span>' +
      '</button>';
  }).join('') + '</div>' +
  '<div class="stage-stack">' + groups.map(function(group) {
    var rollup = currentModule === 'opportunities' && group.totalAmount > 0
      ? formatValue('amount', group.totalAmount, {})
      : group.items.length + ' records';
    var collapsed = !!collapsedStages[stageCollapseKey(currentModule, group.id)];
    return '<section class="stage-section" data-stage-id="' + group.id + '">' +
      '<div class="stage-section-header' + (collapsed ? ' collapsed' : '') + '" onclick="toggleStageSection(\'' + group.id + '\')">' +
        '<div><div class="stage-section-kicker">' + titleize(currentModule) + '</div><div class="stage-section-title">' + group.name + '</div></div>' +
        '<div class="stage-section-tools"><div class="stage-section-rollup">' + rollup + '</div><div class="stage-section-chevron">' + (collapsed ? '\u25B6' : '\u25BC') + '</div></div>' +
      '</div>' +
      '<div class="stage-section-list' + (collapsed ? ' hidden' : '') + '">' + group.items.map(function(row) {
        return '<button type="button" class="focused-card tone-' + getStatusTone(currentModule, row) + '" onclick="openDetail(\'' + currentModule + '\',\'' + row.id + '\')">' +
          '<div class="focused-card-main">' +
            '<div class="focused-card-title">' + getRecordLabel(currentModule, row) + '</div>' +
            '<div class="focused-card-subtitle">' + (getRecordSubtitle(currentModule, row) || 'No secondary context yet') + '</div>' +
          '</div>' +
          '<div class="focused-card-side"><div>' + (row.amount ? formatValue('amount', row.amount, row) : (row.status || row.sales_stage || 'Open')) + '</div><div class="focused-card-actions"><span class="inline-action">Open</span><span class="inline-action">Advance</span></div></div>' +
        '</button>';
      }).join('') + '</div>' +
    '</section>';
  }).join('') + '</div>';
  document.getElementById('board-view').innerHTML = html;
  bindBoardScroll();
  updateStageStripActive(activeStage);
}

function scrollToBoardStage(stageIdValue) {
  currentBoardStage = stageIdValue;
  updateStageStripActive(stageIdValue);
  var boardView = document.getElementById('board-view');
  var target = boardView && boardView.querySelector('.stage-section[data-stage-id="' + stageIdValue + '"]');
  if (!boardView || !target) return;
  var stripHeight = (boardView.querySelector('.stage-strip') || {}).offsetHeight || 40;
  boardView.scrollTo({ top: target.offsetTop - stripHeight + 2, behavior: 'smooth' });
}

function toggleStageSection(stageIdValue) {
  var key = stageCollapseKey(currentModule, stageIdValue);
  collapsedStages[key] = !collapsedStages[key];
  applyCurrentView();
}

function renderDetail(module, record, relationships) {
  var meta = getMeta(module);
  detailOpen = true;
  document.getElementById('detail-pane').classList.remove('hidden');
  document.getElementById('detail-empty').classList.add('hidden');
  document.getElementById('detail-content').classList.remove('hidden');
  syncShellState();

  var title = getRecordLabel(module, record);
  var subtitle = getRecordSubtitle(module, record);
  var summaryBits = meta.summary.map(function(key) {
    var value = formatValue(key, getCellValue(record, key), record);
    if (value === '\u2014') return '';
    return '<span class="detail-chip tone-' + getStatusTone(module, record) + '">' + value + '</span>';
  }).filter(Boolean).slice(0, 2).join('');

  document.getElementById('detail-header').innerHTML =
    '<div class="detail-kicker">' + meta.title + '</div>' +
    '<div class="detail-title-row"><h2>' + title + '</h2><div class="detail-actions"><button type="button" class="tool-btn" onclick="toast(\'Edit flow coming next\')">Edit</button><button type="button" class="tool-btn" onclick="toast(\'Activity log coming next\')">Add note</button></div></div>' +
    '<div class="detail-subtitle">' + (subtitle || meta.subtitle) + '</div>' +
    '<div class="detail-chip-row">' + summaryBits + '</div>';

  var coreFields = Object.keys(record).filter(function(key) {
    return ['id', 'date_entered', 'date_modified', 'modified_user_id', 'created_by', 'deleted', 'description', 'id_c'].indexOf(key) === -1 && record[key] !== null && record[key] !== '';
  });

  var overviewFields = coreFields.slice(0, 10).map(function(key) {
    return '<div class="detail-field"><span class="label">' + titleize(key) + '</span><span class="value">' + formatValue(key, getCellValue(record, key), record) + '</span></div>';
  }).join('');

  var relatedHtml = relationships && relationships.length
    ? relationships.map(function(rel) {
        return '<button type="button" class="related-row" onclick="openDetail(\'' + (rel.first_name ? 'contacts' : 'accounts') + '\',\'' + rel.id + '\')">' +
          '<span class="related-title">' + getRecordLabel(rel.first_name ? 'contacts' : 'accounts', rel) + '</span>' +
          '<span class="related-subtitle">' + (getRecordSubtitle(rel.first_name ? 'contacts' : 'accounts', rel) || 'Linked record') + '</span>' +
          '</button>';
      }).join('')
    : '<div class="section-empty">No linked records yet.</div>';

  var activityHtml = '<div class="timeline-entry"><div class="timeline-dot"></div><div><div class="timeline-title">Record loaded</div><div class="timeline-subtitle">Ready for notes, tasks, and history next.</div></div></div>' +
    '<div class="timeline-entry"><div class="timeline-dot muted"></div><div><div class="timeline-title">Portable identity groundwork</div><div class="timeline-subtitle">DID and consent layers can slot into this inspector cleanly.</div></div></div>';

  document.getElementById('detail-sections').innerHTML =
    '<section class="detail-section"><div class="section-head">Overview</div><div class="detail-fields">' + overviewFields + '</div></section>' +
    '<section class="detail-section"><div class="section-head">Related</div><div class="related-list">' + relatedHtml + '</div></section>' +
    '<section class="detail-section"><div class="section-head">Activity</div><div class="timeline">' + activityHtml + '</div></section>';
}

function applyCurrentView() {
  var filtered = filterRows(currentRows);
  if (!currentModule) {
    setMainState('dashboard');
    renderDashboard();
    closeDetailPane();
    return;
  }
  renderToolbar(currentModule);
  if (!filtered.length) {
    setMainState('empty');
    return;
  }
  if (currentView === 'board') renderBoard(filtered);
  else renderTable(filtered);
}

function changeView(view) {
  currentView = view;
  if (view === 'board') currentBoardStage = null;
  renderToolbar(currentModule);
  applyCurrentView();
}

async function selectModule(module) {
  currentModule = module;
  currentView = getMeta(module).defaultView;
  currentBoardStage = null;
  closeDetailPane();
  renderSidebar();
  renderToolbar(module);
  document.getElementById('loading').classList.remove('hidden');
  try {
    var result = await api.get('/api/v1/' + module + '?limit=50');
    currentRows = result.data || [];
    moduleCounts[module] = result.total || currentRows.length;
    document.getElementById('loading').classList.add('hidden');
    applyCurrentView();
  } catch (e) {
    document.getElementById('loading').classList.add('hidden');
    setMainState('empty');
    log('error', 'selectModule error ' + e);
    toast('Failed to load ' + titleize(module));
  }
}

async function openDetail(module, id) {
  selectedRecordId = id;
  try {
    var result = await api.get('/api/v1/' + module + '/' + id);
    renderDetail(module, result.data || {}, result.relationships || []);
    applyCurrentView();
  } catch (e) {
    log('error', 'openDetail error ' + e);
    toast('Could not open record');
  }
}

function bindControls() {
  document.getElementById('sidebar-toggle').addEventListener('click', function() {
    toggleSidebar();
  });

  document.getElementById('detail-close').addEventListener('click', function() {
    closeDetailPane();
    applyCurrentView();
  });

  document.getElementById('global-search').addEventListener('input', function(event) {
    searchQuery = event.target.value.trim();
    applyCurrentView();
  });

  document.getElementById('new-record-btn').addEventListener('click', function() {
    toast('Quick create is the next move.');
  });

  document.getElementById('filter-btn').addEventListener('click', function() {
    toast('Saved filters and advanced slices are next.');
  });
}

async function init() {
  try {
    show('\u25cf loading...', 'var(--warning)');
    renderOverview();
    bindControls();
    var data = await api.get('/api/v1/modules');
    modules = data.modules.filter(function(module) {
      return !module.startsWith('_') && !module.includes('_contacts') && !module.includes('_opportunities') && !module.includes('_cases');
    });

    // Build definition lookup from API response
    if (data.definitions) {
      data.definitions.forEach(function(def) {
        moduleDefs[def.module] = def;
      });
    }

    modules.forEach(function(module) {
      moduleCounts[module] = 0;
    });

    renderSidebar();
    renderToolbar(null);
    renderDashboard();
    syncShellState();
    setMainState('dashboard');
    show('\u25cf ready', 'var(--success)');

    for (var i = 0; i < modules.length; i += 1) {
      try {
        var result = await api.get('/api/v1/' + modules[i] + '?limit=1');
        moduleCounts[modules[i]] = result.total || 0;
      } catch (e) {
        log('warn', 'count preload failed for ' + modules[i]);
      }
    }
    renderSidebar();
    renderDashboard();
  } catch (e) {
    show('\u25cf ' + e, 'var(--danger)');
    log('error', 'init error', e);
  }
}

function handleScan(file) {
  if (!file) return;
  document.getElementById('scan-overlay').classList.remove('hidden');
  document.getElementById('scan-status').textContent = 'Scanner wiring is present. Local model hookup is next.';
}

function discardScan() {
  document.getElementById('scan-overlay').classList.add('hidden');
}

function saveScan() {
  toast('Scan import flow is queued behind quick-create.');
}

window.selectModule = selectModule;
window.changeView = changeView;
window.openDetail = openDetail;
window.scrollToBoardStage = scrollToBoardStage;
window.toggleStageSection = toggleStageSection;
window.handleScan = handleScan;
window.discardScan = discardScan;
window.saveScan = saveScan;
window.toast = toast;

init();
