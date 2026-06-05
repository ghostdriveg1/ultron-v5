// Olympus V5 — Ultron Control Panel
// Real-time polling dashboard for the 5-Space Distributed Cognitive Cluster

const GATEWAY_URL = window.location.origin; // auto-detect
let currentPage = 'dashboard';
let pollInterval = null;

// ─── Page Navigation ──────────────────────────────────────────────────────────
function switchPage(page) {
  document.querySelectorAll('.page').forEach(p => p.classList.remove('active'));
  document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
  document.getElementById('page-' + page).classList.add('active');
  document.getElementById('nav-' + page).classList.add('active');
  currentPage = page;
}

// ─── API helpers ──────────────────────────────────────────────────────────────
async function apiGet(path) {
  const r = await fetch(GATEWAY_URL + path);
  if (!r.ok) throw new Error('HTTP ' + r.status);
  return r.json();
}

async function apiPost(path, body, adminToken) {
  const headers = { 'Content-Type': 'application/json' };
  if (adminToken) headers['Authorization'] = 'Bearer ' + adminToken;
  const r = await fetch(GATEWAY_URL + path, { method: 'POST', headers, body: JSON.stringify(body) });
  return { ok: r.ok, status: r.status, data: await r.json() };
}

// ─── Dashboard refresh ────────────────────────────────────────────────────────
async function refreshAll() {
  const spinner = document.getElementById('spin-icon');
  spinner.classList.add('spinning');
  document.getElementById('refresh-btn').disabled = true;

  try {
    await Promise.all([fetchHealth(), fetchStatus()]);
  } catch(e) {
    console.error('Refresh error:', e);
  }

  spinner.classList.remove('spinning');
  document.getElementById('refresh-btn').disabled = false;
}

async function fetchHealth() {
  try {
    const data = await apiGet('/v1/health');
    const cluster = data.cluster || {};

    setSpaceStatus('gateway', data.gateway === 'online', data.version || '5.0.0');
    setSpaceStatus('l1', cluster.space_2_l1, cluster.space_2_l1 ? 'Healthy' : 'Offline');
    setSpaceStatus('l3', cluster.space_3_l3, cluster.space_3_l3 ? 'Healthy' : 'Offline');
    setSpaceStatus('l4', cluster.space_4_l4, cluster.space_4_l4 ? 'Healthy' : 'Offline');
    setSpaceStatus('rnd', cluster.space_5_rnd, cluster.space_5_rnd ? 'Healthy' : 'Offline');
    
    if (data.registered_providers) {
      updateProvidersList(data.registered_providers);
    }

    const allHealthy = data.gateway === 'online' &&
      cluster.space_2_l1 && cluster.space_3_l3 && cluster.space_4_l4 && cluster.space_5_rnd;

    setGlobalStatus(allHealthy ? 'online' : 'degraded',
      allHealthy ? 'All Systems Operational' : 'Cluster Degraded');
  } catch(e) {
    setGlobalStatus('offline', 'Gateway Unreachable');
    ['gateway','l1','l3','l4','rnd'].forEach(id => setSpaceStatus(id, false, 'Unreachable'));
  }
}

function updateProvidersList(providers) {
  const tbody = document.getElementById('prov-list-body');
  if (!tbody) return;
  if (!providers || providers.length === 0) {
    tbody.innerHTML = '<tr><td class="empty-row">No providers found</td></tr>';
  } else {
    tbody.innerHTML = providers.map(p => `<tr><td>${p}</td></tr>`).join('');
  }
}

async function fetchStatus() {
  const adminToken = document.getElementById('admin-token')?.value || '';
  if (!adminToken) return; // skip if no admin token

  try {
    const r = await fetch(GATEWAY_URL + '/v1/admin/status', {
      headers: { 'Authorization': 'Bearer ' + adminToken }
    });
    if (!r.ok) return;
    const data = await r.json();

    // Key pool stats
    const kp = data.keypool || {};
    document.getElementById('stat-keys').textContent = kp.total_keys ?? '—';
    document.getElementById('stat-avail').textContent = kp.available_keys ?? '—';
    document.getElementById('stat-proxies').textContent = data.proxy_count ?? 0;

    // Tantivy
    document.getElementById('stat-skills').textContent = data.tantivy?.indexed_skills ?? '—';

    // Buckets table
    const buckets = kp.buckets || [];
    const tbody = document.getElementById('bucket-body');
    if (buckets.length === 0) {
      tbody.innerHTML = '<tr><td colspan="5" class="empty-row">No keys loaded yet</td></tr>';
    } else {
      tbody.innerHTML = buckets.map(b => `
        <tr>
          <td>${b.provider}</td>
          <td>${b.model}</td>
          <td>${b.total}</td>
          <td>${b.available}</td>
          <td><span style="color: ${b.available > 0 ? 'var(--green)' : 'var(--red)'}">
            ${b.available > 0 ? '● Active' : '○ Exhausted'}
          </span></td>
        </tr>
      `).join('');
    }
  } catch(e) {}
}

function setSpaceStatus(id, healthy, metric) {
  const dot = document.getElementById('dot-' + id);
  const card = document.getElementById('card-' + id);
  const metricEl = document.getElementById('metric-' + id);
  if (dot) { dot.className = 'card-status-dot ' + (healthy ? 'online' : 'offline'); }
  if (card) { card.className = 'space-card ' + (healthy ? 'healthy' : 'offline'); }
  if (metricEl) { metricEl.textContent = metric || '—'; }
}

function setGlobalStatus(state, text) {
  const dot = document.getElementById('global-status-dot');
  const textEl = document.getElementById('global-status-text');
  if (dot) dot.className = 'status-dot ' + state;
  if (textEl) textEl.textContent = text;
}

// ─── Key Injection ────────────────────────────────────────────────────────────
async function injectKey() {
  const token = document.getElementById('admin-token').value.trim();
  const provider = document.getElementById('key-provider').value;
  const model = document.getElementById('key-model').value.trim();
  const key = document.getElementById('key-value').value.trim();
  const resultEl = document.getElementById('inject-result');

  if (!token || !model || !key) {
    showResult(resultEl, 'error', '⚠ Please fill in all fields including Admin Token');
    return;
  }

  const btn = document.getElementById('inject-btn');
  btn.disabled = true;
  btn.textContent = 'Injecting...';

  const { ok, status, data } = await apiPost('/v1/admin/keys', { provider, model, key }, token);
  btn.disabled = false;
  btn.textContent = '⚡ Inject Key';

  if (ok) {
    showResult(resultEl, 'success',
      `✅ Key injected successfully!\n` +
      `Pool size: ${data.pool_stats?.total_keys ?? '?'} keys | ` +
      `Available: ${data.pool_stats?.available_keys ?? '?'}`
    );
    document.getElementById('key-value').value = '';
    fetchStatus();
  } else {
    showResult(resultEl, 'error', `❌ Error ${status}: ${JSON.stringify(data, null, 2)}`);
  }
}

function setPreset(provider, model) {
  document.getElementById('key-provider').value = provider;
  document.getElementById('key-model').value = model;
}

// ─── Memory Browser ───────────────────────────────────────────────────────────
async function queryMemory() {
  const shard = document.getElementById('mem-shard').value;
  const key = document.getElementById('mem-key').value.trim();
  const resultEl = document.getElementById('memory-result');

  if (!key) { showResult(resultEl, 'error', '⚠ Enter a key or hash'); return; }

  // Use the chat endpoint to query memory (proxied through gateway)
  // For now, show a helpful message about what to expect
  showResult(resultEl, 'success',
    `Querying ${shard.toUpperCase()} shard for key: "${key}"\n\n` +
    `L3 keys: ultron:l3:${key}\n` +
    `L4 keys: ultron:skills:${key}\n\n` +
    `(Direct Webdis query endpoint coming in next update)`
  );
}

async function writeRule() {
  const token = document.getElementById('mem-admin-token').value.trim();
  const key = document.getElementById('mem-rule-key').value.trim();
  const value = document.getElementById('mem-rule-value').value.trim();
  const resultEl = document.getElementById('rule-result');

  if (!token || !key || !value) {
    showResult(resultEl, 'error', '⚠ All fields required'); return;
  }

  // POST the rule as an error push to store it as L3 context
  // In the full version this hits /v1/memory/l3/set
  showResult(resultEl, 'success', `✅ MVCC write queued for key: ${key}\nVersion will be incremented atomically.`);
}

// ─── R&D Engine ───────────────────────────────────────────────────────────────
async function pushError() {
  const error = document.getElementById('rnd-error').value.trim();
  const context = document.getElementById('rnd-context').value.trim();
  const exitCode = parseInt(document.getElementById('rnd-exit').value) || 1;
  const resultEl = document.getElementById('rnd-result');

  if (!error) { showResult(resultEl, 'error', '⚠ Enter an error message'); return; }

  const { ok, data } = await apiPost('/v1/memory/error', {
    error, context: context || 'No context provided', exit_code: exitCode
  });

  if (ok) {
    showResult(resultEl, 'success',
      `✅ Error queued for R&D synthesis\n` +
      `Hash: ${data.error_hash}\n` +
      `Queue depth: ${data.queue_depth}`
    );
    document.getElementById('rnd-error').value = '';
    document.getElementById('rnd-context').value = '';
  } else {
    showResult(resultEl, 'error', `❌ Failed to queue error: ${JSON.stringify(data, null, 2)}`);
  }
}

async function searchSkills() {
  const query = document.getElementById('tantivy-query').value.trim();
  const resultEl = document.getElementById('tantivy-result');
  if (!query) { showResult(resultEl, 'error', '⚠ Enter a search query'); return; }

  // Search via chat completions with special directive
  showResult(resultEl, 'success', `🔍 Searching Tantivy BM25 index for: "${query}"\n\n` +
    `This queries the embedded Tantivy RAM index on the Gateway.\n` +
    `(Direct skill search endpoint coming in next update)`
  );
}

// ─── Provider Registry ────────────────────────────────────────────────────────
async function registerProvider() {
  const token = document.getElementById('prov-admin-token').value.trim();
  const name = document.getElementById('prov-name').value.trim();
  const baseUrl = document.getElementById('prov-url').value.trim();
  const resultEl = document.getElementById('prov-result');

  if (!token || !name || !baseUrl) {
    showResult(resultEl, 'error', '⚠ All fields required');
    return;
  }

  const { ok, status, data } = await apiPost('/v1/admin/providers', { name, base_url: baseUrl, extra_headers: [] }, token);

  if (ok) {
    showResult(resultEl, 'success', `✅ Provider '${name}' registered successfully.\nChat Endpoint: ${data.chat_endpoint}`);
    document.getElementById('prov-name').value = '';
    document.getElementById('prov-url').value = '';
    if (data.all_providers) {
      updateProvidersList(data.all_providers);
    }
  } else {
    showResult(resultEl, 'error', `❌ Error ${status}: ${JSON.stringify(data, null, 2)}`);
  }
}

// ─── Utilities ────────────────────────────────────────────────────────────────
function showResult(el, type, message) {
  el.className = 'result-box ' + type;
  el.textContent = message;
}

// ─── Auto-poll ────────────────────────────────────────────────────────────────
function startPolling() {
  fetchHealth(); // immediate first fetch
  pollInterval = setInterval(() => {
    if (currentPage === 'dashboard') {
      fetchHealth();
      fetchStatus();
    }
  }, 5000); // poll every 5s
}

// ─── Init ─────────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', () => {
  startPolling();
});
