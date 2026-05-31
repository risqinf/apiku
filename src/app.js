// apiku consumer SPA — dependency-free hash router.
// Talks to /api/v1/* JSON endpoints. Renders home, browse, search,
// and per-provider detail / watch / read / gallery views.
(function () {
  "use strict";

  const API = "/api/v1";
  const app = document.getElementById("app");

  // ---- Provider config ---------------------------------------------------
  const PROVIDERS = {
    donghua:  { label: "Donghua",  api: "anichin",     kind: "donghua", verb: "Tonton" },
    manga:    { label: "Manga",    api: "mangaball",   kind: "manga",   verb: "Baca" },
    novel:    { label: "Novel",    api: "novelid",     kind: "novel",   verb: "Baca" },
    cosplay:  { label: "Cosplay",  api: "cosplaytele", kind: "cosplay", verb: "Lihat" },
    doujin:   { label: "Doujin",   api: "nhentai",     kind: "doujin",  verb: "Baca" },
  };

  const FEEDS = {
    anichin:     [["home","Terbaru"],["popular","Populer"],["rating","Rating"],["title","A-Z"]],
    mangaball:   [["home","Unggulan"],["popular","Populer"],["latest","Terbaru"],["recommend","Rekomendasi"]],
    novelid:     [["home","Semua"],["popular","Tamat"],["novel-translate","Translate"],["fantasi","Fantasi"],["romantis","Romantis"],["aksi","Aksi"],["horror","Horror"]],
    cosplaytele: [["home","Terbaru"],["popular","Populer"]],
    nhentai:     [["popular-today","Hari Ini"],["popular-week","Minggu Ini"],["popular","Sepanjang Masa"],["home","Terbaru"]],
  };

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

  function setActiveNav() {
    const seg = (location.hash.replace(/^#\//,"").split("/")[0]) || "home";
    document.querySelectorAll(".hdr nav a").forEach(a => {
      a.classList.toggle("active", a.dataset.seg === seg);
    });
  }

  function go(hash) { location.hash = hash; }

  function imgTag(url, cls, alt) {
    if (!url) return `<div class="ph">${h(alt||"no image")}</div>`;
    return `<img loading="lazy" referrerpolicy="no-referrer" src="${h(url)}" alt="${h(alt||"")}"
      onerror="this.parentNode.innerHTML='<div class=ph>no image</div>'">`;
  }

  // ---- Shell --------------------------------------------------------------
  function shell(inner) {
    app.innerHTML = `
      <header class="hdr">
        <a class="brand" href="#/"><span>📺</span><span class="full"><b>api</b>ku</span></a>
        <nav>
          <a data-seg="home" href="#/">Home</a>
          <a data-seg="donghua" href="#/browse/donghua">Donghua</a>
          <a data-seg="manga" href="#/browse/manga">Manga</a>
          <a data-seg="novel" href="#/browse/novel">Novel</a>
          <a data-seg="cosplay" href="#/browse/cosplay">Cosplay</a>
          <a data-seg="doujin" href="#/browse/doujin">Doujin</a>
        </nav>
        <div class="spacer"></div>
        <form class="searchbox" id="searchform">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
          <input id="searchinput" type="search" placeholder="Cari judul…" autocomplete="off">
        </form>
      </header>
      <main id="view">${inner}</main>
      <footer>
        Ditenagai <a href="https://github.com/risqinf/apiku" target="_blank" rel="noopener">apiku</a> ·
        <a href="/tester">API console</a> ·
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
    // preserve query text when on a search route
    const m = location.hash.match(/^#\/search\/([^/]+)/);
    if (m) input.value = decodeURIComponent(m[1]);
  }

  function viewEl() { return document.getElementById("view"); }
  function setView(html) { const v = viewEl(); if (v) v.innerHTML = html; }
  const spinner = `<div class="spinner"></div>`;
  function skelGrid(n) {
    return `<div class="grid">${Array.from({length:n||12}).map(()=>`<div class="skeleton poster"></div>`).join("")}</div>`;
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

  // delegate card clicks
  app && document.addEventListener("click", (e) => {
    const el = e.target.closest("[data-go]");
    if (el) { e.preventDefault(); go(el.dataset.go); }
  });

  // ---- Routes -------------------------------------------------------------
  async function routeHome() {
    shell(`
      <div class="row-head"><h2>Selamat datang 👋</h2></div>
      <p style="color:var(--muted);margin-top:-8px">Streaming donghua, baca manga & novel, galeri cosplay — semua di satu tempat.</p>
      <div id="rows"></div>
    `);
    const rows = document.getElementById("rows");
    const sections = [
      { title: "Donghua Terbaru",  prov: "anichin",     feed: "home",          kind: "donghua", seg: "donghua" },
      { title: "Manga Populer",    prov: "mangaball",   feed: "popular",       kind: "manga",   seg: "manga" },
      { title: "Novel Terbaru",    prov: "novelid",     feed: "home",          kind: "novel",   seg: "novel" },
      { title: "Cosplay Terbaru",  prov: "cosplaytele", feed: "home",          kind: "cosplay", seg: "cosplay" },
      { title: "Doujin Hari Ini",  prov: "nhentai",     feed: "popular-today", kind: "doujin",  seg: "doujin" },
    ];
    rows.innerHTML = sections.map((s,i) => `
      <div class="row-head"><h2>${h(s.title)}</h2><a class="more" href="#/browse/${s.seg}">Lihat semua →</a></div>
      <div id="row-${i}">${skelGrid(6)}</div>
    `).join("");
    sections.forEach(async (s, i) => {
      try {
        const data = await api(`/browse/${s.prov}?${qs({ feed: s.feed })}`);
        const items = (data.items || []).slice(0, 12);
        document.getElementById(`row-${i}`).innerHTML = grid(items);
      } catch (e) {
        document.getElementById(`row-${i}`).innerHTML = `<div class="errbox">Gagal memuat.</div>`;
      }
    });
  }

  async function routeBrowse(seg, feed, page) {
    const prov = PROVIDERS[seg];
    if (!prov) return routeHome();
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
      const pg = document.getElementById("pager");
      pg.innerHTML = `
        <div class="pager">
          ${page>1 ? `<a class="btn sm" href="#/browse/${seg}/${feed}/${page-1}">← Sebelumnya</a>` : ""}
          <span>Halaman ${page}</span>
          ${(data.items && data.items.length) ? `<a class="btn sm" href="#/browse/${seg}/${feed}/${page+1}">Berikutnya →</a>` : ""}
        </div>`;
    } catch (e) {
      document.getElementById("list").innerHTML = `<div class="errbox">${h(e.message)}</div>`;
    }
  }

  async function routeSearch(query, page) {
    page = parseInt(page || "1", 10);
    shell(`
      <div class="row-head"><h2>Hasil: “${h(query)}”</h2></div>
      <div id="list">${skelGrid(12)}</div>
    `);
    try {
      const data = await api(`/search?${qs({ q: query, source: "all", page })}`);
      document.getElementById("list").innerHTML = grid(data.items);
    } catch (e) {
      document.getElementById("list").innerHTML = `<div class="errbox">${h(e.message)}</div>`;
    }
  }

  window.addEventListener("hashchange", router);
  window.addEventListener("load", router);

  function router() {
    const parts = location.hash.replace(/^#\//, "").split("/").map(decodeURIComponent);
    const seg = parts[0] || "";
    window.scrollTo(0, 0);
    if (seg === "" || seg === "home") return routeHome();
    if (seg === "browse") return routeBrowse(parts[1], parts[2], parts[3]);
    if (seg === "search") return routeSearch(parts[1] || "", parts[2]);
    if (seg === "detail") return routeDetail(parts[1], parts[2]);
    if (seg === "watch") return routeWatch(parts[1]);
    if (seg === "read") return routeRead(parts[1], parts[2]);
    return routeHome();
  }

  // detail/watch/read defined in part 2 (appended below)
  window.__apiku = { shell, setView, grid, api, h, qs, imgTag, spinner, go, PROVIDERS, viewEl };
})();

// ===========================================================================
// Detail / watch / read views
// ===========================================================================
(function () {
  "use strict";
  const A = window.__apiku;
  const { shell, setView, api, h, qs, imgTag, spinner, go } = A;

  const CHAPTER_SIZE = 60;

  // Map content kind -> the API detail endpoint family
  const DETAIL_EP = {
    donghua: "donghua",
    manga: "manga",
    novel: "novel",
    cosplay: "cosplay",
    doujin: "nhentai",
  };

  function crumbs(items) {
    return `<div class="crumbs">` +
      items.map((it, i) => i < items.length - 1
        ? `<a href="${it.href}">${h(it.label)}</a><span>/</span>`
        : `<b>${h(it.label)}</b>`).join("") +
      `</div>`;
  }

  // ---- Detail (series / post) --------------------------------------------
  A.routeDetail = async function routeDetail(kind, id) {
    shell(`<div id="d">${spinner}</div>`);
    const ep = DETAIL_EP[kind];
    try {
      if (kind === "cosplay") return renderCosplay(id);
      if (kind === "doujin") return renderDoujin(id);
      const data = await api(`/${ep}/${encodeURIComponent(id)}?${qs({ page: 1, size: CHAPTER_SIZE })}`);
      if (kind === "donghua") return renderDonghuaSeries(id, data);
      return renderReadableSeries(kind, id, data, 1);
    } catch (e) {
      setD(`<div class="errbox">${h(e.message)}</div>`);
    }
  };

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

  // ---- Donghua series -----------------------------------------------------
  function renderDonghuaSeries(id, data) {
    const eps = data.episodes || [];
    const facts = [
      data.status ? `<span class="pill ok">${h(data.status)}</span>` : "",
      `<span class="pill">${data.episode_count} episode</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const firstEp = eps[0];
    const actions = firstEp
      ? `<a class="btn primary" href="#/watch/${encodeURIComponent(firstEp.id)}">▶ Tonton Episode ${firstEp.number}</a>`
      : "";
    const epList = eps.length
      ? `<div class="ep-list">${eps.map(e=>`<button class="ep-btn center" data-go="#/watch/${encodeURIComponent(e.id)}">Eps ${e.number}</button>`).join("")}</div>`
      : `<div class="empty">Belum ada episode.</div>`;
    setD(
      heroHtml("donghua","Donghua",data,facts,actions,data.synopsis,data.cover) +
      `<div class="row-head"><h2>Episode</h2></div>${epList}`
    );
  }

  // ---- Manga / Novel series (readable, paginated chapters) ----------------
  async function renderReadableSeries(kind, id, data, page) {
    const label = kind === "manga" ? "Manga" : "Novel";
    const chs = data.chapters || [];
    const totalPages = data.chapter_total_pages || 1;
    const facts = [
      data.status ? `<span class="pill ok">${h(data.status)}</span>` : "",
      data.author ? `<span class="pill">✍ ${h(data.author)}</span>` : "",
      data.rating ? `<span class="pill">★ ${h(data.rating)}</span>` : "",
      `<span class="pill">${data.chapter_count} bab</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = chs[0];
    const readPath = kind === "manga" ? "read/manga" : "read/novel";
    const actions = first
      ? `<a class="btn primary" href="#/${readPath}/${encodeURIComponent(first.id)}">📖 Mulai Baca</a>`
      : "";
    const syn = data.description || data.synopsis;
    const chList = chs.length
      ? `<div class="ep-list wide">${chs.map(c=>`
          <button class="ep-btn" data-go="#/${readPath}/${encodeURIComponent(c.id)}">
            <span>Bab ${typeof c.number==="number" ? (Number.isInteger(c.number)?c.number:c.number) : c.number}${c.title?` · ${h(c.title)}`:""}</span>
          </button>`).join("")}</div>`
      : `<div class="empty">Belum ada bab.</div>`;
    const pager = totalPages > 1 ? `
      <div class="pager">
        ${page>1?`<button class="btn sm" id="ch-prev">← Bab sebelumnya</button>`:""}
        <span>Halaman ${page} / ${totalPages}</span>
        ${page<totalPages?`<button class="btn sm" id="ch-next">Bab berikutnya →</button>`:""}
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

  // ---- Cosplay post -------------------------------------------------------
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
    const actions = dls + (data.unzip_password?`<span class="pill">🔑 ${h(data.unzip_password)}</span>`:"");
    const imgs = (data.images||[]).map(u=>`<a href="${h(u)}" target="_blank" rel="noopener">${imgTag(u,"","")}</a>`).join("");
    setD(
      heroHtml("cosplay","Cosplay",data,facts,actions,null,data.cover) +
      `<div class="row-head"><h2>${(data.images||[]).length} Foto</h2></div>
       <div class="gallery">${imgs}</div>`
    );
  }

  // ---- Doujin (nhentai) — gallery as a single chapter ---------------------
  async function renderDoujin(id) {
    const data = await api(`/nhentai/${encodeURIComponent(id)}`);
    const facts = [
      data.author ? `<span class="pill">${h(data.author)}</span>` : "",
      ...(data.genres||[]).slice(0,6).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = (data.chapters||[])[0];
    const actions = first
      ? `<a class="btn primary" href="#/read/nhentai/${encodeURIComponent(first.id)}">📖 Baca</a>`
      : "";
    setD(heroHtml("doujin","Doujin",data,facts,actions,data.description,data.cover));
  }

  // ---- Watch (donghua episode) -------------------------------------------
  A.routeWatch = async function routeWatch(id) {
    shell(`<div id="d">${spinner}</div>`);
    try {
      const e = await api(`/donghua/episode/${encodeURIComponent(id)}`);
      const servers = e.servers || [];
      const seriesLink = e.series_id ? `#/detail/donghua/${encodeURIComponent(e.series_id)}` : "#/";
      const player = servers.length
        ? `<div class="player-wrap"><div class="frame"><iframe id="player" src="${h(servers[0].embed_url)}" allowfullscreen allow="autoplay; encrypted-media; picture-in-picture"></iframe></div></div>`
        : `<div class="empty">Tidak ada server video.</div>`;
      const bar = servers.length
        ? `<div class="server-bar">${servers.map((s,i)=>`<button class="srv ${i===0?"active":""}" data-src="${h(s.embed_url)}">${h(s.label)}${s.format?` · ${h(s.format)}`:""}</button>`).join("")}</div>`
        : "";
      const dls = (e.downloads||[]).map(g=>`
        <div class="dl-group"><div class="q">${h(g.quality)}</div>
          <div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div>
        </div>`).join("");
      const navBtns = `
        <div class="server-bar" style="margin-top:8px">
          ${e.prev_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.prev_id)}">← Eps sebelumnya</a>`:""}
          <a class="btn sm" href="${seriesLink}">☰ Semua episode</a>
          ${e.next_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.next_id)}">Eps berikutnya →</a>`:""}
        </div>`;
      setView(
        crumbs([{href:"#/",label:"Home"},{href:"#/browse/donghua",label:"Donghua"},{label:`${e.series_title||"Episode"} · Eps ${e.episode_number}`}]) +
        `<div class="row-head"><h2>${h(e.series_title||"Episode")} — Episode ${e.episode_number}</h2></div>` +
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
  };

  // ---- Read (manga pages / novel text / doujin pages) --------------------
  A.routeRead = async function routeRead(kind, id) {
    shell(`<div id="d">${spinner}</div>`);
    try {
      if (kind === "novel") return renderNovelChapter(id);
      // manga + nhentai both use the chapter endpoint with pages[]
      const ep = kind === "nhentai" ? "nhentai/chapter" : "manga/chapter";
      const c = await api(`/${ep}/${encodeURIComponent(id)}`);
      const pages = c.pages || [];
      const imgs = pages.map(p=>`<img loading="lazy" referrerpolicy="no-referrer" src="${h(p.url)}" alt="page ${p.index}"
        onerror="this.style.opacity=.25">`).join("");
      setView(
        `<div class="row-head"><h2>${h(c.series_title||"Baca")} ${c.chapter_number?`· Ch ${c.chapter_number}`:""}</h2></div>` +
        `<div class="reader">${pages.length?imgs:`<div class="empty">Tidak ada halaman.</div>`}</div>`
      );
    } catch (e) {
      setView(`<div class="errbox">${h(e.message)}</div>`);
    }
  };

  async function renderNovelChapter(id) {
    const c = await api(`/novel/chapter/${encodeURIComponent(id)}`);
    const paras = (c.body || "").split(/\n{2,}/).map(s=>s.trim()).filter(Boolean)
      .map(p=>`<p>${h(p)}</p>`).join("");
    const nav = `
      <div class="reader-nav">
        ${c.prev_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.prev_id)}">← Sebelumnya</a>`:""}
        ${c.series_id?`<a class="btn sm" href="#/detail/novel/${encodeURIComponent(c.series_id)}">☰ Daftar bab</a>`:""}
        ${c.next_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.next_id)}">Berikutnya →</a>`:""}
      </div>`;
    setView(
      `<div class="row-head"><h2>${h(c.series_title||"Novel")} · Bab ${c.chapter_number}</h2></div>` +
      (c.chapter_title?`<p style="color:var(--muted);margin-top:-8px">${h(c.chapter_title)}</p>`:"") +
      `<div class="novel-body">${paras||"<p>(kosong)</p>"}</div>` + nav
    );
  }
})();
