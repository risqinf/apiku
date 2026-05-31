// apiku consumer SPA — dependency-free hash router.
// Talks to /api/v1/* JSON endpoints. Renders home, browse, search,
// per-provider detail / watch / read / gallery views, plus an API explorer.
(function () {
  "use strict";

  const API = "/api/v1";
  const app = document.getElementById("app");
  const CHAPTER_SIZE = 60;

  // ---- Provider config ---------------------------------------------------
  // `adult: true` means the provider is hidden unless the 18+ toggle is on.
  const PROVIDERS = {
    donghua: { label: "Donghua", api: "anichin",     kind: "donghua", adult: false },
    manga:   { label: "Manga",   api: "mangaball",   kind: "manga",   adult: false },
    novel:   { label: "Novel",   api: "novelid",     kind: "novel",   adult: false },
    cosplay: { label: "Cosplay", api: "cosplaytele", kind: "cosplay", adult: true },
    doujin:  { label: "Doujin",  api: "nhentai",     kind: "doujin",  adult: true },
  };

  const FEEDS = {
    anichin:     [["home","Terbaru"],["popular","Populer"],["rating","Rating"],["title","A-Z"]],
    mangaball:   [["home","Unggulan"],["popular","Populer"],["latest","Terbaru"],["recommend","Rekomendasi"]],
    novelid:     [["home","Semua"],["popular","Tamat"],["novel-translate","Translate"],["fantasi","Fantasi"],["romantis","Romantis"],["aksi","Aksi"],["horror","Horror"]],
    cosplaytele: [["home","Terbaru"],["popular","Populer"]],
    nhentai:     [["popular-today","Hari Ini"],["popular-week","Minggu Ini"],["popular","Sepanjang Masa"],["home","Terbaru"]],
  };

  // Map content kind -> the API detail endpoint family
  const DETAIL_EP = {
    donghua: "donghua", manga: "manga", novel: "novel", cosplay: "cosplay", doujin: "nhentai",
  };

  // ---- Persisted preferences ---------------------------------------------
  const store = {
    get theme() { return localStorage.getItem("apiku.theme") || "dark"; },
    set theme(v) { localStorage.setItem("apiku.theme", v); },
    get adult() { return localStorage.getItem("apiku.adult") === "1"; },
    set adult(v) { localStorage.setItem("apiku.adult", v ? "1" : "0"); },
  };
  function applyTheme() {
    document.documentElement.setAttribute("data-theme", store.theme);
  }
  applyTheme();

  function adultOn() { return store.adult; }
  function providerVisible(kind) {
    const p = Object.values(PROVIDERS).find(x => x.kind === kind);
    return p ? (!p.adult || adultOn()) : true;
  }

  // ---- Tiny helpers -------------------------------------------------------
  const h = (s) => (s == null ? "" : String(s)
    .replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;")
    .replace(/"/g,"&quot;").replace(/'/g,"&#39;"));
  const qs = (o) => Object.entries(o).filter(([,v]) => v!=null && v!=="")
    .map(([k,v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&");

  async function api(path) {
    const r = await fetch(API + path);
    const j = await r.json();
    if (!j.ok) throw new Error(j.error ? `${j.error.code}: ${j.error.message}` : "request failed");
    return j.data;
  }
  async function apiRaw(method, path, qstr) {
    const url = API + path + (qstr ? (path.includes("?") ? "&" : "?") + qstr : "");
    const t0 = performance.now();
    const r = await fetch(url, { method });
    const text = await r.text();
    let json; try { json = JSON.parse(text); } catch { json = text; }
    return { status: r.status, ms: Math.round(performance.now() - t0), json };
  }

  function go(hash) { location.hash = hash; }
  function viewEl() { return document.getElementById("view"); }
  function setView(html) { const v = viewEl(); if (v) v.innerHTML = html; }
  const spinner = `<div class="spinner"></div>`;
  function skelGrid(n) {
    return `<div class="grid">${Array.from({length:n||12}).map(()=>`<div class="skeleton poster"></div>`).join("")}</div>`;
  }

  function imgTag(url, _cls, alt) {
    if (!url) return `<div class="ph">${h(alt||"no image")}</div>`;
    return `<img loading="lazy" referrerpolicy="no-referrer" src="${h(url)}" alt="${h(alt||"")}"
      onerror="this.parentNode.innerHTML='<div class=ph>no image</div>'">`;
  }

  function setActiveNav() {
    const hash = location.hash;
    let seg = (hash.replace(/^#\//,"").split("/")[0]) || "home";
    document.querySelectorAll(".hdr nav a").forEach(a => {
      a.classList.toggle("active", a.dataset.seg === seg);
    });
  }

  // ---- Shell --------------------------------------------------------------
  function shell(inner) {
    const navItems = [
      { seg: "home", href: "#/", label: "Home" },
      { seg: "donghua", href: "#/browse/donghua", label: "Donghua" },
      { seg: "manga", href: "#/browse/manga", label: "Manga" },
      { seg: "novel", href: "#/browse/novel", label: "Novel" },
      { seg: "cosplay", href: "#/browse/cosplay", label: "Cosplay", adult: true },
      { seg: "doujin", href: "#/browse/doujin", label: "Doujin", adult: true },
      { seg: "explorer", href: "#/explorer", label: "Explorer" },
    ].filter(n => !n.adult || adultOn());

    const themeIcon = store.theme === "dark" ? "&#9728;" : "&#9789;"; // sun / moon

    app.innerHTML = `
      <header class="hdr">
        <a class="brand" href="#/"><span>&#128250;</span><span class="full"><b>api</b>ku</span></a>
        <nav>
          ${navItems.map(n => `<a data-seg="${n.seg}" href="${n.href}">${n.label}</a>`).join("")}
        </nav>
        <div class="spacer"></div>
        <form class="searchbox" id="searchform">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
          <input id="searchinput" type="search" placeholder="Cari judul..." autocomplete="off">
        </form>
        <button class="icon-btn" id="adultBtn" title="Konten 18+">${adultOn() ? "&#128286; 18+" : "18+"}</button>
        <button class="icon-btn" id="themeBtn" title="Ganti tema">${themeIcon}</button>
      </header>
      <main id="view">${inner}</main>
      <footer>
        Ditenagai <a href="https://github.com/risqinf/apiku" target="_blank" rel="noopener">apiku</a> &middot;
        <a href="#/explorer">API Explorer</a> &middot;
        <a href="/tester">Dev console</a> &middot;
        Konten berasal dari sumber pihak ketiga.
      </footer>`;

    setActiveNav();

    const form = document.getElementById("searchform");
    const input = document.getElementById("searchinput");
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const q = input.value.trim();
      if (q) go(`#/search/${encodeURIComponent(q)}`);
    });
    const m = location.hash.match(/^#\/search\/([^/]+)/);
    if (m) input.value = decodeURIComponent(m[1]);

    document.getElementById("themeBtn").addEventListener("click", () => {
      store.theme = store.theme === "dark" ? "light" : "dark";
      applyTheme();
      router(); // re-render to refresh the icon
    });
    document.getElementById("adultBtn").addEventListener("click", () => {
      if (!adultOn()) {
        if (!confirm("Aktifkan konten 18+? Hanya untuk pengguna dewasa (18+).")) return;
      }
      store.adult = !store.adult;
      // If we're currently on a now-hidden adult page, bounce home.
      const seg = (location.hash.replace(/^#\//,"").split("/")[1]) || "";
      if (!adultOn() && (seg === "cosplay" || seg === "doujin")) { go("#/"); return; }
      router();
    });
  }

  // ---- Card rendering -----------------------------------------------------
  function cardHtml(item) {
    const prov = Object.values(PROVIDERS).find(p => p.kind === item.kind) || {};
    const tags = (item.tags || []).slice(0, 2).map(t => `<span>${h(t)}</span>`).join("");
    return `
      <div class="card" data-go="#/detail/${encodeURIComponent(item.kind)}/${encodeURIComponent(item.id)}">
        <div class="poster">
          ${imgTag(item.thumbnail, "", item.title)}
          <span class="badge src">${h(prov.label || item.source)}</span>
        </div>
        <div class="meta">
          <div class="t">${h(item.title)}</div>
          <div class="sub">${tags}</div>
        </div>
      </div>`;
  }

  function grid(items) {
    if (!items || !items.length) return `<div class="empty">Tidak ada hasil.</div>`;
    return `<div class="grid">${items.map(cardHtml).join("")}</div>`;
  }

  // delegate card / [data-go] clicks
  document.addEventListener("click", (e) => {
    const el = e.target.closest("[data-go]");
    if (el) { e.preventDefault(); go(el.dataset.go); }
  });

  function crumbs(items) {
    return `<div class="crumbs">` +
      items.map((it, i) => i < items.length - 1
        ? `<a href="${it.href}">${h(it.label)}</a><span>/</span>`
        : `<b>${h(it.label)}</b>`).join("") +
      `</div>`;
  }

  // ===========================================================================
  // Routes: home / browse / search
  // ===========================================================================
  async function routeHome() {
    shell(`
      <div class="row-head"><h2>Selamat datang &#128075;</h2></div>
      <p style="color:var(--muted);margin-top:-8px">Streaming donghua, baca manga &amp; novel, galeri cosplay - semua di satu tempat.</p>
      <div id="rows"></div>
    `);
    const rows = document.getElementById("rows");
    let sections = [
      { title: "Donghua Terbaru", prov: "anichin",     feed: "home",          seg: "donghua" },
      { title: "Manga Populer",   prov: "mangaball",   feed: "popular",       seg: "manga" },
      { title: "Novel Terbaru",   prov: "novelid",     feed: "home",          seg: "novel" },
      { title: "Cosplay Terbaru", prov: "cosplaytele", feed: "home",          seg: "cosplay", adult: true },
      { title: "Doujin Hari Ini", prov: "nhentai",     feed: "popular-today", seg: "doujin",  adult: true },
    ].filter(s => !s.adult || adultOn());

    rows.innerHTML = sections.map((s,i) => `
      <div class="row-head"><h2>${h(s.title)}</h2><a class="more" href="#/browse/${s.seg}">Lihat semua &rarr;</a></div>
      <div id="row-${i}">${skelGrid(6)}</div>
    `).join("");
    sections.forEach(async (s, i) => {
      try {
        const data = await api(`/browse/${s.prov}?${qs({ feed: s.feed })}`);
        document.getElementById(`row-${i}`).innerHTML = grid((data.items || []).slice(0, 12));
      } catch (e) {
        const el = document.getElementById(`row-${i}`);
        if (el) el.innerHTML = `<div class="errbox">Gagal memuat.</div>`;
      }
    });
  }

  async function routeBrowse(seg, feed, page) {
    const prov = PROVIDERS[seg];
    if (!prov) return routeHome();
    if (prov.adult && !adultOn()) return routeHome();
    page = parseInt(page || "1", 10);
    const feeds = FEEDS[prov.api] || [["home","Semua"]];
    feed = feed || feeds[0][0];
    shell(`
      <div class="row-head"><h2>${h(prov.label)}</h2></div>
      <div class="chips">
        ${feeds.map(([v,l]) => `<a class="chip ${v===feed?"active":""}" href="#/browse/${seg}/${v}">${h(l)}</a>`).join("")}
      </div>
      <div id="list">${skelGrid(18)}</div>
      <div id="pager"></div>
    `);
    try {
      const data = await api(`/browse/${prov.api}?${qs({ feed, page })}`);
      document.getElementById("list").innerHTML = grid(data.items);
      document.getElementById("pager").innerHTML = `
        <div class="pager">
          ${page>1 ? `<a class="btn sm" href="#/browse/${seg}/${feed}/${page-1}">&larr; Sebelumnya</a>` : ""}
          <span>Halaman ${page}</span>
          ${(data.items && data.items.length) ? `<a class="btn sm" href="#/browse/${seg}/${feed}/${page+1}">Berikutnya &rarr;</a>` : ""}
        </div>`;
    } catch (e) {
      document.getElementById("list").innerHTML = `<div class="errbox">${h(e.message)}</div>`;
    }
  }

  async function routeSearch(query, page) {
    page = parseInt(page || "1", 10);
    shell(`
      <div class="row-head"><h2>Hasil: &ldquo;${h(query)}&rdquo;</h2></div>
      <div id="list">${skelGrid(12)}</div>
    `);
    try {
      const data = await api(`/search?${qs({ q: query, source: "all", page })}`);
      // Hide adult results unless the toggle is on.
      const items = (data.items || []).filter(it => providerVisible(it.kind));
      document.getElementById("list").innerHTML = grid(items);
    } catch (e) {
      document.getElementById("list").innerHTML = `<div class="errbox">${h(e.message)}</div>`;
    }
  }

  // ===========================================================================
  // Detail / watch / read
  // ===========================================================================
  function setD(html) { const el = document.getElementById("d"); if (el) el.innerHTML = html; }

  function heroHtml(kind, label, data, factsExtra, actionsHtml, synopsis, cover) {
    return `
      ${crumbs([{href:"#/",label:"Home"},{href:`#/browse/${kind}`,label:label},{label:data.title||"Detail"}])}
      <div class="detail-hero">
        <div class="poster">${imgTag(cover, "", data.title)}</div>
        <div class="info">
          <h1>${h(data.title)}</h1>
          <div class="facts">${factsExtra}</div>
          ${synopsis ? `<p class="syn">${h(synopsis)}</p>` : ""}
          <div class="actions">${actionsHtml}</div>
        </div>
      </div>`;
  }

  async function routeDetail(kind, id) {
    shell(`<div id="d">${spinner}</div>`);
    const ep = DETAIL_EP[kind];
    if (!ep) return setD(`<div class="errbox">Tipe tidak dikenal: ${h(kind)}</div>`);
    try {
      if (kind === "cosplay") return renderCosplay(id);
      if (kind === "doujin") return renderDoujin(id);
      const data = await api(`/${ep}/${encodeURIComponent(id)}?${qs({ page: 1, size: CHAPTER_SIZE })}`);
      if (kind === "donghua") return renderDonghuaSeries(id, data);
      return renderReadableSeries(kind, id, data, 1);
    } catch (e) {
      setD(`<div class="errbox">${h(e.message)}</div>`);
    }
  }

  function renderDonghuaSeries(id, data) {
    const eps = data.episodes || [];
    const facts = [
      data.status ? `<span class="pill ok">${h(data.status)}</span>` : "",
      `<span class="pill">${data.episode_count} episode</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const firstEp = eps[0];
    const actions = firstEp
      ? `<a class="btn primary" href="#/watch/${encodeURIComponent(firstEp.id)}">&#9654; Tonton Eps ${firstEp.number}</a>`
      : "";
    const epList = eps.length
      ? `<div class="ep-list">${eps.map(e=>`<button class="ep-btn center" data-go="#/watch/${encodeURIComponent(e.id)}">Eps ${e.number}</button>`).join("")}</div>`
      : `<div class="empty">Belum ada episode.</div>`;
    setD(
      heroHtml("donghua","Donghua",data,facts,actions,data.synopsis,data.cover) +
      `<div class="row-head"><h2>Episode</h2></div>${epList}`
    );
  }

  async function renderReadableSeries(kind, id, data, page) {
    const label = kind === "manga" ? "Manga" : "Novel";
    const chs = data.chapters || [];
    const totalPages = data.chapter_total_pages || 1;
    const facts = [
      data.status ? `<span class="pill ok">${h(data.status)}</span>` : "",
      data.author ? `<span class="pill">&#9997; ${h(data.author)}</span>` : "",
      data.rating ? `<span class="pill">&#9733; ${h(data.rating)}</span>` : "",
      `<span class="pill">${data.chapter_count} bab</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = chs[0];
    const readPath = kind === "manga" ? "read/manga" : "read/novel";
    const actions = first
      ? `<a class="btn primary" href="#/${readPath}/${encodeURIComponent(first.id)}">&#128214; Mulai Baca</a>`
      : "";
    const syn = data.description || data.synopsis;
    const chList = chs.length
      ? `<div class="ep-list wide">${chs.map(c=>`
          <button class="ep-btn" data-go="#/${readPath}/${encodeURIComponent(c.id)}">
            <span>Bab ${h(c.number)}${c.title?` &middot; ${h(c.title)}`:""}</span>
          </button>`).join("")}</div>`
      : `<div class="empty">Belum ada bab.</div>`;
    const pager = totalPages > 1 ? `
      <div class="pager">
        ${page>1?`<button class="btn sm" id="ch-prev">&larr; Bab sebelumnya</button>`:""}
        <span>Halaman ${page} / ${totalPages}</span>
        ${page<totalPages?`<button class="btn sm" id="ch-next">Bab berikutnya &rarr;</button>`:""}
      </div>` : "";
    setD(
      heroHtml(kind, label, data, facts, actions, syn, data.cover) +
      `<div class="row-head"><h2>Daftar Bab</h2></div>${pager}${chList}${pager}`
    );
    const ep = DETAIL_EP[kind];
    const load = async (p) => {
      const el = document.getElementById("d");
      el.querySelectorAll(".ep-list").forEach(n => n.innerHTML = `<div class="spinner"></div>`);
      const fresh = await api(`/${ep}/${encodeURIComponent(id)}?${qs({ page: p, size: CHAPTER_SIZE })}`);
      renderReadableSeries(kind, id, fresh, p);
    };
    const pv = document.getElementById("ch-prev"); if (pv) pv.onclick = () => load(page-1);
    const nx = document.getElementById("ch-next"); if (nx) nx.onclick = () => load(page+1);
  }

  async function renderCosplay(id) {
    const data = await api(`/cosplay/${encodeURIComponent(id)}`);
    const facts = [
      data.cosplayer ? `<span class="pill">${h(data.cosplayer)}</span>` : "",
      data.character ? `<span class="pill">${h(data.character)}</span>` : "",
      data.series ? `<span class="pill">${h(data.series)}</span>` : "",
      data.photo_count ? `<span class="pill">${data.photo_count} foto</span>` : "",
      ...(data.tags||[]).slice(0,4).map(t=>`<span class="pill">${h(t)}</span>`),
    ].join("");
    const dls = (data.downloads||[]).map(d=>`<a class="btn sm" target="_blank" rel="noopener" href="${h(d.url)}">${h(d.name)}</a>`).join("");
    const actions = dls + (data.unzip_password?`<span class="pill">&#128273; ${h(data.unzip_password)}</span>`:"");
    const imgs = (data.images||[]).map(u=>`<a href="${h(u)}" target="_blank" rel="noopener">${imgTag(u,"","")}</a>`).join("");
    setD(
      heroHtml("cosplay","Cosplay",data,facts,actions,null,data.cover) +
      `<div class="row-head"><h2>${(data.images||[]).length} Foto</h2></div>
       <div class="gallery">${imgs}</div>`
    );
  }

  async function renderDoujin(id) {
    const data = await api(`/nhentai/${encodeURIComponent(id)}`);
    const facts = [
      data.author ? `<span class="pill">${h(data.author)}</span>` : "",
      ...(data.genres||[]).slice(0,6).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = (data.chapters||[])[0];
    const actions = first
      ? `<a class="btn primary" href="#/read/nhentai/${encodeURIComponent(first.id)}">&#128214; Baca</a>`
      : "";
    setD(heroHtml("doujin","Doujin",data,facts,actions,data.description,data.cover));
  }

  // ---- Watch (donghua episode) -------------------------------------------
  async function routeWatch(id) {
    shell(`<div id="d">${spinner}</div>`);
    try {
      const e = await api(`/donghua/episode/${encodeURIComponent(id)}`);
      const servers = e.servers || [];
      const seriesLink = e.series_id ? `#/detail/donghua/${encodeURIComponent(e.series_id)}` : "#/";
      const player = servers.length
        ? `<div class="player-wrap"><div class="frame"><iframe id="player" src="${h(servers[0].embed_url)}" allowfullscreen allow="autoplay; encrypted-media; picture-in-picture"></iframe></div></div>`
        : `<div class="empty">Tidak ada server video.</div>`;
      const bar = servers.length
        ? `<div class="server-bar">${servers.map((s,i)=>`<button class="srv ${i===0?"active":""}" data-src="${h(s.embed_url)}">${h(s.label)}${s.format?` &middot; ${h(s.format)}`:""}</button>`).join("")}</div>`
        : "";
      const dls = (e.downloads||[]).map(g=>`
        <div class="dl-group"><div class="q">${h(g.quality)}</div>
          <div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div>
        </div>`).join("");
      const navBtns = `
        <div class="server-bar" style="margin-top:8px">
          ${e.prev_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.prev_id)}">&larr; Eps sebelumnya</a>`:""}
          <a class="btn sm" href="${seriesLink}">&#9776; Semua episode</a>
          ${e.next_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.next_id)}">Eps berikutnya &rarr;</a>`:""}
        </div>`;
      setView(
        crumbs([{href:"#/",label:"Home"},{href:"#/browse/donghua",label:"Donghua"},{label:`${e.series_title||"Episode"} - Eps ${e.episode_number}`}]) +
        `<div class="row-head"><h2>${h(e.series_title||"Episode")} - Episode ${e.episode_number}</h2></div>` +
        player + bar + navBtns +
        (dls ? `<div class="row-head"><h2>Unduh</h2></div>${dls}` : "")
      );
      document.querySelectorAll(".server-bar .srv").forEach(btn => {
        btn.onclick = () => {
          document.getElementById("player").src = btn.dataset.src;
          document.querySelectorAll(".server-bar .srv").forEach(b=>b.classList.remove("active"));
          btn.classList.add("active");
        };
      });
    } catch (e) {
      setView(`<div class="errbox">${h(e.message)}</div>`);
    }
  }

  // ---- Read (manga / doujin pages, novel text) ---------------------------
  async function routeRead(kind, id) {
    shell(`<div id="d">${spinner}</div>`);
    try {
      if (kind === "novel") return renderNovelChapter(id);
      const ep = kind === "nhentai" ? "nhentai/chapter" : "manga/chapter";
      const c = await api(`/${ep}/${encodeURIComponent(id)}`);
      const pages = c.pages || [];
      const imgs = pages.map(p=>`<img loading="lazy" referrerpolicy="no-referrer" src="${h(p.url)}" alt="page ${p.index}" onerror="this.style.opacity=.25">`).join("");
      setView(
        `<div class="row-head"><h2>${h(c.series_title||"Baca")} ${c.chapter_number?`&middot; Ch ${c.chapter_number}`:""}</h2></div>` +
        `<div class="reader">${pages.length?imgs:`<div class="empty">Tidak ada halaman.</div>`}</div>`
      );
    } catch (e) {
      setView(`<div class="errbox">${h(e.message)}</div>`);
    }
  }

  async function renderNovelChapter(id) {
    const c = await api(`/novel/chapter/${encodeURIComponent(id)}`);
    const paras = (c.body || "").split(/\n{2,}/).map(s=>s.trim()).filter(Boolean)
      .map(p=>`<p>${h(p)}</p>`).join("");
    const nav = `
      <div class="reader-nav">
        ${c.prev_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.prev_id)}">&larr; Sebelumnya</a>`:""}
        ${c.series_id?`<a class="btn sm" href="#/detail/novel/${encodeURIComponent(c.series_id)}">&#9776; Daftar bab</a>`:""}
        ${c.next_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.next_id)}">Berikutnya &rarr;</a>`:""}
      </div>`;
    setView(
      `<div class="row-head"><h2>${h(c.series_title||"Novel")} &middot; Bab ${c.chapter_number}</h2></div>` +
      (c.chapter_title?`<p style="color:var(--muted);margin-top:-8px">${h(c.chapter_title)}</p>`:"") +
      `<div class="novel-body">${paras||"<p>(kosong)</p>"}</div>` + nav
    );
  }

  // ===========================================================================
  // Explorer (in-app API console)
  // ===========================================================================
  const EXPLORER_ENDPOINTS = [
    { m:"GET", p:"/api/v1/health", note:"Liveness" },
    { m:"GET", p:"/api/v1/info", note:"Server info + providers" },
    { m:"GET", p:"/api/v1/search?q=one+piece&source=all&page=1", note:"Cross-provider search" },
    { m:"GET", p:"/api/v1/browse/anichin?feed=home&page=1", note:"Donghua feed" },
    { m:"GET", p:"/api/v1/browse/mangaball?feed=popular", note:"Manga feed" },
    { m:"GET", p:"/api/v1/browse/novelid?feed=home", note:"Novel feed" },
    { m:"GET", p:"/api/v1/browse/cosplaytele?feed=home", note:"Cosplay feed (18+)" },
    { m:"GET", p:"/api/v1/browse/nhentai?feed=popular-today", note:"Doujin feed (18+)" },
    { m:"GET", p:"/api/v1/manga/{id}?page=1&size=60", note:"Manga series (paste opaque id)" },
    { m:"GET", p:"/api/v1/manga/chapter/{id}", note:"Manga chapter pages" },
    { m:"GET", p:"/api/v1/donghua/{id}", note:"Donghua series" },
    { m:"GET", p:"/api/v1/donghua/episode/{id}", note:"Donghua episode + servers" },
    { m:"GET", p:"/api/v1/novel/{id}?page=1&size=60", note:"Novel series" },
    { m:"GET", p:"/api/v1/novel/chapter/{id}", note:"Novel chapter text" },
    { m:"GET", p:"/api/v1/cosplay/{id}", note:"Cosplay post (18+)" },
    { m:"GET", p:"/api/v1/nhentai/{id}", note:"Doujin gallery (18+)" },
    { m:"GET", p:"/api/v1/nhentai/chapter/{id}", note:"Doujin pages (18+)" },
  ];

  function routeExplorer() {
    shell(`
      <div class="row-head"><h2>API Explorer</h2></div>
      <p style="color:var(--muted);margin-top:-8px">Uji endpoint <code>/api/v1/*</code> langsung dan lihat respons JSON mentah. Sama seperti <a href="/tester">dev console</a> tapi inline.</p>
      <div class="explorer">
        <div class="exp-controls">
          <select id="exp-preset">
            <option value="">— pilih endpoint —</option>
            ${EXPLORER_ENDPOINTS.map((e,i)=>`<option value="${i}">${e.m} ${h(e.p)}  (${h(e.note)})</option>`).join("")}
          </select>
          <div class="exp-row">
            <input id="exp-path" type="text" value="/api/v1/info" spellcheck="false">
            <button class="btn primary" id="exp-send">Kirim</button>
          </div>
          <p class="muted" style="font-size:12px">Tip: ganti <code>{id}</code> dengan opaque id dari hasil search/browse.</p>
        </div>
        <div class="exp-meta" id="exp-meta"></div>
        <pre class="exp-out" id="exp-out">// Respons akan tampil di sini.</pre>
      </div>
    `);
    const pathInput = document.getElementById("exp-path");
    const preset = document.getElementById("exp-preset");
    preset.addEventListener("change", () => {
      const i = preset.value;
      if (i !== "") pathInput.value = EXPLORER_ENDPOINTS[parseInt(i,10)].p;
    });
    const send = async () => {
      let path = pathInput.value.trim();
      if (!path.startsWith("/api/v1/")) {
        // tolerate "/info" or "info"
        path = "/api/v1/" + path.replace(/^\/+/,"").replace(/^api\/v1\//,"");
      }
      const rel = path.replace(/^\/api\/v1/, "");
      const meta = document.getElementById("exp-meta");
      const out = document.getElementById("exp-out");
      meta.innerHTML = `<span class="pill">…</span>`;
      out.textContent = "Loading…";
      try {
        const res = await apiRaw("GET", rel);
        const cls = res.status === 200 ? "ok" : "";
        meta.innerHTML = `<span class="pill ${cls}">HTTP ${res.status}</span> <span class="pill">${res.ms} ms</span>`;
        out.textContent = typeof res.json === "string" ? res.json : JSON.stringify(res.json, null, 2);
      } catch (e) {
        meta.innerHTML = `<span class="pill">error</span>`;
        out.textContent = String(e.message || e);
      }
    };
    document.getElementById("exp-send").addEventListener("click", send);
    pathInput.addEventListener("keydown", (e) => { if (e.key === "Enter") send(); });
  }

  // ===========================================================================
  // Router
  // ===========================================================================
  function router() {
    const parts = location.hash.replace(/^#\//, "").split("/").map(decodeURIComponent);
    const seg = parts[0] || "";
    window.scrollTo(0, 0);
    switch (seg) {
      case "":
      case "home":     return routeHome();
      case "browse":   return routeBrowse(parts[1], parts[2], parts[3]);
      case "search":   return routeSearch(parts[1] || "", parts[2]);
      case "detail":   return routeDetail(parts[1], parts[2]);
      case "watch":    return routeWatch(parts[1]);
      case "read":     return routeRead(parts[1], parts[2]);
      case "explorer": return routeExplorer();
      default:         return routeHome();
    }
  }

  window.addEventListener("hashchange", router);
  window.addEventListener("load", router);
  // In case the script loads after DOMContentLoaded (it's at end of body)
  router();
})();
