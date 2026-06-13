// apiku tester - vanilla JS (no framework dependencies).
// Handles: tab switching, endpoint form, request sending, response rendering,
// visual preview, and copy-as-cURL.

const $ = id => document.getElementById(id);
const $$ = sel => document.querySelectorAll(sel);

let lastResponse = null;
let lastIdsBySource = {};

// ---- Tab switching --------------------------------------------------------
$$('.tab').forEach(btn => {
  btn.addEventListener('click', () => {
    const t = btn.dataset.tab;
    $$('.tab').forEach(b => b.classList.toggle('active', b.dataset.tab === t));
    $$('.tab-content').forEach(c => c.classList.toggle('active', c.id === 'tab-' + t));
  });
});
$$('.rtab').forEach(btn => {
  btn.addEventListener('click', () => {
    const t = btn.dataset.rtab;
    $$('.rtab').forEach(b => b.classList.toggle('active', b.dataset.rtab === t));
    $$('.rtab-content').forEach(c => c.classList.toggle('active', c.id === 'rtab-' + t));
  });
});

// Lang tabs (in code-examples view)
function bindLangTabs() {
  $$('.lang-tab').forEach(btn => {
    if (btn.dataset.bound) return;
    btn.dataset.bound = '1';
    btn.addEventListener('click', () => {
      const t = btn.dataset.lang;
      $$('.lang-tab').forEach(b => b.classList.toggle('active', b.dataset.lang === t));
      $$('.lang-content').forEach(c => c.classList.toggle('active', c.id === 'lang-' + t));
    });
  });
}
bindLangTabs();

// Copy code buttons
function bindCopyButtons() {
  $$('.copy-code-btn').forEach(btn => {
    if (btn.dataset.bound) return;
    btn.dataset.bound = '1';
    btn.addEventListener('click', () => {
      const code = btn.closest('.code-wrap').querySelector('code');
      const txt = code ? code.textContent : '';
      navigator.clipboard.writeText(txt).then(
        () => { btn.textContent = 'Copied'; setTimeout(() => btn.textContent = 'Copy', 1200); },
        () => { btn.textContent = 'Failed'; setTimeout(() => btn.textContent = 'Copy', 1200); }
      );
    });
  });
}
bindCopyButtons();

// ---- Endpoint selector ----------------------------------------------------
const endpointSelect = $('endpoint-select');
const paramFields = $('param-fields');

function renderParamFields() {
  const opt = endpointSelect.options[endpointSelect.selectedIndex];
  const path = opt.value;
  const params = (opt.dataset.params || '').split(',').filter(Boolean);

  const existing = {};
  $$('#param-fields input, #param-fields select').forEach(el => existing[el.name] = el.value);

  let html = '';
  if (path === '/api/v1/search') {
    html = `
      <div class="field">
        <label for="p_q">Query (q)</label>
        <input type="text" id="p_q" name="q" placeholder="e.g. one piece, or 'Genshin Impact [full color]' for nhentai" value="${esc(existing.q || 'one piece')}">
      </div>
      <div class="field">
        <label for="p_source">Source</label>
        <select id="p_source" name="source">
          <option value="all" ${(existing.source || 'all') === 'all' ? 'selected' : ''}>all (parallel)</option>
          <option value="manga" ${existing.source === 'manga' ? 'selected' : ''}>manga (Mangaball)</option>
          <option value="donghua" ${existing.source === 'donghua' ? 'selected' : ''}>donghua (Anichin)</option>
          <option value="cosplay" ${existing.source === 'cosplay' ? 'selected' : ''}>cosplay (Cosplaytele)</option>
          <option value="nhentai" ${existing.source === 'nhentai' ? 'selected' : ''}>nhentai (doujin)</option>
          <option value="novel" ${existing.source === 'novel' ? 'selected' : ''}>novel (NovelID)</option>
        </select>
      </div>
      <div class="field">
        <label for="p_page">Page</label>
        <input type="number" id="p_page" name="page" min="1" value="${esc(existing.page || '1')}">
      </div>
    `;
  } else if (path.startsWith('/api/v1/browse/')) {
    const provider = path.split('/').pop();
    html = `
      <div class="field">
        <label for="p_feed">Feed</label>
        <select id="p_feed" name="feed">
          ${feedOptionsFor(provider, existing.feed || '')}
        </select>
      </div>
      <div class="field">
        <label for="p_page">Page</label>
        <input type="number" id="p_page" name="page" min="1" value="${esc(existing.page || '1')}">
      </div>
      ${provider === 'mangaball' ? `<div class="field">
        <label for="p_size">Page size</label>
        <input type="number" id="p_size" name="size" min="5" max="60" value="${esc(existing.size || '30')}">
      </div>` : ''}
    `;
  } else if (params.includes('id')) {
    const placeholder = pickIdPlaceholder(path);
    const showPaging = params.includes('page');
    html = `
      <div class="field">
        <label for="p_id">ID (opaque)</label>
        <input type="text" id="p_id" name="id" placeholder="${esc(placeholder)}" value="${esc(existing.id || '')}">
        <p class="muted" style="font-size:11px;margin:4px 0 0">Run a search first to get a valid ID.</p>
      </div>
      ${showPaging ? `
        <div class="field">
          <label for="p_page">Chapter page</label>
          <input type="number" id="p_page" name="page" min="1" value="${esc(existing.page || '1')}">
        </div>
        <div class="field">
          <label for="p_size">Chapters per page</label>
          <input type="number" id="p_size" name="size" min="10" max="200" value="${esc(existing.size || '50')}">
        </div>
      ` : ''}
    `;
  }
  paramFields.innerHTML = html;
}

function feedOptionsFor(provider, current) {
  const sets = {
    mangaball: [
      ['home', 'Home (featured)'],
      ['popular', 'Popular'],
      ['latest', 'Latest update'],
      ['recommend', 'Recommended'],
    ],
    anichin: [
      ['home', 'Home (latest update)'],
      ['popular', 'Popular'],
      ['rating', 'Rating'],
      ['title', 'A-Z'],
      ['latest-added', 'Latest added'],
    ],
    cosplaytele: [
      ['home', 'Home (latest)'],
      ['popular', 'Popular / Hot'],
    ],
    nhentai: [
      ['home', 'Home (recent)'],
      ['popular-today', 'Popular today'],
      ['popular-week', 'Popular this week'],
      ['popular', 'Popular all-time'],
    ],
    novelid: [
      ['home', 'Home (semua)'],
      ['popular', 'Tamat (popular)'],
      ['novel-translate', 'Novel Translate'],
      ['fantasi', 'Fantasi'],
      ['romantis', 'Romantis'],
      ['religi', 'Religi'],
      ['motivasi', 'Motivasi'],
      ['horror', 'Horror'],
      ['aksi', 'Aksi'],
      ['komedi', 'Komedi'],
      ['sastra', 'Sastra'],
      ['novel-anak', 'Novel Anak'],
    ],
  };
  const opts = sets[provider] || [['home', 'Home']];
  return opts.map(([v, label]) =>
    `<option value="${v}" ${current === v ? 'selected' : ''}>${label}</option>`
  ).join('');
}
endpointSelect.addEventListener('change', () => { renderParamFields(); updateUrlPreview(); });

function pickIdPlaceholder(path) {
  if (path.includes('/manga/chapter/')) return lastIdsBySource['manga_chapter'] || 'mbiabc.aHR0cHM6Ly...';
  if (path.includes('/manga/')) return lastIdsBySource['manga_series'] || 'mbsabc.aHR0cHM6Ly...';
  if (path.includes('/donghua/episode/')) return lastIdsBySource['donghua_episode'] || 'aciabc.aHR0cHM6Ly...';
  if (path.includes('/donghua/')) return lastIdsBySource['donghua_series'] || 'acsabc.aHR0cHM6Ly...';
  if (path.includes('/cosplay/')) return lastIdsBySource['cosplay_post'] || 'ctpabc.aHR0cHM6Ly...';
  if (path.includes('/nhentai/chapter/')) return lastIdsBySource['nhentai_chapter'] || 'nhiabc.aHR0cHM6Ly...';
  if (path.includes('/nhentai/')) return lastIdsBySource['nhentai_series'] || 'nhsabc.aHR0cHM6Ly...';
  if (path.includes('/novel/chapter/')) return lastIdsBySource['novel_chapter'] || 'nviabc.aHR0cHM6Ly...';
  if (path.includes('/novel/')) return lastIdsBySource['novel_series'] || 'nvsabc.aHR0cHM6Ly...';
  return 'opaque-id-here';
}

// ---- URL preview ---------------------------------------------------------
function buildUrl() {
  const opt = endpointSelect.options[endpointSelect.selectedIndex];
  let path = opt.value;
  const params = (opt.dataset.params || '').split(',').filter(Boolean);

  if (params.includes('id')) {
    const id = ($('p_id') || {}).value || '';
    path = path.replace('{id}', encodeURIComponent(id));
    // ID-based endpoints with chapter pagination append page/size
    if (params.includes('page') || params.includes('size')) {
      const qs = new URLSearchParams();
      const page = ($('p_page') || {}).value || '';
      const size = ($('p_size') || {}).value || '';
      if (page && page !== '1') qs.set('page', page);
      if (size) qs.set('size', size);
      const s = qs.toString();
      if (s) path += '?' + s;
    }
  }

  if (path === '/api/v1/search') {
    const qs = new URLSearchParams();
    const q = ($('p_q') || {}).value || '';
    const src = ($('p_source') || {}).value || 'all';
    const page = ($('p_page') || {}).value || '1';
    if (q) qs.set('q', q);
    if (src && src !== 'all') qs.set('source', src);
    if (page && page !== '1') qs.set('page', page);
    const s = qs.toString();
    if (s) path += '?' + s;
  }
  if (path.startsWith('/api/v1/browse/')) {
    const qs = new URLSearchParams();
    const feed = ($('p_feed') || {}).value || '';
    const page = ($('p_page') || {}).value || '1';
    const size = ($('p_size') || {}).value || '';
    if (feed && feed !== 'home') qs.set('feed', feed);
    if (page && page !== '1') qs.set('page', page);
    if (size) qs.set('size', size);
    const s = qs.toString();
    if (s) path += '?' + s;
  }
  return path;
}

function updateUrlPreview() {
  const url = buildUrl();
  const el = $('last-url');
  el.textContent = url;
  el.classList.add('has-url');
}
paramFields.addEventListener('input', updateUrlPreview);
renderParamFields();
updateUrlPreview();

// ---- Send request --------------------------------------------------------
$('send-btn').addEventListener('click', send);
document.addEventListener('keydown', e => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') send();
});

async function send() {
  const url = buildUrl();
  setStatus('loading', 'Loading...');
  $('rendered-view').innerHTML = '<div class="placeholder">Loading...</div>';

  const start = performance.now();
  try {
    const resp = await fetch(url);
    const text = await resp.text();
    const elapsed = (performance.now() - start).toFixed(0);
    let parsed;
    try { parsed = JSON.parse(text); } catch { parsed = text; }
    lastResponse = parsed;

    setStatus(resp.ok ? 'ok' : 'err', `HTTP ${resp.status} - ${elapsed}ms`);
    renderJson(parsed);
    renderHeaders(resp);
    renderPreview(parsed);
    cacheIds(parsed);
  } catch (e) {
    setStatus('err', e.message);
    $('response-json').innerHTML = '// Error: ' + e.message;
  }
}

function setStatus(state, label) {
  $('response-meta').innerHTML = `<span class="status ${state}">${label}</span>`;
}

function renderJson(data) {
  const el = $('response-json');
  if (typeof data === 'string') {
    el.textContent = data;
  } else {
    el.innerHTML = syntaxHighlight(JSON.stringify(data, null, 2));
  }
}

function renderHeaders(resp) {
  const lines = [];
  resp.headers.forEach((v, k) => lines.push(`${k}: ${v}`));
  $('response-headers').textContent = lines.join('\n');
}

function syntaxHighlight(json) {
  return esc(json).replace(/("(\\u[a-zA-Z0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(true|false|null)\b|-?\d+(?:\.\d*)?(?:[eE][+\-]?\d+)?)/g, m => {
    let cls = 'number';
    if (/^"/.test(m)) cls = /:$/.test(m) ? 'key' : 'string';
    else if (/true|false/.test(m)) cls = 'boolean';
    else if (/null/.test(m)) cls = 'null';
    return '<span class="' + cls + '">' + m + '</span>';
  });
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, m => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[m]));
}

// ---- Visual preview ------------------------------------------------------
function renderPreview(data) {
  const view = $('rendered-view');
  if (!data || typeof data !== 'object') {
    view.innerHTML = '<div class="placeholder">No structured data to render</div>';
    return;
  }
  if (data.ok === false) {
    const code = (data.error || {}).code || 'error';
    const msg = (data.error || {}).message || 'unknown error';
    view.innerHTML = `<div class="placeholder" style="color:var(--red)">${esc(code)}: ${esc(msg)}</div>`;
    return;
  }

  const payload = data.data || data;

  if (payload && Array.isArray(payload.items)) return renderSearchResults(payload, view);
  if (payload && typeof payload.body === 'string' && payload.chapter_number !== undefined) return renderNovelChapter(payload, view);
  if (payload && Array.isArray(payload.chapters) && (payload.synopsis !== undefined || payload.rating !== undefined) && payload.description === undefined) return renderNovelSeries(payload, view);
  if (payload && Array.isArray(payload.chapters)) return renderMangaSeries(payload, view);
  if (payload && Array.isArray(payload.pages)) return renderMangaChapter(payload, view);
  if (payload && Array.isArray(payload.episodes)) return renderDonghuaSeries(payload, view);
  if (payload && Array.isArray(payload.servers)) return renderDonghuaEpisode(payload, view);
  if (payload && Array.isArray(payload.images)) return renderCosplay(payload, view);
  if (payload && payload.providers) return renderInfo(payload, view);
  if (payload && payload.status) {
    view.innerHTML = `<div class="placeholder" style="color:var(--green)">${esc(payload.status)}</div>`;
    return;
  }
  view.innerHTML = '<div class="placeholder">Response rendered as JSON only (unrecognised shape)</div>';
}

function renderSearchResults(p, view) {
  if (!p.items.length) {
    view.innerHTML = `<div class="placeholder">No results for "${esc(p.query)}"</div>`;
    return;
  }
  const sourceCounts = {};
  p.items.forEach(it => sourceCounts[it.source] = (sourceCounts[it.source] || 0) + 1);
  const summary = Object.entries(sourceCounts).map(([s, n]) => `<span class="pill">${s}: ${n}</span>`).join('');
  view.innerHTML = `
    <div class="preview-card">
      <h3>Search Results</h3>
      <div class="preview-meta">
        <span class="pill"><strong>${p.total}</strong> total</span>
        ${summary}
      </div>
      <div class="search-results-grid">
        ${p.items.slice(0, 30).map(item => `
          <div class="result-card" onclick="useId('${item.id}', '${item.kind}')">
            ${item.thumbnail ? `<img src="${item.thumbnail}" loading="lazy" referrerpolicy="no-referrer" onerror="this.style.opacity=0.2;this.alt='no image'">` : '<div style="aspect-ratio:3/4;background:var(--bg)"></div>'}
            <div class="body">
              <span class="source-tag ${item.source}">${item.source}</span>
              <div class="title">${esc(item.title)}</div>
              <code>${esc(item.id.slice(0, 30))}...</code>
            </div>
          </div>
        `).join('')}
      </div>
      ${p.items.length > 30 ? `<p class="muted" style="font-size:12px;margin-top:8px">+ ${p.items.length - 30} more</p>` : ''}
    </div>
  `;
}

function useId(id, kind) {
  const target = {
    manga: '/api/v1/manga/{id}',
    donghua: '/api/v1/donghua/{id}',
    cosplay: '/api/v1/cosplay/{id}',
    doujin: '/api/v1/nhentai/{id}',
    novel: '/api/v1/novel/{id}',
  }[kind] || '/api/v1/manga/{id}';
  endpointSelect.value = target;
  renderParamFields();
  setTimeout(() => {
    const idInput = $('p_id');
    if (idInput) idInput.value = id;
    updateUrlPreview();
    send();
  }, 50);
}

function renderMangaSeries(p, view) {
  const totalPages = p.chapter_total_pages || 1;
  const curPage = p.chapter_page || 1;
  view.innerHTML = `
    <div class="preview-card">
      <h3>${esc(p.title)}</h3>
      ${p.cover ? `<img class="preview-cover" src="${p.cover}" referrerpolicy="no-referrer" onerror="this.style.display='none'">` : ''}
      <div class="preview-meta">
        ${p.author ? `<span class="pill">Author: ${esc(p.author)}</span>` : ''}
        ${p.artist ? `<span class="pill">Artist: ${esc(p.artist)}</span>` : ''}
        <span class="pill"><strong>${p.chapter_count}</strong> chapters</span>
      </div>
      ${p.genres && p.genres.length ? `<div class="preview-meta">${p.genres.map(g => `<span class="pill">${esc(g)}</span>`).join('')}</div>` : ''}
      ${p.description ? `<p style="clear:both;font-size:13px;color:var(--muted)">${esc(p.description.slice(0, 300))}...</p>` : ''}
      <div style="clear:both"></div>
      ${chapterPager(curPage, totalPages, '/api/v1/manga/{id}', p.id)}
      <ul class="preview-list">
        ${p.chapters.map(c => `
          <li>
            <div>
              <span class="num">Ch. ${c.number}</span>
              <span>${esc(c.title || '')}</span>
              ${c.translations && c.translations.length > 1 ? `<span class="pill" style="margin-left:8px">${c.translations.length} langs</span>` : ''}
            </div>
            <button class="mini" onclick="loadById('/api/v1/manga/chapter/{id}', '${c.id}')">Open</button>
          </li>
        `).join('')}
      </ul>
      ${chapterPager(curPage, totalPages, '/api/v1/manga/{id}', p.id)}
    </div>
  `;
}

function chapterPager(curPage, totalPages, endpoint, seriesId) {
  if (totalPages <= 1) return '';
  const prev = curPage > 1 ? curPage - 1 : null;
  const next = curPage < totalPages ? curPage + 1 : null;
  return `<div style="display:flex;justify-content:center;gap:8px;margin:12px 0;align-items:center;font-size:13px">
    ${prev ? `<button class="mini" onclick="loadByIdPaged('${endpoint}', '${seriesId}', ${prev})">&laquo; Prev</button>` : ''}
    <span class="muted">Page ${curPage} / ${totalPages}</span>
    ${next ? `<button class="mini" onclick="loadByIdPaged('${endpoint}', '${seriesId}', ${next})">Next &raquo;</button>` : ''}
  </div>`;
}

function loadById(endpoint, id) {
  endpointSelect.value = endpoint;
  renderParamFields();
  setTimeout(() => {
    if ($('p_id')) $('p_id').value = id;
    updateUrlPreview();
    send();
  }, 50);
}

function loadByIdPaged(endpoint, id, page) {
  endpointSelect.value = endpoint;
  renderParamFields();
  setTimeout(() => {
    if ($('p_id')) $('p_id').value = id;
    if ($('p_page')) $('p_page').value = page;
    updateUrlPreview();
    send();
  }, 50);
}

function renderMangaChapter(p, view) {
  view.innerHTML = `
    <div class="preview-card">
      <h3>${esc(p.series_title || 'Manga')} - Chapter ${p.chapter_number}</h3>
      <div class="preview-meta"><span class="pill"><strong>${p.page_count}</strong> pages</span></div>
      <div class="preview-grid" style="grid-template-columns:1fr">
        ${p.pages.map(pg => `<img src="${pg.url}" loading="lazy" referrerpolicy="no-referrer" style="height:auto;width:100%;max-width:600px;margin:0 auto;display:block;border:1px solid var(--border)" onerror="this.style.opacity=0.3;this.alt='page '+${pg.index}+' failed'">`).join('')}
      </div>
    </div>
  `;
}

function renderDonghuaSeries(p, view) {
  const totalPages = p.episode_total_pages || 1;
  const curPage = p.episode_page || 1;
  view.innerHTML = `
    <div class="preview-card">
      <h3>${esc(p.title)}</h3>
      ${p.cover ? `<img class="preview-cover" src="${p.cover}" referrerpolicy="no-referrer" onerror="this.style.display='none'">` : ''}
      <div class="preview-meta">
        ${p.status ? `<span class="pill">${esc(p.status)}</span>` : ''}
        <span class="pill"><strong>${p.episode_count}</strong> episodes</span>
      </div>
      ${p.genres && p.genres.length ? `<div class="preview-meta">${p.genres.map(g => `<span class="pill">${esc(g)}</span>`).join('')}</div>` : ''}
      ${p.synopsis ? `<p style="clear:both;font-size:13px;color:var(--muted)">${esc(p.synopsis.slice(0, 400))}...</p>` : ''}
      <div style="clear:both"></div>
      ${chapterPager(curPage, totalPages, '/api/v1/donghua/{id}', p.id)}
      <ul class="preview-list">
        ${[...p.episodes].sort((a,b) => b.number - a.number).map(e => `
          <li>
            <div>
              <span class="num">Ep. ${e.number}</span>
              <span>${esc(e.title || '')}</span>
            </div>
            <button class="mini" onclick="loadById('/api/v1/donghua/episode/{id}', '${e.id}')">Watch</button>
          </li>
        `).join('')}
      </ul>
      ${chapterPager(curPage, totalPages, '/api/v1/donghua/{id}', p.id)}
    </div>
  `;
}

function renderDonghuaEpisode(p, view) {
  const first = p.servers[0];
  view.innerHTML = `
    <div class="preview-card">
      <h3>${esc(p.series_title || 'Donghua')} - Episode ${p.episode_number}</h3>
      ${first ? `<div class="video-frame"><iframe id="ep-iframe" src="${first.embed_url}" allowfullscreen allow="accelerometer; autoplay; encrypted-media; gyroscope; picture-in-picture"></iframe></div>` : ''}
      <div class="server-list">
        ${p.servers.map((s, i) => `<button class="server-btn ${i === 0 ? 'active' : ''}" data-src="${s.embed_url}" onclick="document.getElementById('ep-iframe').src=this.dataset.src;this.parentNode.querySelectorAll('.server-btn').forEach(b=>b.classList.remove('active'));this.classList.add('active')">${esc(s.label)}${s.format ? ' (' + s.format + ')' : ''}</button>`).join('')}
      </div>
      ${p.downloads && p.downloads.length ? `
        <h3 style="margin-top:16px">Downloads</h3>
        ${p.downloads.map(g => `
          <div style="margin-bottom:8px">
            <strong>${esc(g.quality)}</strong>:
            ${g.mirrors.map(m => `<a href="${m.url}" target="_blank" rel="noopener" class="pill" style="display:inline-block;margin:2px;text-decoration:none">${esc(m.name)}</a>`).join('')}
          </div>
        `).join('')}
      ` : ''}
      <div style="margin-top:12px;font-size:12px;color:var(--muted)">
        ${p.prev_id ? `<button class="mini" onclick="loadById('/api/v1/donghua/episode/{id}', '${p.prev_id}')">Prev</button>` : ''}
        ${p.next_id ? `<button class="mini" onclick="loadById('/api/v1/donghua/episode/{id}', '${p.next_id}')">Next</button>` : ''}
      </div>
    </div>
  `;
}

function renderCosplay(p, view) {
  view.innerHTML = `
    <div class="preview-card">
      <h3>${esc(p.title)}</h3>
      <div class="preview-meta">
        ${p.cosplayer ? `<span class="pill">Cosplayer: ${esc(p.cosplayer)}</span>` : ''}
        ${p.character ? `<span class="pill">Character: ${esc(p.character)}</span>` : ''}
        ${p.series ? `<span class="pill">Series: ${esc(p.series)}</span>` : ''}
        ${p.photo_count ? `<span class="pill">${p.photo_count} photos</span>` : ''}
        ${p.video_count ? `<span class="pill">${p.video_count} videos</span>` : ''}
      </div>
      ${p.tags && p.tags.length ? `<div class="preview-meta">${p.tags.map(t => `<span class="pill">${esc(t)}</span>`).join('')}</div>` : ''}
      <div class="preview-grid">
        ${p.images.map(u => `<img src="${u}" loading="lazy" referrerpolicy="no-referrer" onerror="this.style.opacity=0.3" onclick="window.open('${u}', '_blank')">`).join('')}
      </div>
      ${p.downloads && p.downloads.length ? `
        <h3 style="margin-top:16px">Downloads</h3>
        ${p.downloads.map(m => `<a href="${m.url}" target="_blank" rel="noopener" class="pill" style="display:inline-block;margin:2px;text-decoration:none">${esc(m.name)}</a>`).join('')}
      ` : ''}
      ${p.unzip_password ? `<p style="margin-top:12px;font-size:13px"><strong>Unzip password:</strong> <code>${esc(p.unzip_password)}</code></p>` : ''}
    </div>
  `;
}

function renderNovelSeries(p, view) {
  const totalPages = p.chapter_total_pages || 1;
  const curPage = p.chapter_page || 1;
  view.innerHTML = `
    <div class="preview-card">
      <h3>${esc(p.title || 'Novel')}</h3>
      ${p.cover ? `<img class="preview-cover" src="${p.cover}" referrerpolicy="no-referrer" onerror="this.style.display='none'">` : ''}
      <div class="preview-meta">
        ${p.author ? `<span class="pill">Author: ${esc(p.author)}</span>` : ''}
        ${p.rating ? `<span class="pill">Rating: ${esc(p.rating)}</span>` : ''}
        ${p.status ? `<span class="pill">${esc(p.status)}</span>` : ''}
        <span class="pill"><strong>${p.chapter_count}</strong> chapters</span>
      </div>
      ${p.genres && p.genres.length ? `<div class="preview-meta">${p.genres.map(g => `<span class="pill">${esc(g)}</span>`).join('')}</div>` : ''}
      ${p.synopsis ? `<p style="clear:both;font-size:13px;color:var(--muted);line-height:1.6">${esc(p.synopsis.slice(0, 600))}${p.synopsis.length > 600 ? '...' : ''}</p>` : ''}
      <div style="clear:both"></div>
      ${chapterPager(curPage, totalPages, '/api/v1/novel/{id}', p.id)}
      <ul class="preview-list">
        ${p.chapters.map(c => `
          <li>
            <div>
              <span class="num">Bab ${c.number}</span>
              <span>${esc(c.title || '')}</span>
            </div>
            <button class="mini" onclick="loadById('/api/v1/novel/chapter/{id}', '${c.id}')">Read</button>
          </li>
        `).join('')}
      </ul>
      ${chapterPager(curPage, totalPages, '/api/v1/novel/{id}', p.id)}
    </div>
  `;
}

function renderNovelChapter(p, view) {
  const paragraphs = (p.body || '').split(/\n{2,}/).map(s => s.trim()).filter(Boolean);
  view.innerHTML = `
    <div class="preview-card">
      <h3>${esc(p.series_title || 'Novel')} - Bab ${p.chapter_number}</h3>
      ${p.chapter_title ? `<p style="font-size:16px;color:var(--muted);margin-top:-8px">${esc(p.chapter_title)}</p>` : ''}
      <div class="preview-meta">
        <span class="pill"><strong>${p.word_count}</strong> words</span>
      </div>
      <div style="max-width:760px;margin:16px auto;font-size:15px;line-height:1.8;color:var(--text)">
        ${paragraphs.map(par => `<p style="margin:0 0 14px">${esc(par)}</p>`).join('')}
      </div>
      <div style="margin-top:12px;font-size:12px;color:var(--muted);text-align:center">
        ${p.prev_id ? `<button class="mini" onclick="loadById('/api/v1/novel/chapter/{id}', '${p.prev_id}')">&laquo; Bab sebelumnya</button>` : ''}
        ${p.series_id ? `<button class="mini" onclick="loadById('/api/v1/novel/{id}', '${p.series_id}')">Daftar bab</button>` : ''}
        ${p.next_id ? `<button class="mini" onclick="loadById('/api/v1/novel/chapter/{id}', '${p.next_id}')">Bab berikutnya &raquo;</button>` : ''}
      </div>
    </div>
  `;
}

function renderInfo(p, view) {
  view.innerHTML = `
    <div class="preview-card">
      <h3>Server Info</h3>
      <div class="info-grid">
        <div class="info-card"><div class="info-label">CPU</div><div class="info-value">${p.system.cpu_cores}</div></div>
        <div class="info-card"><div class="info-label">RAM</div><div class="info-value">${(p.system.total_mem_mib/1024).toFixed(1)} GB</div></div>
        <div class="info-card"><div class="info-label">Threads</div><div class="info-value">${p.system.tokio_threads}</div></div>
        <div class="info-card"><div class="info-label">Concurrency</div><div class="info-value">${p.system.http_concurrency}</div></div>
      </div>
      <h3>Providers</h3>
      <ul>${p.providers.map(p => `<li><strong>${p.source}</strong> - ${p.label}</li>`).join('')}</ul>
    </div>
  `;
}

function cacheIds(data) {
  const payload = data && data.data;
  if (!payload || !Array.isArray(payload.items)) return;
  for (const it of payload.items) {
    if (it.kind === 'manga') lastIdsBySource['manga_series'] = it.id;
    if (it.kind === 'donghua') lastIdsBySource['donghua_series'] = it.id;
    if (it.kind === 'cosplay') lastIdsBySource['cosplay_post'] = it.id;
    if (it.kind === 'doujin') lastIdsBySource['nhentai_series'] = it.id;
    if (it.kind === 'novel') lastIdsBySource['novel_series'] = it.id;
  }
}

// ---- Copy as cURL --------------------------------------------------------
$('copy-curl-btn').addEventListener('click', async () => {
  const url = location.origin + buildUrl();
  const cmd = `curl '${url}'`;
  try {
    await navigator.clipboard.writeText(cmd);
    $('copy-curl-btn').textContent = 'Copied';
    setTimeout(() => $('copy-curl-btn').textContent = 'Copy as cURL', 1500);
  } catch {
    prompt('Copy this:', cmd);
  }
});

// Re-bind language tabs when entering the examples tab
document.addEventListener('click', () => {
  bindLangTabs();
  bindCopyButtons();
}, true);
