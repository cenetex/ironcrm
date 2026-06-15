var statusEl = document.getElementById('db-status');
var modules = [];
var moduleCounts = {};
var dashboardPreviewRows = {};
var moduleDefs = {};   // module -> ModuleDefinition from API
var currentModule = null;
var currentView = 'table';
var currentRows = [];
var selectedRecordId = null;
var searchQuery = '';
var sidebarCollapsed = false;
var detailOpen = false;
var currentMainState = 'dashboard';
var currentBoardStage = null;
var collapsedStages = {};
var currentDetailRecord = null;
var currentDetailModule = null;
var currentDetailMode = 'view';
var currentDetailRelationships = [];
var currentDetailActivities = [];
var pendingDeleteKey = null;
var importsMenuOpen = false;
var draftRecordId = null;
var searchResults = [];
var searchTimer = null;
var importProvider = 'suitecrm';
var suiteCrmMode = 'import';
var suiteCrmUrl = '';
var suiteCrmStatus = null;
var suiteCrmReview = null;
var suiteCrmUndoSnapshot = null;

var isTauri = !!(window.__TAURI_INTERNALS__);
var invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;

function show(msg, color) {
  statusEl.textContent = msg;
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
      if (path === '/api/v1/export') return invoke('export_workspace');
      if (path === '/api/v1/suitecrm/detect') return invoke('detect_suitecrm');
      if (path === '/api/v1/odoo/detect') return invoke('detect_odoo');
      var query = path.includes('?') ? path.split('?')[1] : '';
      var clean = path.split('?')[0];
      var parts = clean.split('/');
      if (clean === '/api/v1/search') {
        var searchParams = new URLSearchParams(query);
        return invoke('search_records', {
          query: searchParams.get('q') || '',
          limit: parseInt(searchParams.get('limit'), 10) || 25
        });
      }
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
  },
  post: async function(path, body) {
    if (isTauri && invoke) {
      var parts = path.split('/');
      if (path === '/api/v1/restore/exact') {
        return invoke('restore_workspace_exact', { bundle: body });
      }
      if (path === '/api/v1/restore') {
        return invoke('restore_workspace', { bundle: body });
      }
      if (path === '/api/v1/suitecrm/connect') {
        return invoke('connect_suitecrm', {
          databaseUrl: body.database_url || '',
          mode: body.mode || 'import'
        });
      }
      if (path === '/api/v1/odoo/connect') {
        return invoke('connect_odoo', {
          databaseUrl: body.database_url || '',
          mode: body.mode || 'import'
        });
      }
      if (parts.length === 6 && parts[1] === 'api' && parts[2] === 'v1' && parts[5] === 'notes') {
        return invoke('add_record_note', { module: parts[3], id: parts[4], note: body.note || '' });
      }
      if (parts.length === 6 && parts[1] === 'api' && parts[2] === 'v1' && parts[5] === 'links') {
        return invoke('link_record', {
          module: parts[3],
          id: parts[4],
          targetModule: body.target_module || '',
          targetId: body.target_id || '',
          relationshipType: body.relationship_type || 'related'
        });
      }
      if (parts.length === 4 && parts[1] === 'api' && parts[2] === 'v1') {
        return invoke('create_record', { module: parts[3], fields: body });
      }
      throw new Error('Unsupported path ' + path);
    }
    var res = await fetch(path, {
      method: "POST",
      headers: {"Content-Type": "application/json"},
      body: JSON.stringify(body)
    });
    if (!res.ok) {
      var err = await res.json().catch(function() { return {error: res.statusText}; });
      throw new Error(err.error || res.statusText);
    }
    return res.json();
  },
  put: async function(path, body) {
    if (isTauri && invoke) {
      var parts = path.split('/');
      if (parts.length === 5 && parts[1] === 'api' && parts[2] === 'v1') {
        return invoke('update_record', { module: parts[3], id: parts[4], fields: body });
      }
      throw new Error('Unsupported path ' + path);
    }
    var res = await fetch(path, {
      method: "PUT",
      headers: {"Content-Type": "application/json"},
      body: JSON.stringify(body)
    });
    if (!res.ok) {
      var err = await res.json().catch(function() { return {error: res.statusText}; });
      throw new Error(err.error || res.statusText);
    }
    return res.json();
  },
  del: async function(path) {
    if (isTauri && invoke) {
      var parts = path.split('/');
      if (parts.length === 5 && parts[1] === 'api' && parts[2] === 'v1' && parts[3] === 'links') {
        return invoke('delete_record_link', { linkId: parts[4] });
      }
      if (parts.length === 5 && parts[1] === 'api' && parts[2] === 'v1') {
        return invoke('delete_record', { module: parts[3], id: parts[4] });
      }
      throw new Error('Unsupported path ' + path);
    }
    var res = await fetch(path, { method: "DELETE" });
    if (!res.ok) {
      var err = await res.json().catch(function() { return {error: res.statusText}; });
      throw new Error(err.error || res.statusText);
    }
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

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function escapeAttr(s) {
  return escapeHtml(s);
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

function getNextBoardStage(module, row) {
  var def = moduleDefs[module];
  if (!def || !def.board_field || !def.board_stages || !def.board_stages.length) return null;
  var current = row[def.board_field] || def.board_stages[0];
  var normalized = String(current || '').toLowerCase();
  if (normalized.indexOf('closed') >= 0 || normalized.indexOf('converted') >= 0) return null;
  var index = def.board_stages.indexOf(current);
  if (index < 0) return def.board_stages[0];
  if (index >= def.board_stages.length - 1) return null;
  return def.board_stages[index + 1];
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
    if (row.__draft) return true;
    return Object.keys(row).some(function(key) {
      var value = getCellValue(row, key);
      return value !== null && value !== undefined && String(value).toLowerCase().indexOf(q) >= 0;
    });
  });
}

function renderOverview() {
  var dashboardActive = currentMainState === 'dashboard' && !currentModule ? ' active' : '';
  document.getElementById('sidebar-overview').innerHTML =
    '<button class="overview-item dashboard-home-item' + dashboardActive + '" type="button" data-action="show-dashboard">' +
      '<span class="overview-title">Dashboard</span>' +
      '<span class="overview-desc">KPIs, recent, pipeline, attention</span>' +
    '</button>';
}

function openOverviewItem(itemId) {
  if (itemId === 'pipeline' && modules.indexOf('opportunities') >= 0) {
    selectModule('opportunities');
    return;
  }
  if (itemId === 'inbox' && modules.indexOf('leads') >= 0) {
    selectModule('leads');
    return;
  }
  var fallback = modules.filter(function(module) {
    return (moduleCounts[module] || 0) > 0;
  })[0] || (modules.indexOf('contacts') >= 0 ? 'contacts' : modules[0]);
  if (fallback) selectModule(fallback);
}

function showDashboard() {
  currentModule = null;
  currentRows = [];
  selectedRecordId = null;
  currentBoardStage = null;
  closeDetailPane();
  renderOverview();
  renderSidebar();
  renderToolbar(null);
  renderDashboard();
  setMainState('dashboard');
}

function renderSidebar() {
  document.getElementById('module-list').innerHTML = modules.map(function(module) {
    var meta = getMeta(module);
    var count = moduleCounts[module] || 0;
    return '<button class="module-item' + (module === currentModule ? ' active' : '') + '" type="button" data-action="select-module" data-module="' + escapeAttr(module) + '">' +
      '<span class="module-icon">' + escapeHtml(meta.icon) + '</span>' +
      '<span class="module-copy"><span class="module-name">' + escapeHtml(meta.title) + '</span><span class="module-subtitle">' + escapeHtml(meta.subtitle) + '</span></span>' +
      '<span class="module-count">' + escapeHtml(count) + '</span>' +
      '</button>';
  }).join('');
}

function renderToolbar(module) {
  var meta = getMeta(module);
  var newButton = document.getElementById('new-record-btn');
  document.getElementById('view-eyebrow').textContent = module ? 'Module' : 'Workspace';
  document.getElementById('view-title').textContent = module ? meta.title : 'Dashboard';
  document.getElementById('view-meta').innerHTML = module
    ? '<div class="meta-chip count-chip">' + escapeHtml(moduleCounts[module] || 0) + ' records</div>'
    : '<div class="meta-chip count-chip">Local-first workspace</div>';

  document.getElementById('view-switcher').innerHTML = module
    ? meta.views.map(function(view) {
      return '<button type="button" class="segmented-btn' + (view === currentView ? ' active' : '') + '" data-action="change-view" data-view="' + escapeAttr(view) + '">' + escapeHtml(titleize(view)) + '</button>';
    }).join('')
    : '';

  newButton.classList.toggle('hidden', !module);
}

function renderSuiteCrmToolbar() {
  var provider = importProviderMeta();
  document.getElementById('view-eyebrow').textContent = 'Connection';
  document.getElementById('view-title').textContent = provider.label + ' Import';
  document.getElementById('view-meta').innerHTML = '<div class="meta-chip count-chip">Review before loading</div>';
  document.getElementById('view-switcher').innerHTML = '';
  document.getElementById('new-record-btn').classList.add('hidden');
}

function setImportsMenuOpen(open) {
  importsMenuOpen = Boolean(open);
  var button = document.getElementById('imports-btn');
  var menu = document.getElementById('imports-menu');
  if (!button || !menu) return;
  button.setAttribute('aria-expanded', importsMenuOpen ? 'true' : 'false');
  menu.classList.toggle('hidden', !importsMenuOpen);
}

function toggleImportsMenu() {
  setImportsMenuOpen(!importsMenuOpen);
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
  if (currentDetailMode === 'new') {
    removeDraftRow();
  }
  detailOpen = false;
  selectedRecordId = null;
  currentDetailRecord = null;
  currentDetailModule = null;
  currentDetailMode = 'view';
  currentDetailRelationships = [];
  currentDetailActivities = [];
  pendingDeleteKey = null;
  document.getElementById('detail-pane').classList.remove('hidden');
  document.getElementById('detail-empty').classList.remove('hidden');
  document.getElementById('detail-content').classList.add('hidden');
  syncShellState();
}

function suiteCrmModeCopy() {
  var provider = importProviderMeta();
  if (suiteCrmMode === 'sync_readonly') {
    return 'Read ' + provider.label + ' data without writing back or loading records into IronCRM yet.';
  }
  return 'Load ' + provider.label + ' records into this local workspace. Imported records become editable IronCRM records.';
}

function importProviderMeta() {
  if (importProvider === 'odoo') {
    return {
      key: 'odoo',
      label: 'Odoo',
      sourceLabel: 'Odoo',
      route: '/api/v1/odoo/connect',
      detectRoute: '/api/v1/odoo/detect',
      placeholder: 'postgres://odoo:odoo@127.0.0.1:5433/odoo',
      connectionName: 'PostgreSQL connection string',
      detecting: 'Looking for a local Odoo sandbox connection.',
      missing: 'Enter an Odoo PostgreSQL connection string.'
    };
  }
  return {
    key: 'suitecrm',
    label: 'SuiteCRM',
    sourceLabel: 'SuiteCRM',
    route: '/api/v1/suitecrm/connect',
    detectRoute: '/api/v1/suitecrm/detect',
    placeholder: 'mysql://user:password@localhost:3306/suitecrm',
    connectionName: 'Connection string',
    detecting: 'Looking for a local SuiteCRM config.php.',
    missing: 'Enter a SuiteCRM MySQL connection string.'
  };
}

function formatCount(value) {
  var number = Number(value || 0);
  return Number.isFinite(number) ? number.toLocaleString() : '0';
}

function renderSuiteCrmReview() {
  if (!suiteCrmReview) return '';
  var provider = importProviderMeta();
  var counts = Array.isArray(suiteCrmReview.module_counts) ? suiteCrmReview.module_counts : [];
  var countCards = counts.length ? counts.map(function(item) {
    return '<div class="suitecrm-count-card">' +
      '<span>' + escapeHtml(item.label || item.module || 'Module') + '</span>' +
      '<strong>' + escapeHtml(formatCount(item.records)) + '</strong>' +
    '</div>';
  }).join('') : '<div class="suitecrm-empty">No importable core records found.</div>';
  var warnings = Array.isArray(suiteCrmReview.warnings) ? suiteCrmReview.warnings : [];
  var warningHtml = warnings.length ? '<div class="suitecrm-warnings">' +
    warnings.map(function(warning) {
      return '<div class="suitecrm-warning">' + escapeHtml(warning) + '</div>';
    }).join('') +
  '</div>' : '<div class="suitecrm-clean">No blocking warnings found.</div>';
  var importActions = suiteCrmMode === 'import'
    ? '<button type="button" class="tool-btn primary-btn" data-action="confirm-suitecrm-import">Import now</button>'
    : '';
  return '<div class="suitecrm-review">' +
    '<div class="suitecrm-review-head">' +
      '<div><strong>' + escapeHtml(suiteCrmMode === 'import' ? 'Import review' : 'Read-only review') + '</strong><span>' + escapeHtml(formatCount(suiteCrmReview.records)) + ' records, ' + escapeHtml(formatCount(suiteCrmReview.relationships)) + ' links</span></div>' +
      '<div class="suitecrm-review-actions">' + importActions + '<button type="button" class="tool-btn" data-action="refresh-suitecrm-review">Refresh</button><button type="button" class="tool-btn" data-action="cancel-suitecrm-review">Clear</button></div>' +
    '</div>' +
    '<div class="suitecrm-count-grid">' + countCards + '</div>' +
    warningHtml +
  '</div>';
}

function renderSuiteCrmUndo() {
  if (!suiteCrmUndoSnapshot) return '';
  var provider = importProviderMeta();
  var summary = previewBundleSummary(suiteCrmUndoSnapshot.bundle || {});
  return '<div class="suitecrm-undo">' +
    '<div><strong>Undo snapshot ready</strong><span>' + escapeHtml(summary.records || 0) + ' records before the last import</span></div>' +
    '<button type="button" class="tool-btn danger-btn" data-action="undo-suitecrm-import">Undo import</button>' +
  '</div>';
}

function renderSuiteCrmWizard() {
  var wizard = document.getElementById('suitecrm-wizard');
  if (!wizard) return;
  var provider = importProviderMeta();
  var statusHtml = '';
  if (suiteCrmStatus) {
    statusHtml = '<div class="suitecrm-status ' + escapeAttr(suiteCrmStatus.type) + '">' +
      '<strong>' + escapeHtml(suiteCrmStatus.label) + '</strong>' +
      '<span>' + escapeHtml(suiteCrmStatus.message) + '</span>' +
    '</div>';
  }
  var actionLabel = suiteCrmMode === 'sync_readonly' ? 'Connect read-only' : 'Review import';
  wizard.innerHTML = '<section class="suitecrm-wizard-shell">' +
    '<div class="suitecrm-wizard-hero">' +
      '<div>' +
        '<div class="section-label">' + escapeHtml(provider.sourceLabel) + '</div>' +
        '<h2>Import wizard</h2>' +
        '<p>Detect a local ' + escapeHtml(provider.sourceLabel) + ' database, review records and relationship quality, then load a reversible copy into this local workspace.</p>' +
      '</div>' +
      '<button type="button" class="tool-btn" data-action="show-dashboard">Back to dashboard</button>' +
    '</div>' +
    '<div class="suitecrm-wizard-grid">' +
      '<section class="suitecrm-step-card">' +
        '<div class="suitecrm-step-number">1</div>' +
        '<div class="suitecrm-step-body">' +
          '<h3>Source</h3>' +
          '<label class="suitecrm-url"><span>' + escapeHtml(provider.connectionName) + '</span><input id="suitecrm-url" type="text" autocomplete="off" spellcheck="false" placeholder="' + escapeAttr(provider.placeholder) + '" value="' + escapeAttr(suiteCrmUrl) + '"></label>' +
          '<div class="suitecrm-footer"><button type="button" class="tool-btn" data-action="detect-suitecrm">Detect local ' + escapeHtml(provider.sourceLabel) + '</button>' + statusHtml + '</div>' +
        '</div>' +
      '</section>' +
      '<section class="suitecrm-step-card">' +
        '<div class="suitecrm-step-number">2</div>' +
        '<div class="suitecrm-step-body">' +
          '<h3>Mode</h3>' +
          '<div class="suitecrm-mode-control" role="group" aria-label="SuiteCRM connection mode">' +
            '<button type="button" class="suitecrm-mode-btn' + (suiteCrmMode === 'import' ? ' active' : '') + '" data-action="select-suitecrm-mode" data-mode="import">Import (load one way)</button>' +
            '<button type="button" class="suitecrm-mode-btn' + (suiteCrmMode === 'sync_readonly' ? ' active' : '') + '" data-action="select-suitecrm-mode" data-mode="sync_readonly">Sync (read only mode)</button>' +
          '</div>' +
          '<div class="suitecrm-mode-copy">' + escapeHtml(suiteCrmModeCopy()) + '</div>' +
          '<div class="suitecrm-footer"><button type="button" class="tool-btn primary-btn" data-action="connect-suitecrm">' + escapeHtml(actionLabel) + '</button></div>' +
        '</div>' +
      '</section>' +
      '<section class="suitecrm-step-card suitecrm-step-card-wide">' +
        '<div class="suitecrm-step-number">3</div>' +
        '<div class="suitecrm-step-body">' +
          '<h3>Review and load</h3>' +
          renderSuiteCrmReview() +
          renderSuiteCrmUndo() +
          (!suiteCrmReview && !suiteCrmUndoSnapshot ? '<div class="suitecrm-empty">Run review to see module counts, links, and warnings before importing.</div>' : '') +
        '</div>' +
      '</section>' +
    '</div>' +
  '</section>';
}

function openImportWizard(provider) {
  importProvider = provider === 'odoo' ? 'odoo' : 'suitecrm';
  suiteCrmMode = suiteCrmMode || 'import';
  currentModule = null;
  currentRows = [];
  selectedRecordId = null;
  currentBoardStage = null;
  closeDetailPane();
  renderSuiteCrmToolbar();
  renderSuiteCrmWizard();
  setMainState('suitecrm');
}

function openSuiteCrmWizard() {
  openImportWizard('suitecrm');
}

function openOdooWizard() {
  openImportWizard('odoo');
}

function renderDashboardKpis() {
  var totalRecords = modules.reduce(function(sum, module) {
    return sum + Number(moduleCounts[module] || 0);
  }, 0);
  var opportunities = Number(moduleCounts.opportunities || 0);
  var leads = Number(moduleCounts.leads || 0);
  var cases = Number(moduleCounts.cases || 0);
  var contacts = Number(moduleCounts.contacts || 0);
  var accounts = Number(moduleCounts.accounts || 0);
  var max = Math.max(totalRecords, opportunities + leads, leads + cases, contacts + accounts, 1);
  var items = [
    { label: 'Records', value: totalRecords, detail: modules.length + ' modules loaded', bars: [totalRecords] },
    { label: 'Pipeline', value: opportunities, detail: leads + ' leads in motion', bars: [opportunities, leads] },
    { label: 'Needs attention', value: leads + cases, detail: leads + ' leads, ' + cases + ' cases', bars: [leads, cases] },
    { label: 'Relationships', value: contacts + accounts, detail: contacts + ' contacts, ' + accounts + ' accounts', bars: [contacts, accounts] }
  ];
  return '<section class="dashboard-kpis">' + items.map(function(item) {
    var bars = item.bars.map(function(value) {
      var width = Math.max(8, Math.round((Number(value || 0) / max) * 100));
      return '<span style="width:' + escapeAttr(width) + '%"></span>';
    }).join('');
    return '<div class="dashboard-kpi-card">' +
      '<div class="dashboard-action-label">' + escapeHtml(item.label) + '</div>' +
      '<strong>' + escapeHtml(formatCount(item.value)) + '</strong>' +
      '<p>' + escapeHtml(item.detail) + '</p>' +
      '<div class="dashboard-mini-chart">' + bars + '</div>' +
    '</div>';
  }).join('') + '</section>';
}

function recordSortValue(row) {
  var value = row && (row.date_modified || row.date_entered || row.updated_at || row.created_at);
  var time = value ? Date.parse(value) : NaN;
  return Number.isFinite(time) ? time : 0;
}

function getDashboardPreview(module, limit) {
  return (dashboardPreviewRows[module] || []).slice(0, limit || 3);
}

function getRecentDashboardRows(limit) {
  var rows = [];
  modules.forEach(function(module) {
    getDashboardPreview(module, 3).forEach(function(row) {
      rows.push({ module: module, row: row });
    });
  });
  rows.sort(function(a, b) {
    var delta = recordSortValue(b.row) - recordSortValue(a.row);
    if (delta) return delta;
    return getRecordLabel(a.module, a.row).localeCompare(getRecordLabel(b.module, b.row));
  });
  return rows.slice(0, limit || 4);
}

function renderDashboardRecordRow(module, row, metaText) {
  var label = getRecordLabel(module, row);
  var subtitle = getRecordSubtitle(module, row) || getMeta(module).title;
  return '<button type="button" class="dashboard-record-row" data-action="open-detail" data-module="' + escapeAttr(module) + '" data-record-id="' + escapeAttr(row.id) + '">' +
    '<span class="dashboard-record-copy"><span class="dashboard-record-title">' + escapeHtml(label) + '</span><span class="dashboard-record-subtitle">' + escapeHtml(subtitle) + '</span></span>' +
    '<span class="dashboard-record-meta">' + escapeHtml(metaText || getMeta(module).title) + '</span>' +
  '</button>';
}

function renderDashboardRecordList(items, emptyText) {
  if (!items.length) return '<div class="dashboard-section-empty">' + escapeHtml(emptyText) + '</div>';
  return items.map(function(item) {
    return renderDashboardRecordRow(item.module, item.row, item.meta);
  }).join('');
}

function renderDashboardSections() {
  var recentItems = getRecentDashboardRows(4).map(function(item) {
    return { module: item.module, row: item.row, meta: getMeta(item.module).title };
  });
  var pipelineItems = getDashboardPreview('opportunities', 3).map(function(row) {
    return { module: 'opportunities', row: row, meta: row.sales_stage || 'Opportunity' };
  });
  var attentionItems = getDashboardPreview('leads', 2).map(function(row) {
    return { module: 'leads', row: row, meta: row.status || 'Lead' };
  }).concat(getDashboardPreview('cases', 2).map(function(row) {
    return { module: 'cases', row: row, meta: row.priority || row.status || 'Case' };
  })).slice(0, 4);
  return '<section class="dashboard-sections">' +
    '<div class="dashboard-section-card">' +
      '<div class="dashboard-section-head"><h3>Recent</h3><button type="button" class="detail-btn section-btn" data-action="overview" data-item-id="recent">Open</button></div>' +
      '<p>Jump back into local records without leaving the dashboard.</p>' +
      '<div class="dashboard-section-list">' + renderDashboardRecordList(recentItems, 'No local records yet.') + '</div>' +
    '</div>' +
    '<div class="dashboard-section-card">' +
      '<div class="dashboard-section-head"><h3>Pipeline</h3><button type="button" class="detail-btn section-btn" data-action="overview" data-item-id="pipeline">Open</button></div>' +
      '<p>Open active opportunities and keep the sales loop moving.</p>' +
      '<div class="dashboard-section-list">' + renderDashboardRecordList(pipelineItems, 'No opportunities yet.') + '</div>' +
    '</div>' +
    '<div class="dashboard-section-card">' +
      '<div class="dashboard-section-head"><h3>Needs attention</h3><button type="button" class="detail-btn section-btn" data-action="overview" data-item-id="inbox">Open</button></div>' +
      '<p>Start with leads and customer cases that need follow-up.</p>' +
      '<div class="dashboard-section-list">' + renderDashboardRecordList(attentionItems, 'No leads or cases need attention.') + '</div>' +
    '</div>' +
  '</section>';
}

function renderDashboard() {
  var dashboard = document.getElementById('dashboard');
  dashboard.innerHTML = renderDashboardKpis() + renderDashboardSections();
}

function renderSearchToolbar() {
  document.getElementById('view-eyebrow').textContent = 'Workspace';
  document.getElementById('view-title').textContent = 'Search';
  document.getElementById('view-meta').innerHTML = '<div class="meta-chip count-chip">' + escapeHtml(searchResults.length) + ' matches</div>';
  document.getElementById('view-switcher').innerHTML = '';
  document.getElementById('new-record-btn').classList.add('hidden');
}

function renderSearchResults(isLoading) {
  setMainState('dashboard');
  renderSearchToolbar();
  var dashboard = document.getElementById('dashboard');
  if (isLoading) {
    dashboard.innerHTML = '<section class="search-results"><div class="section-empty">Searching...</div></section>';
    return;
  }
  if (!searchResults.length) {
    dashboard.innerHTML = '<section class="search-results"><div class="section-empty">No matching records yet.</div></section>';
    return;
  }

  dashboard.innerHTML = '<section class="search-results">' + searchResults.map(function(row) {
    var module = row._module || '';
    var meta = getMeta(module);
    var match = row._match === 'activity' ? 'Activity match' : meta.title;
    return '<button type="button" class="search-result-row" data-action="open-detail" data-module="' + escapeAttr(module) + '" data-record-id="' + escapeAttr(row.id) + '">' +
      '<span class="search-result-icon">' + escapeHtml(meta.icon) + '</span>' +
      '<span class="search-result-copy"><span class="search-result-title">' + escapeHtml(getRecordLabel(module, row)) + '</span><span class="search-result-subtitle">' + escapeHtml(match + ' · ' + (getRecordSubtitle(module, row) || meta.subtitle)) + '</span></span>' +
      '</button>';
  }).join('') + '</section>';
}

function scheduleGlobalSearch() {
  if (searchTimer) clearTimeout(searchTimer);
  if (!searchQuery) {
    searchResults = [];
    applyCurrentView();
    return;
  }
  renderSearchResults(true);
  searchTimer = setTimeout(runGlobalSearch, 180);
}

async function runGlobalSearch() {
  var query = searchQuery;
  try {
    var result = await api.get('/api/v1/search?q=' + encodeURIComponent(query) + '&limit=50');
    if (query !== searchQuery) return;
    searchResults = result.data || [];
    renderSearchResults(false);
  } catch (e) {
    log('error', 'search error ' + e);
    searchResults = [];
    renderSearchResults(false);
    toast('Search failed');
  }
}

function setMainState(state) {
  currentMainState = state;
  document.getElementById('dashboard').classList.toggle('hidden', state !== 'dashboard');
  document.getElementById('suitecrm-wizard').classList.toggle('hidden', state !== 'suitecrm');
  document.getElementById('table-view').classList.toggle('hidden', state !== 'table');
  document.getElementById('board-view').classList.toggle('hidden', state !== 'board');
  document.getElementById('empty-state').classList.toggle('hidden', state !== 'empty');
  renderOverview();
  var emptyCreateBtn = document.getElementById('empty-create-btn');
  var emptyCopy = document.getElementById('empty-state-copy');
  if (emptyCreateBtn) emptyCreateBtn.classList.toggle('hidden', !currentModule);
  if (emptyCopy) {
    emptyCopy.textContent = currentModule
      ? 'Create the first record in this module.'
      : 'Choose a module or import data to get started.';
  }
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
    return '<th>' + escapeHtml(titleize(key === 'full_name' ? 'name' : key)) + '</th>';
  }).join('') + '</tr>';

  document.getElementById('table-body').innerHTML = rows.map(function(row) {
    var label = getRecordLabel(currentModule, row);
    var subtitle = getRecordSubtitle(currentModule, row);
    var selected = row.id === selectedRecordId ? ' selected' : '';
    var rowAction = row.__draft ? 'edit-draft' : 'open-detail';
    return '<tr class="data-row' + selected + (row.__draft ? ' draft-row' : '') + '" data-action="' + rowAction + '" data-module="' + escapeAttr(currentModule) + '" data-record-id="' + escapeAttr(row.id) + '">' + columns.map(function(key, index) {
      var value = formatValue(key, getCellValue(row, key), row);
      if (index === 0) {
        return '<td><div class="primary-cell"><div class="primary-title">' + escapeHtml(value) + '</div><div class="primary-subtitle">' + escapeHtml(subtitle || label) + '</div></div></td>';
      }
      return '<td>' + escapeHtml(value) + '</td>';
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
    return '<button type="button" class="stage-pill' + (group.id === activeStage ? ' active' : '') + '" data-action="scroll-stage" data-stage-id="' + escapeAttr(group.id) + '">' +
      '<span class="stage-pill-label">' + escapeHtml(group.name) + '</span>' +
      '<span class="stage-pill-count">' + escapeHtml(group.items.length) + '</span>' +
      '</button>';
  }).join('') + '</div>' +
  '<div class="stage-stack">' + groups.map(function(group) {
    var rollup = currentModule === 'opportunities' && group.totalAmount > 0
      ? formatValue('amount', group.totalAmount, {})
      : group.items.length + ' records';
    var collapsed = !!collapsedStages[stageCollapseKey(currentModule, group.id)];
    return '<section class="stage-section" data-stage-id="' + escapeAttr(group.id) + '">' +
      '<div class="stage-section-header' + (collapsed ? ' collapsed' : '') + '" data-action="toggle-stage" data-stage-id="' + escapeAttr(group.id) + '">' +
        '<div><div class="stage-section-kicker">' + escapeHtml(titleize(currentModule)) + '</div><div class="stage-section-title">' + escapeHtml(group.name) + '</div></div>' +
        '<div class="stage-section-tools"><div class="stage-section-rollup">' + escapeHtml(rollup) + '</div><div class="stage-section-chevron">' + (collapsed ? '\u25B6' : '\u25BC') + '</div></div>' +
      '</div>' +
      '<div class="stage-section-list' + (collapsed ? ' hidden' : '') + '">' + group.items.map(function(row) {
        var nextStage = getNextBoardStage(currentModule, row);
        var actionHtml = '<span class="inline-action">Open</span>' + (nextStage
          ? '<span class="inline-action action-click" data-action="advance-record" data-record-id="' + escapeAttr(row.id) + '">Advance</span>'
          : '<span class="inline-action">Final</span>');
        return '<button type="button" class="focused-card tone-' + escapeAttr(getStatusTone(currentModule, row)) + '" data-action="open-detail" data-module="' + escapeAttr(currentModule) + '" data-record-id="' + escapeAttr(row.id) + '">' +
          '<div class="focused-card-main">' +
            '<div class="focused-card-title">' + escapeHtml(getRecordLabel(currentModule, row)) + '</div>' +
            '<div class="focused-card-subtitle">' + escapeHtml(getRecordSubtitle(currentModule, row) || 'Open record') + '</div>' +
          '</div>' +
          '<div class="focused-card-side"><div>' + escapeHtml(row.amount ? formatValue('amount', row.amount, row) : (row.status || row.sales_stage || 'Open')) + '</div><div class="focused-card-actions">' + actionHtml + '</div></div>' +
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

async function advanceRecord(event, id) {
  if (event) event.stopPropagation();
  var row = currentRows.filter(function(item) { return item.id === id; })[0];
  if (!row || !currentModule) return;
  var def = moduleDefs[currentModule];
  var nextStage = getNextBoardStage(currentModule, row);
  if (!def || !def.board_field || !nextStage) return;

  var fields = {};
  fields[def.board_field] = nextStage;
  try {
    await api.put('/api/v1/' + currentModule + '/' + id, fields);
    toast('Moved to ' + nextStage);
    await refreshCurrentModule();
    if (selectedRecordId === id) await openDetail(currentModule, id);
  } catch (e) {
    toast('Error: ' + e.message);
  }
}

async function updateCurrentStage(stage) {
  if (!currentDetailModule || !currentDetailRecord || !currentDetailRecord.id) return;
  var def = moduleDefs[currentDetailModule];
  if (!def || !def.board_field || !stage) return;

  var fields = {};
  fields[def.board_field] = stage;
  try {
    await api.put('/api/v1/' + currentDetailModule + '/' + currentDetailRecord.id, fields);
    toast('Stage updated');
    var id = currentDetailRecord.id;
    await refreshCurrentModule();
    await openDetail(currentDetailModule, id);
  } catch (e) {
    toast('Error: ' + e.message);
  }
}

function makeDetailKey(module, record) {
  return module + ':' + (record && record.id ? record.id : '');
}

function renderDetailActions(module, record) {
  if (pendingDeleteKey && pendingDeleteKey === makeDetailKey(module, record)) {
    return '<button type="button" class="detail-btn" data-action="cancel-delete">Cancel</button><button type="button" class="detail-btn danger-detail-btn" data-action="confirm-delete-current">Delete now</button>';
  }
  return '<button type="button" class="detail-btn" data-action="edit-current">Edit</button><button type="button" class="detail-btn danger-detail-btn" data-action="delete-current">Delete</button>';
}

function renderDetail(module, record, relationships, activities) {
  var meta = getMeta(module);
  var def = moduleDefs[module];
  currentDetailModule = module;
  currentDetailRecord = record;
  currentDetailRelationships = relationships || [];
  currentDetailActivities = activities || [];
  currentDetailMode = 'view';
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
    return '<span class="detail-chip tone-' + escapeAttr(getStatusTone(module, record)) + '">' + escapeHtml(value) + '</span>';
  }).filter(Boolean).slice(0, 2).join('');
  var stageControl = def && def.board_field && def.board_stages && def.board_stages.length
    ? '<label class="stage-control"><span>' + escapeHtml(titleize(def.board_field)) + '</span><select id="stage-picker">' + def.board_stages.map(function(stage) {
        var selected = stage === record[def.board_field] ? ' selected' : '';
        return '<option value="' + escapeAttr(stage) + '"' + selected + '>' + escapeHtml(stage) + '</option>';
      }).join('') + '</select></label>'
    : '';
  var deleteConfirm = pendingDeleteKey === makeDetailKey(module, record)
    ? '<div class="detail-confirmation">Confirm delete for this record.</div>'
    : '';

  document.getElementById('detail-header').innerHTML =
    '<div class="detail-kicker-row"><div class="detail-kicker">' + escapeHtml(meta.title) + '</div><div class="detail-header-actions">' + renderDetailActions(module, record) + '</div></div>' +
    '<div class="detail-title-row"><h2>' + escapeHtml(title) + '</h2></div>' +
    '<div class="detail-subtitle">' + escapeHtml(subtitle || meta.subtitle) + '</div>' +
    '<div class="detail-chip-row">' + summaryBits + '</div>' +
    deleteConfirm +
    stageControl;

  var coreFields = Object.keys(record).filter(function(key) {
    return ['id', 'date_entered', 'date_modified', 'modified_user_id', 'created_by', 'deleted', 'description', 'id_c'].indexOf(key) === -1 && record[key] !== null && record[key] !== '';
  });

  var overviewFields = coreFields.slice(0, 10).map(function(key) {
    return '<div class="detail-field"><span class="label">' + escapeHtml(titleize(key)) + '</span><span class="value">' + escapeHtml(formatValue(key, getCellValue(record, key), record)) + '</span></div>';
  }).join('');

  var relatedHtml = relationships && relationships.length
    ? relationships.map(function(rel) {
        var relModule = rel._module || (rel.first_name ? 'contacts' : 'accounts');
        var unlink = rel._link_id
          ? '<button type="button" class="detail-btn quiet-btn" data-action="unlink-record" data-link-id="' + escapeAttr(rel._link_id) + '">Unlink</button>'
          : '';
        return '<div class="related-item">' +
          '<button type="button" class="related-row" data-action="open-detail" data-module="' + escapeAttr(relModule) + '" data-record-id="' + escapeAttr(rel.id) + '">' +
            '<span class="related-title">' + escapeHtml(getRecordLabel(relModule, rel)) + '</span>' +
            '<span class="related-subtitle">' + escapeHtml(getRecordSubtitle(relModule, rel) || rel._relationship_type || 'Linked record') + '</span>' +
          '</button>' +
          unlink +
        '</div>';
      }).join('')
    : '<div class="section-empty">No linked records yet.</div>';

  var moduleOptions = modules.map(function(moduleName) {
    return '<option value="' + escapeAttr(moduleName) + '">' + escapeHtml(getMeta(moduleName).title) + '</option>';
  }).join('');
  var linkComposer = '<div class="link-composer">' +
    '<div class="link-type-row"><select id="link-type-select"><option value="related">Related to</option><option value="employer">Employer</option><option value="decision_maker">Decision maker</option><option value="stakeholder">Stakeholder</option><option value="parent">Parent</option></select></div>' +
    '<div class="link-search-row"><select id="link-module-select">' + moduleOptions + '</select><input id="link-search-input" type="search" placeholder="Find record to link"></div>' +
    '<div id="link-candidates" class="link-candidates"></div>' +
    '</div>';

  var noteComposer = '<div class="note-composer"><textarea id="note-input" placeholder="Add a note"></textarea></div>';
  var activityHtml = activities && activities.length
    ? activities.map(renderActivityEntry).join('')
    : '<div class="section-empty">No activity yet.</div>';

  document.getElementById('detail-sections').innerHTML =
    '<section class="detail-section"><div class="section-head-row"><div class="section-head">Overview</div></div><div class="detail-fields">' + overviewFields + '</div></section>' +
    '<section class="detail-section"><div class="section-head-row"><div class="section-head">Related</div><button type="button" class="detail-btn section-btn" data-action="find-link-targets">Find</button></div>' + linkComposer + '<div class="related-list">' + relatedHtml + '</div></section>' +
    '<section class="detail-section"><div class="section-head-row"><div class="section-head">Activity</div><button type="button" class="detail-btn primary-detail-btn section-btn" data-action="save-note">Add note</button></div>' + noteComposer + '<div class="timeline">' + activityHtml + '</div></section>';
}

function applyCurrentView() {
  if (searchQuery) {
    renderSearchResults(false);
    return;
  }
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
  searchQuery = '';
  searchResults = [];
  var searchInput = document.getElementById('global-search');
  if (searchInput) searchInput.value = '';
  currentModule = module;
  currentView = getMeta(module).defaultView;
  currentBoardStage = null;
  draftRecordId = null;
  closeDetailPane();
  renderOverview();
  renderSidebar();
  renderToolbar(module);
  document.getElementById('loading').classList.remove('hidden');
  try {
    var result = await api.get('/api/v1/' + module + '?limit=50');
    currentRows = result.data || [];
    dashboardPreviewRows[module] = currentRows.slice(0, 3);
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
  pendingDeleteKey = null;
  selectedRecordId = id;
  try {
    var result = await api.get('/api/v1/' + module + '/' + id);
    renderDetail(module, result.data || {}, result.relationships || [], result.activities || []);
    if (searchQuery || currentModule) {
      applyCurrentView();
    } else {
      renderDashboard();
      setMainState('dashboard');
    }
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
    scheduleGlobalSearch();
  });

  document.getElementById('imports-btn').addEventListener('click', function(event) {
    event.preventDefault();
    event.stopPropagation();
    toggleImportsMenu();
  });

  document.addEventListener('click', function(event) {
    if (!event.target || !event.target.closest || !event.target.closest('#imports-control')) {
      setImportsMenuOpen(false);
    }
  });

  document.addEventListener('keydown', function(event) {
    if (event.key === 'Escape') setImportsMenuOpen(false);
  });

  document.addEventListener('input', function(event) {
    if (event.target && event.target.id === 'suitecrm-url') {
      suiteCrmUrl = event.target.value;
      suiteCrmReview = null;
    }
  });

  document.getElementById('new-record-btn').addEventListener('click', function() {
    if (currentModule) openNewRecordInline(currentModule);
  });

  document.getElementById('empty-create-btn').addEventListener('click', function() {
    if (currentModule) openNewRecordInline(currentModule);
  });

  document.addEventListener('click', handleActionClick);
  document.addEventListener('submit', function(event) {
    if (event.target && event.target.id === 'detail-form') {
      event.preventDefault();
      saveInlineEdit();
    }
  });
  document.addEventListener('change', function(event) {
    if (event.target && event.target.id === 'stage-picker') {
      updateCurrentStage(event.target.value);
    }
  });
}

function handleActionClick(event) {
  var target = event.target.closest('[data-action]');
  if (!target) return;

  var action = target.getAttribute('data-action');
  if (target.closest('#imports-menu')) setImportsMenuOpen(false);
  if (action === 'advance-record') {
    event.preventDefault();
    event.stopPropagation();
    advanceRecord(event, target.getAttribute('data-record-id'));
    return;
  }

  switch (action) {
    case 'show-dashboard':
      showDashboard();
      break;
    case 'open-suitecrm-wizard':
      openSuiteCrmWizard();
      break;
    case 'open-odoo-wizard':
      openOdooWizard();
      break;
    case 'overview':
      openOverviewItem(target.getAttribute('data-item-id'));
      break;
    case 'select-module':
      selectModule(target.getAttribute('data-module'));
      break;
    case 'change-view':
      changeView(target.getAttribute('data-view'));
      break;
    case 'open-detail':
      openDetail(target.getAttribute('data-module'), target.getAttribute('data-record-id'));
      break;
    case 'scroll-stage':
      scrollToBoardStage(target.getAttribute('data-stage-id'));
      break;
    case 'toggle-stage':
      toggleStageSection(target.getAttribute('data-stage-id'));
      break;
    case 'edit-current':
      editCurrentRecord();
      break;
    case 'delete-current':
      deleteCurrentRecord();
      break;
    case 'cancel-delete':
      cancelDeleteRecord();
      break;
    case 'confirm-delete-current':
      confirmDeleteCurrentRecord();
      break;
    case 'save-note':
      saveRecordNote();
      break;
    case 'find-link-targets':
      findLinkTargets();
      break;
    case 'link-record':
      linkRecord(target.getAttribute('data-target-module'), target.getAttribute('data-record-id'));
      break;
    case 'unlink-record':
      unlinkRecord(target.getAttribute('data-link-id'));
      break;
    case 'export-workspace':
      exportWorkspace();
      break;
    case 'restore-workspace':
      restoreWorkspace();
      break;
    case 'select-suitecrm-mode':
      selectSuiteCrmMode(target.getAttribute('data-mode'));
      break;
    case 'detect-suitecrm':
      detectSuiteCrm();
      break;
    case 'connect-suitecrm':
      connectSuiteCrm();
      break;
    case 'confirm-suitecrm-import':
      confirmSuiteCrmImport();
      break;
    case 'refresh-suitecrm-review':
      connectSuiteCrm();
      break;
    case 'cancel-suitecrm-review':
      suiteCrmReview = null;
      suiteCrmStatus = null;
      renderSuiteCrmWizard();
      break;
    case 'undo-suitecrm-import':
      undoSuiteCrmImport();
      break;
    case 'edit-draft':
      editDraftRecord();
      break;
    case 'cancel-inline':
      cancelInlineEdit();
      break;
    case 'save-inline':
      saveInlineEdit();
      break;
    default:
      return;
  }
}

// ── Inline detail editor ──

function renderFormFields(module, existing) {
  var def = moduleDefs[module];
  if (!def) return '<p class="section-empty">No definition for this module.</p>';
  var skip = ["id","date_entered","date_modified","modified_user_id","created_by","deleted","id_c"];
  var fields = def.fields.filter(function(f) { return skip.indexOf(f.name) === -1; });
  var primaryNames = getPrimaryFieldNames(module, fields);
  var primaryFields = fields.filter(function(f) { return primaryNames.indexOf(f.name) >= 0; });
  var extraFields = fields.filter(function(f) { return primaryNames.indexOf(f.name) < 0; });
  var primaryHtml = primaryFields.map(function(f) {
    return renderFormField(f, existing);
  }).join("");
  if (!extraFields.length) return primaryHtml;
  return primaryHtml +
    '<details class="more-fields"><summary>More fields</summary><div class="more-fields-body">' +
    extraFields.map(function(f) { return renderFormField(f, existing); }).join("") +
    '</div></details>';
}

function getPrimaryFieldNames(module, fields) {
  var meta = getMeta(module);
  var def = moduleDefs[module];
  if (def && def.primary_fields && def.primary_fields.length) {
    var availablePrimary = fields.map(function(f) { return f.name; });
    var explicit = [];
    def.primary_fields.forEach(function(name) {
      if (availablePrimary.indexOf(name) >= 0 && explicit.indexOf(name) < 0) explicit.push(name);
    });
    if (explicit.length) return explicit;
  }
  var preferred = []
    .concat(meta.columns || [])
    .concat(meta.summary || [])
    .concat(def && def.board_field ? [def.board_field] : [])
    .concat(['name', 'first_name', 'last_name', 'account_name', 'title', 'email1', 'phone_work', 'status', 'sales_stage', 'amount', 'date_closed', 'description']);
  var available = fields.map(function(f) { return f.name; });
  var names = [];
  preferred.forEach(function(name) {
    if (name === 'full_name') {
      ['first_name', 'last_name'].forEach(function(part) {
        if (available.indexOf(part) >= 0 && names.indexOf(part) < 0) names.push(part);
      });
      return;
    }
    if (available.indexOf(name) >= 0 && names.indexOf(name) < 0) names.push(name);
  });
  return names.length ? names : available.slice(0, 8);
}

function renderFormField(f, existing) {
    var value = (existing && existing[f.name]) || "";
    if (f.name.indexOf("description") >= 0 || f.name === "work_log") {
      return '<label>' + escapeHtml(titleize(f.name)) + '<textarea name="' + escapeAttr(f.name) + '">' + escapeHtml(value) + '</textarea></label>';
    }
    return '<label>' + escapeHtml(titleize(f.name)) + '<input name="' + escapeAttr(f.name) + '" value="' + escapeAttr(value) + '" placeholder="' + escapeAttr(titleize(f.name)) + '"></label>';
}

function summarizeChangedFields(raw) {
  try {
    var parsed = typeof raw === 'string' ? JSON.parse(raw) : raw;
    var keys = Object.keys(parsed || {}).filter(function(key) {
      return ['id', 'date_entered', 'date_modified', 'deleted'].indexOf(key) === -1;
    });
    if (!keys.length) return '';
    return keys.slice(0, 4).map(titleize).join(', ') + (keys.length > 4 ? ' +' + (keys.length - 4) : '');
  } catch (e) {
    return '';
  }
}

function activityFields(activity) {
  try {
    return typeof activity.changed_fields === 'string'
      ? JSON.parse(activity.changed_fields)
      : (activity.changed_fields || {});
  } catch (e) {
    return {};
  }
}

function renderActivityEntry(activity) {
  var fields = activityFields(activity);
  var operation = activity.operation || 'activity';
  var title = operation === 'note' ? 'Note' : titleize(operation);
  var detail = operation === 'note'
    ? (fields.note || '')
    : summarizeChangedFields(activity.changed_fields);
  var subtitle = (activity.created_at || '') + (detail ? ' · ' + detail : '');
  return '<div class="timeline-entry"><div class="timeline-dot"></div><div><div class="timeline-title">' + escapeHtml(title) + '</div><div class="timeline-subtitle">' + escapeHtml(subtitle) + '</div></div></div>';
}

async function saveRecordNote() {
  if (!currentDetailModule || !currentDetailRecord || !currentDetailRecord.id) return;
  var input = document.getElementById('note-input');
  var note = input ? input.value.trim() : '';
  if (!note) return;
  try {
    await api.post('/api/v1/' + currentDetailModule + '/' + currentDetailRecord.id + '/notes', { note: note });
    if (input) input.value = '';
    await openDetail(currentDetailModule, currentDetailRecord.id);
    toast('Note added');
  } catch (e) {
    toast('Error: ' + e.message);
  }
}

async function findLinkTargets() {
  if (!currentDetailRecord || !currentDetailRecord.id) return;
  var input = document.getElementById('link-search-input');
  var select = document.getElementById('link-module-select');
  var candidates = document.getElementById('link-candidates');
  var query = input ? input.value.trim() : '';
  var moduleFilter = select ? select.value : '';
  if (!candidates) return;
  if (!query) {
    candidates.innerHTML = '<div class="section-empty">Type a name, company, email, or note to search.</div>';
    return;
  }
  candidates.innerHTML = '<div class="section-empty">Searching...</div>';
  try {
    var result = await api.get('/api/v1/search?q=' + encodeURIComponent(query) + '&limit=12');
    var rows = (result.data || []).filter(function(row) {
      if (moduleFilter && row._module !== moduleFilter) return false;
      return !(row._module === currentDetailModule && row.id === currentDetailRecord.id);
    });
    renderLinkCandidates(rows);
  } catch (e) {
    candidates.innerHTML = '<div class="section-empty">Search failed.</div>';
  }
}

function renderLinkCandidates(rows) {
  var candidates = document.getElementById('link-candidates');
  if (!candidates) return;
  if (!rows.length) {
    candidates.innerHTML = '<div class="section-empty">No records found.</div>';
    return;
  }
  candidates.innerHTML = rows.map(function(row) {
    var module = row._module || '';
    var meta = getMeta(module);
    return '<button type="button" class="link-candidate" data-action="link-record" data-target-module="' + escapeAttr(module) + '" data-record-id="' + escapeAttr(row.id) + '">' +
      '<span class="search-result-icon">' + escapeHtml(meta.icon) + '</span>' +
      '<span class="search-result-copy"><span class="search-result-title">' + escapeHtml(getRecordLabel(module, row)) + '</span><span class="search-result-subtitle">' + escapeHtml(meta.title + ' · ' + (getRecordSubtitle(module, row) || meta.subtitle)) + '</span></span>' +
      '</button>';
  }).join('');
}

async function linkRecord(targetModule, targetId) {
  if (!currentDetailModule || !currentDetailRecord || !currentDetailRecord.id || !targetModule || !targetId) return;
  var typeSelect = document.getElementById('link-type-select');
  var relationshipType = typeSelect ? typeSelect.value : 'related';
  try {
    await api.post('/api/v1/' + currentDetailModule + '/' + currentDetailRecord.id + '/links', {
      target_module: targetModule,
      target_id: targetId,
      relationship_type: relationshipType
    });
    await openDetail(currentDetailModule, currentDetailRecord.id);
    toast('Linked');
  } catch (e) {
    toast('Error: ' + e.message);
  }
}

async function unlinkRecord(linkId) {
  if (!linkId || !currentDetailModule || !currentDetailRecord || !currentDetailRecord.id) return;
  try {
    await api.del('/api/v1/links/' + linkId);
    await openDetail(currentDetailModule, currentDetailRecord.id);
    toast('Unlinked');
  } catch (e) {
    toast('Error: ' + e.message);
  }
}

function readSuiteCrmUrl() {
  var input = document.getElementById('suitecrm-url');
  if (input) suiteCrmUrl = input.value;
  return suiteCrmUrl.trim();
}

function selectSuiteCrmMode(mode) {
  readSuiteCrmUrl();
  suiteCrmMode = mode === 'sync_readonly' ? 'sync_readonly' : 'import';
  suiteCrmStatus = null;
  suiteCrmReview = null;
  renderSuiteCrmWizard();
}

async function detectSuiteCrm() {
  var provider = importProviderMeta();
  readSuiteCrmUrl();
  suiteCrmStatus = {
    type: 'pending',
    label: 'Detecting',
    message: provider.detecting
  };
  renderSuiteCrmWizard();
  try {
    var result = await api.get(provider.detectRoute);
    if (result.found && result.database_url) {
      suiteCrmUrl = result.database_url;
      suiteCrmReview = null;
      suiteCrmStatus = {
        type: 'success',
        label: 'Detected',
        message: result.config_path ? 'Found ' + result.config_path : result.message
      };
      renderSuiteCrmWizard();
      toast(provider.sourceLabel + ' connection detected');
      return;
    }
    suiteCrmStatus = {
      type: 'error',
      label: 'Not found',
      message: result.message || ('No local ' + provider.sourceLabel + ' source found.')
    };
    renderSuiteCrmWizard();
  } catch (e) {
    suiteCrmStatus = {
      type: 'error',
      label: 'Detection failed',
      message: e.message
    };
    renderSuiteCrmWizard();
  }
}

async function connectSuiteCrm() {
  var provider = importProviderMeta();
  var databaseUrl = readSuiteCrmUrl();
  if (!databaseUrl) {
    suiteCrmStatus = {
      type: 'error',
      label: 'Missing connection',
      message: provider.missing
    };
    renderSuiteCrmWizard();
    return;
  }

  suiteCrmStatus = {
    type: 'pending',
    label: 'Reviewing',
    message: 'Checking ' + provider.sourceLabel + ' modules, record counts, links, and warnings.'
  };
  renderSuiteCrmWizard();

  try {
    var result = await api.post(provider.route, {
      database_url: databaseUrl,
      mode: 'sync_readonly'
    });
    result._url = databaseUrl;
    result._provider = importProvider;
    suiteCrmReview = result;
    suiteCrmStatus = {
      type: 'success',
      label: suiteCrmMode === 'sync_readonly' ? 'Read-only connection ready' : 'Import review ready',
      message: (result.records || 0) + ' records and ' + (result.relationships || 0) + ' links ready to review.'
    };
    renderSuiteCrmWizard();
    toast(suiteCrmMode === 'sync_readonly' ? provider.sourceLabel + ' read-only review ready' : provider.sourceLabel + ' import review ready');
  } catch (e) {
    suiteCrmStatus = {
      type: 'error',
      label: 'Review failed',
      message: e.message
    };
    renderSuiteCrmWizard();
    toast(provider.sourceLabel + ' review failed');
  }
}

async function confirmSuiteCrmImport() {
  var provider = importProviderMeta();
  var databaseUrl = readSuiteCrmUrl();
  if (!databaseUrl) {
    suiteCrmStatus = {
      type: 'error',
      label: 'Missing connection',
      message: provider.missing
    };
    renderSuiteCrmWizard();
    return;
  }
  if (!suiteCrmReview || suiteCrmReview._url !== databaseUrl || suiteCrmReview._provider !== importProvider) {
    await connectSuiteCrm();
    return;
  }

  suiteCrmStatus = {
    type: 'pending',
    label: 'Snapshotting',
    message: 'Saving the current workspace before import.'
  };
  renderSuiteCrmWizard();

  try {
    var snapshot = await api.get('/api/v1/export');
    suiteCrmStatus = {
    type: 'pending',
    label: 'Importing',
      message: 'Loading reviewed ' + provider.sourceLabel + ' records into this workspace.'
    };
    renderSuiteCrmWizard();

    var result = await api.post(provider.route, {
      database_url: databaseUrl,
      mode: 'import'
    });
    result._url = databaseUrl;
    result._provider = importProvider;
    suiteCrmReview = result;
    suiteCrmUndoSnapshot = {
      bundle: snapshot,
      importedAt: new Date().toISOString(),
      result: result
    };
    suiteCrmStatus = {
      type: 'success',
      label: 'Import complete',
      message: (result.records || 0) + ' records loaded. Undo is available for this session.'
    };
    toast(result.message || (provider.sourceLabel + ' import complete'));
    await reloadWorkspace();
    openSuiteCrmWizard();
  } catch (e) {
    suiteCrmStatus = {
      type: 'error',
      label: 'Import failed',
      message: e.message
    };
    renderSuiteCrmWizard();
    toast(provider.sourceLabel + ' import failed');
  }
}

async function undoSuiteCrmImport() {
  if (!suiteCrmUndoSnapshot || !suiteCrmUndoSnapshot.bundle) {
    toast('No import snapshot available');
    return;
  }
  var provider = importProviderMeta();
  var ok = confirm('Undo the last ' + provider.sourceLabel + ' import?\\n\\nThis restores the workspace snapshot taken immediately before import.');
  if (!ok) return;

  suiteCrmStatus = {
    type: 'pending',
    label: 'Undoing',
    message: 'Restoring the pre-import workspace snapshot.'
  };
  renderSuiteCrmWizard();

  try {
    var summary = await api.post('/api/v1/restore/exact', suiteCrmUndoSnapshot.bundle);
    suiteCrmUndoSnapshot = null;
    suiteCrmReview = null;
    suiteCrmStatus = {
      type: 'success',
      label: 'Import undone',
      message: 'Restored ' + (summary.records || 0) + ' records from the snapshot.'
    };
    toast('SuiteCRM import undone');
    await reloadWorkspace();
    openSuiteCrmWizard();
  } catch (e) {
    suiteCrmStatus = {
      type: 'error',
      label: 'Undo failed',
      message: e.message
    };
    renderSuiteCrmWizard();
    toast('Undo failed');
  }
}

async function exportWorkspace() {
  try {
    var bundle = await api.get('/api/v1/export');
    var blob = new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' });
    var url = URL.createObjectURL(blob);
    var anchor = document.createElement('a');
    var stamp = new Date().toISOString().replace(/[:.]/g, '-');
    anchor.href = url;
    anchor.download = 'ironcrm-' + stamp + '.ironcrm.json';
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(url);
    toast('Export ready');
  } catch (e) {
    toast('Export failed: ' + e.message);
  }
}

function restoreWorkspace() {
  var input = document.createElement('input');
  input.type = 'file';
  input.accept = '.json,.ironcrm,application/json';
  input.addEventListener('change', function() {
    var file = input.files && input.files[0];
    if (!file) return;
    var reader = new FileReader();
    reader.onload = async function() {
      try {
        var bundle = JSON.parse(String(reader.result || '{}'));
        var summary = previewBundleSummary(bundle);
        if (!summary.valid) {
          toast(summary.message);
          return;
        }
        var ok = confirm('Restore this IronCRM bundle?\\n\\n' +
          summary.records + ' records\\n' +
          summary.links + ' links\\n' +
          summary.activities + ' activities\\n\\n' +
          'Existing records with the same IDs will be updated.');
        if (!ok) return;
        var restoreSummary = await api.post('/api/v1/restore', bundle);
        toast('Restored ' + (restoreSummary.records || 0) + ' records');
        await reloadWorkspace();
      } catch (e) {
        toast('Restore failed: ' + e.message);
      }
    };
    reader.readAsText(file);
  });
  input.click();
}

function previewBundleSummary(bundle) {
  if (!bundle || bundle.kind !== 'ironcrm.workspace' || bundle.version !== 1) {
    return { valid: false, message: 'Not an IronCRM workspace bundle' };
  }
  return {
    valid: true,
    records: Array.isArray(bundle.records) ? bundle.records.length : 0,
    links: Array.isArray(bundle.record_links) ? bundle.record_links.length : 0,
    activities: Array.isArray(bundle.activities) ? bundle.activities.length : 0
  };
}

function editCurrentRecord() {
  if (!currentDetailModule || !currentDetailRecord) return;
  pendingDeleteKey = null;
  renderInlineEditor(currentDetailModule, currentDetailRecord, false);
}

async function deleteCurrentRecord() {
  if (!currentDetailModule || !currentDetailRecord || !currentDetailRecord.id) {
    toast('Select a saved record before deleting');
    return;
  }
  pendingDeleteKey = makeDetailKey(currentDetailModule, currentDetailRecord);
  renderDetail(currentDetailModule, currentDetailRecord, currentDetailRelationships, currentDetailActivities);
}

function cancelDeleteRecord() {
  pendingDeleteKey = null;
  if (currentDetailModule && currentDetailRecord) {
    renderDetail(currentDetailModule, currentDetailRecord, currentDetailRelationships, currentDetailActivities);
  }
}

async function confirmDeleteCurrentRecord() {
  if (!currentDetailModule || !currentDetailRecord || !currentDetailRecord.id) return;
  if (pendingDeleteKey !== makeDetailKey(currentDetailModule, currentDetailRecord)) {
    deleteCurrentRecord();
    return;
  }
  var module = currentDetailModule;
  var id = currentDetailRecord.id;
  try {
    await api.del('/api/v1/' + module + '/' + id);
    toast('Deleted');
    pendingDeleteKey = null;
    closeDetailPane();
    if (currentModule === module) {
      await refreshCurrentModule();
    } else if (currentModule) {
      await selectModule(currentModule);
    } else {
      await refreshDashboardPreviewModule(module);
      setMainState('dashboard');
    }
  } catch (e) {
    toast('Error: ' + e.message);
  }
}

function removeDraftRow() {
  if (!draftRecordId) return;
  currentRows = currentRows.filter(function(row) { return row.id !== draftRecordId; });
  draftRecordId = null;
}

function makeDraftRecord(module) {
  var draft = { id: 'draft-' + Date.now(), __draft: true };
  if (module === 'contacts' || module === 'leads') {
    draft.first_name = '';
    draft.last_name = '';
    draft.email1 = '';
  } else {
    draft.name = '';
  }
  return draft;
}

function openNewRecordInline(module) {
  removeDraftRow();
  var draft = makeDraftRecord(module);
  draftRecordId = draft.id;
  currentRows = [draft].concat(currentRows);
  selectedRecordId = draft.id;
  renderInlineEditor(module, draft, true);
  applyCurrentView();
}

function editDraftRecord() {
  var draft = currentRows.filter(function(row) { return row.id === draftRecordId; })[0];
  if (draft && currentModule) renderInlineEditor(currentModule, draft, true);
}

function renderInlineEditor(module, record, isNew) {
  var meta = getMeta(module);
  currentDetailModule = module;
  currentDetailRecord = record;
  currentDetailMode = isNew ? 'new' : 'edit';
  selectedRecordId = record.id || null;
  detailOpen = true;
  document.getElementById('detail-pane').classList.remove('hidden');
  document.getElementById('detail-empty').classList.add('hidden');
  document.getElementById('detail-content').classList.remove('hidden');
  syncShellState();

  document.getElementById('detail-header').innerHTML =
    '<div class="detail-kicker-row"><div class="detail-kicker">' + escapeHtml(meta.title) + '</div><div class="detail-header-actions"><button type="button" class="detail-btn" data-action="cancel-inline">Cancel</button><button type="button" class="detail-btn primary-detail-btn" data-action="save-inline">Save</button></div></div>' +
    '<div class="detail-title-row"><h2>' + escapeHtml((isNew ? 'New ' : 'Edit ') + meta.title) + '</h2></div>' +
    '<div class="detail-subtitle">' + (isNew ? 'Draft row added to this module.' : 'Editing inline in the record sidebar.') + '</div>';

  document.getElementById('detail-sections').innerHTML =
    '<section class="detail-section"><div class="section-head-row"><div class="section-head">Fields</div></div><form id="detail-form" class="inline-form">' + renderFormFields(module, isNew ? null : record) + '</form></section>';
}

function collectInlineFields() {
  var fields = {};
  var inputs = document.querySelectorAll("#detail-form input, #detail-form textarea, #detail-form select");
  for (var i = 0; i < inputs.length; i += 1) {
    if (inputs[i].value) fields[inputs[i].name] = inputs[i].value;
  }
  return fields;
}

async function refreshCurrentModule() {
  if (!currentModule) return;
  var result = await api.get('/api/v1/' + currentModule + '?limit=50');
  currentRows = result.data || [];
  dashboardPreviewRows[currentModule] = currentRows.slice(0, 3);
  moduleCounts[currentModule] = result.total || currentRows.length;
  renderSidebar();
  renderToolbar(currentModule);
  applyCurrentView();
}

async function refreshDashboardPreviewModule(module) {
  if (!module) return;
  try {
    var result = await api.get('/api/v1/' + module + '?limit=3');
    dashboardPreviewRows[module] = result.data || [];
    moduleCounts[module] = result.total || 0;
    renderSidebar();
    renderDashboard();
  } catch (e) {
    log('warn', 'dashboard preview refresh failed ' + module + ' ' + e);
  }
}

async function reloadWorkspace() {
  var data = await api.get('/api/v1/modules');
  modules = data.modules.filter(function(module) {
    return !module.startsWith('_') && !module.includes('_contacts') && !module.includes('_opportunities') && !module.includes('_cases');
  });
  dashboardPreviewRows = {};
  moduleDefs = {};
  if (data.definitions) {
    data.definitions.forEach(function(def) {
      moduleDefs[def.module] = def;
    });
  }
  for (var i = 0; i < modules.length; i += 1) {
    try {
      var result = await api.get('/api/v1/' + modules[i] + '?limit=3');
      dashboardPreviewRows[modules[i]] = result.data || [];
      moduleCounts[modules[i]] = result.total || 0;
    } catch (e) {
      dashboardPreviewRows[modules[i]] = dashboardPreviewRows[modules[i]] || [];
      moduleCounts[modules[i]] = moduleCounts[modules[i]] || 0;
    }
  }
  renderSidebar();
  renderOverview();
  if (currentModule && modules.indexOf(currentModule) >= 0) {
    await refreshCurrentModule();
  } else {
    currentModule = null;
    renderToolbar(null);
    renderDashboard();
    setMainState('dashboard');
  }
}

async function saveInlineEdit() {
  if (!currentDetailModule || !currentDetailRecord) return;
  var fields = collectInlineFields();
  try {
    if (currentDetailMode === 'edit') {
      await api.put("/api/v1/" + currentDetailModule + "/" + currentDetailRecord.id, fields);
      toast("Updated");
      var editedModule = currentDetailModule;
      var editedId = currentDetailRecord.id;
      if (currentModule) {
        await refreshCurrentModule();
      } else {
        await refreshDashboardPreviewModule(editedModule);
      }
      await openDetail(editedModule, editedId);
    } else {
      var result = await api.post("/api/v1/" + currentDetailModule, fields);
      toast("Created");
      removeDraftRow();
      await refreshCurrentModule();
      await openDetail(currentDetailModule, result.id);
    }
  } catch (e) {
    toast("Error: " + e.message);
  }
}

function cancelInlineEdit() {
  if (currentDetailMode === 'new') {
    removeDraftRow();
    closeDetailPane();
    applyCurrentView();
    return;
  }
  if (currentDetailModule && currentDetailRecord && currentDetailRecord.id) {
    openDetail(currentDetailModule, currentDetailRecord.id);
  }
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
        var result = await api.get('/api/v1/' + modules[i] + '?limit=3');
        dashboardPreviewRows[modules[i]] = result.data || [];
        moduleCounts[modules[i]] = result.total || 0;
      } catch (e) {
        dashboardPreviewRows[modules[i]] = dashboardPreviewRows[modules[i]] || [];
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

window.selectModule = selectModule;
window.changeView = changeView;
window.openDetail = openDetail;
window.scrollToBoardStage = scrollToBoardStage;
window.toggleStageSection = toggleStageSection;
window.openOverviewItem = openOverviewItem;
window.advanceRecord = advanceRecord;
window.editCurrentRecord = editCurrentRecord;
window.deleteCurrentRecord = deleteCurrentRecord;
window.cancelDeleteRecord = cancelDeleteRecord;
window.confirmDeleteCurrentRecord = confirmDeleteCurrentRecord;
window.openNewRecordInline = openNewRecordInline;
window.editDraftRecord = editDraftRecord;
window.saveInlineEdit = saveInlineEdit;
window.cancelInlineEdit = cancelInlineEdit;
window.saveRecordNote = saveRecordNote;
window.selectSuiteCrmMode = selectSuiteCrmMode;
window.detectSuiteCrm = detectSuiteCrm;
window.connectSuiteCrm = connectSuiteCrm;


init();
