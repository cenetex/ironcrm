var s = document.getElementById('db-status');
function show(msg, color) { s.innerHTML = msg; if (color) s.style.color = color; }

var isTauri = !!(window.__TAURI_INTERNALS__);
var invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;

var api = {
  get: async function(path) {
    if (isTauri && invoke) {
      if (path === '/api/v1/modules') return invoke('get_modules');
      var q = path.includes('?') ? path.split('?')[1] : '';
      var clean = path.split('?')[0];
      var parts = clean.split('/');
      // parts = ['', 'api', 'v1', 'accounts'] for module list
      // parts = ['', 'api', 'v1', 'accounts', 'id'] for record detail
      if (parts.length === 4 && parts[1] === 'api' && parts[2] === 'v1') {
        var p = new URLSearchParams(q);
        return invoke('get_module_records', { module: parts[3], limit: parseInt(p.get('limit'))||25, offset: parseInt(p.get('offset'))||0 });
      }
      if (parts.length === 5 && parts[1] === 'api' && parts[2] === 'v1') {
        return invoke('get_module_record', { module: parts[3], id: parts[4] });
      }
      throw new Error('bad path: '+path);
    }
    var res = await fetch(path);
    return res.json();
  }
};

var modules = [];
var currentModule = null;

async function init() {
  try {
    show('● loading...', 'var(--warning)');
    var data = await api.get('/api/v1/modules');
    modules = data.modules.filter(function(m) { return !m.startsWith('_') && !m.includes('_contacts') && !m.includes('_opportunities') && !m.includes('_cases'); });
    renderSidebar();
    show('\u25cf ' + modules.length + ' modules', 'var(--success)');
  } catch(e) { show('\u25cf ' + e, 'var(--danger)'); }
}

function renderSidebar() {
  var icons = { accounts: '\ud83c\udfe2', contacts: '\ud83d\udc64', opportunities: '\ud83d\udcbc', leads: '\ud83c\udfaf', cases: '\ud83c\udfab' };
  document.getElementById('module-list').innerHTML = modules.map(function(m) {
    return '<div class="module-item" data-module="'+m+'" onclick="selectModule(\''+m+'\')"><span class="icon">'+(icons[m]||'\ud83d\udccb')+'</span><span>'+m+'</span></div>';
  }).join('');
}

async function selectModule(mod) {
  currentModule = mod;
  document.querySelectorAll('.module-item').forEach(function(el){ el.classList.remove('active'); });
  var el = document.querySelector('[data-module="'+mod+'"]'); if (el) el.classList.add('active');
  document.getElementById('view-title').textContent = mod.replace(/_/g,' ').replace(/\b\w/g,function(c){return c.toUpperCase();});
  document.getElementById('empty-state').classList.add('hidden');
  document.getElementById('loading').classList.remove('hidden');
  try {
    var data = await api.get('/api/v1/'+mod+'?limit=50');
    renderTable(data.data);
  } catch(e) { document.getElementById('loading').textContent = 'Error: '+e; }
}

function renderTable(rows) {
  document.getElementById('loading').classList.add('hidden');
  if (!rows || !rows.length) { document.getElementById('empty-state').classList.remove('hidden'); document.getElementById('data-table').classList.add('hidden'); return; }
  var skip = ['id','date_entered','date_modified','modified_user_id','created_by','assigned_user_id','deleted','description','id_c'];
  var cols = Object.keys(rows[0]).filter(function(k){ return skip.indexOf(k)===-1 && rows[0][k]!==null; }).slice(0,5);
  document.getElementById('table-head').innerHTML = '<tr>'+cols.map(function(c){return'<th>'+c.replace(/_/g,' ')+'</th>';}).join('')+'</tr>';
  document.getElementById('table-body').innerHTML = rows.map(function(r){
    return '<tr onclick="openDetail(\''+currentModule+'\',\''+r.id+'\')">'+cols.map(function(c){return'<td>'+(r[c]||'')+'</td>';}).join('')+'</tr>';
  }).join('');
  document.getElementById('data-table').classList.remove('hidden');
  document.getElementById('empty-state').classList.add('hidden');
}

async function openDetail(mod, id) {
  try { var d=await api.get('/api/v1/'+mod+'/'+id); showDetail(mod,d.data,d.relationships); } catch(e){}
}
function showDetail(mod, rec, rels) {
  var ov=document.getElementById('detail-overlay'); if(ov) ov.remove();
  ov=document.createElement('div'); ov.id='detail-overlay'; ov.onclick=function(e){if(e.target===ov)ov.remove();};
  var name = rec.first_name ? rec.first_name+' '+(rec.last_name||'') : (rec.name||'Record');
  var fields = Object.entries(rec).filter(function(e){return['id','date_entered','date_modified','modified_user_id','created_by','deleted','id_c'].indexOf(e[0])===-1&&e[1]!==null;}).slice(0,10).map(function(e){return'<div class="detail-field"><span class="label">'+e[0].replace(/_/g,' ')+'</span><span class="value">'+(e[1]||'')+'</span></div>';}).join('');
  var rhtml=''; if(rels&&rels.length) rhtml='<div class="related-section"><h4>Related ('+rels.length+')</h4>'+rels.map(function(r){var n=r.first_name?r.first_name+' '+(r.last_name||''):(r.name||'Related');var m=r.first_name?'contacts':'accounts';return'<div class="related-item" onclick="openDetail(\''+m+'\',\''+r.id+'\')">'+n+'</div>';}).join('')+'</div>';
  ov.innerHTML='<div id="detail-panel"><button class="close-btn" onclick="this.closest(\'#detail-overlay\').remove()">\u2715</button><h3>'+name+'</h3><div class="detail-fields">'+fields+'</div>'+rhtml+'</div>';
  document.body.appendChild(ov);
}
function handleScan(f) { if(!f)return; document.getElementById('scan-overlay').classList.remove('hidden'); document.getElementById('scan-status').textContent='Scanner offline'; }
function discardScan() { document.getElementById('scan-overlay').classList.add('hidden'); }

init();
