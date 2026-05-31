// apiku consumer SPA — dependency-free hash router.
// Home / browse / search / detail / watch / read / gallery / docs / explorer.
(function () {
  "use strict";

  const API = "/api/v1";
  const app = document.getElementById("app");
  const CHAPTER_SIZE = 60;

  // ---- SVG icons ----------------------------------------------------------
  const I = {
    home:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/></svg>',
    donghua:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="14" rx="3"/><path d="m10 9 5 3-5 3z" fill="currentColor"/><path d="M8 21h8"/></svg>',
    manga:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 5a2 2 0 0 1 2-2h6v18H5a2 2 0 0 1-2-2z"/><path d="M21 5a2 2 0 0 0-2-2h-6v18h6a2 2 0 0 0 2-2z"/></svg>',
    novel:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h13a3 3 0 0 1 3 3v13H7a3 3 0 0 1-3-3z"/><path d="M8 8h8M8 12h8M8 16h5"/></svg>',
    cosplay:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="8" r="4"/><path d="M4 21c0-4 4-6 8-6s8 2 8 6"/></svg>',
    doujin:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 21s-7-4.5-9.5-9A5.5 5.5 0 0 1 12 6a5.5 5.5 0 0 1 9.5 6c-2.5 4.5-9.5 9-9.5 9z"/></svg>',
    docs:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3v5h5"/><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M9 13h6M9 17h6"/></svg>',
    explorer:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m8 16 2-6 6-2-2 6z"/><circle cx="12" cy="12" r="9"/></svg>',
    search:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>',
    sun:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
    moon:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>',
    menu:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M3 6h18M3 12h18M3 18h18"/></svg>',
    close:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>',
    play:'<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>',
    book:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19V5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z"/><path d="M6 17h13"/></svg>',
    arrow:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>',
  };

  // ---- Provider config ---------------------------------------------------
  const PROVIDERS = {
    donghua: { label: "Donghua", api: "anichin",     kind: "donghua", adult: false, icon: I.donghua },
    manga:   { label: "Manga",   api: "mangaball",   kind: "manga",   adult: false, icon: I.manga },
    novel:   { label: "Novel",   api: "novelid",     kind: "novel",   adult: false, icon: I.novel },
    cosplay: { label: "Cosplay", api: "cosplaytele", kind: "cosplay", adult: true,  icon: I.cosplay },
    doujin:  { label: "Doujin",  api: "nhentai",     kind: "doujin",  adult: true,  icon: I.doujin },
  };

  const FEEDS = {
    anichin:     [["home","Terbaru"],["popular","Populer"],["rating","Rating"],["title","A-Z"]],
    mangaball:   [["home","Unggulan"],["popular","Populer"],["latest","Terbaru"],["recommend","Rekomendasi"]],
    novelid:     [["home","Semua"],["popular","Tamat"],["novel-translate","Translate"],["fantasi","Fantasi"],["romantis","Romantis"],["aksi","Aksi"],["horror","Horror"]],
    cosplaytele: [["home","Terbaru"],["popular","Populer"]],
    nhentai:     [["popular-today","Hari Ini"],["popular-week","Minggu Ini"],["popular","Sepanjang Masa"],["home","Terbaru"]],
  };

  const DETAIL_EP = { donghua:"donghua", manga:"manga", novel:"novel", cosplay:"cosplay", doujin:"nhentai" };

  // ---- Preferences --------------------------------------------------------
  const store = {
    get theme(){ return localStorage.getItem("apiku.theme") || "dark"; },
    set theme(v){ localStorage.setItem("apiku.theme", v); },
    get adult(){ return localStorage.getItem("apiku.adult") === "1"; },
    set adult(v){ localStorage.setItem("apiku.adult", v ? "1" : "0"); },
  };
  const applyTheme = () => document.documentElement.setAttribute("data-theme", store.theme);
  applyTheme();
  const adultOn = () => store.adult;
  function providerVisible(kind){ const p = Object.values(PROVIDERS).find(x=>x.kind===kind); return p ? (!p.adult || adultOn()) : true; }

  // ---- Helpers ------------------------------------------------------------
  const h = (s) => (s==null?"":String(s).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;").replace(/'/g,"&#39;"));
  const qs = (o) => Object.entries(o).filter(([,v])=>v!=null&&v!=="").map(([k,v])=>`${encodeURIComponent(k)}=${encodeURIComponent(v)}`).join("&");

  async function api(path){ const r=await fetch(API+path); const j=await r.json(); if(!j.ok) throw new Error(j.error?`${j.error.code}: ${j.error.message}`:"request failed"); return j.data; }
  async function apiRaw(method, rel){ const url=API+rel; const t0=performance.now(); const r=await fetch(url,{method}); const text=await r.text(); let json; try{json=JSON.parse(text);}catch{json=text;} return {status:r.status, ms:Math.round(performance.now()-t0), json}; }

  const go = (hash) => { location.hash = hash; };
  const viewEl = () => document.getElementById("view");
  const setView = (html) => { const v=viewEl(); if(v) v.innerHTML=html; };
  const spinner = `<div class="spinner"></div>`;
  const skelGrid = (n) => `<div class="grid">${Array.from({length:n||12}).map(()=>`<div class="skeleton poster"></div>`).join("")}</div>`;

  function imgTag(url, _cls, alt){
    if(!url) return `<div class="ph">${h(alt||"no image")}</div>`;
    return `<img loading="lazy" referrerpolicy="no-referrer" src="${h(url)}" alt="${h(alt||"")}" onerror="this.parentNode.innerHTML='<div class=ph>no image</div>'">`;
  }

  // ---- Shell --------------------------------------------------------------
  function navLinks(){
    const items = [
      ["home", "#/", "Home", I.home],
      ["donghua", "#/browse/donghua", "Donghua", I.donghua],
      ["manga", "#/browse/manga", "Manga", I.manga],
      ["novel", "#/browse/novel", "Novel", I.novel],
    ];
    if (adultOn()) {
      items.push(["cosplay", "#/browse/cosplay", "Cosplay", I.cosplay]);
      items.push(["doujin", "#/browse/doujin", "Doujin", I.doujin]);
    }
    items.push(["docs", "#/docs", "API Docs", I.docs]);
    items.push(["explorer", "#/explorer", "Explorer", I.explorer]);
    return items;
  }

  function shell(inner){
    const seg = (location.hash.replace(/^#\//,"").split("/")[0]) || "home";
    const links = navLinks();
    const themeIco = store.theme === "dark" ? I.sun : I.moon;

    app.innerHTML = `
      <div class="drawer-scrim" id="scrim"></div>
      <aside class="drawer" id="drawer">
        <div class="dhead">
          <span class="brand"><span class="logo">&#128250;</span><b>apiku</b></span>
          <button class="icon-btn" id="drawerClose">${I.close}</button>
        </div>
        <nav>
          ${links.map(([s,href,label,ico])=>`<a data-seg="${s}" href="${href}" class="${s===seg?"active":""}">${ico}<span>${label}</span></a>`).join("")}
        </nav>
        <div class="dsep"></div>
        <div class="drow">
          <button class="icon-btn ${adultOn()?"on":""}" id="adultBtnD">18+ ${adultOn()?"ON":"OFF"}</button>
          <button class="icon-btn" id="themeBtnD">${themeIco}<span>Tema</span></button>
        </div>
      </aside>

      <header class="hdr">
        <button class="icon-btn hamburger" id="hamburger">${I.menu}</button>
        <a class="brand" href="#/"><span class="logo">&#128250;</span><b>apiku</b></a>
        <nav class="desktop">
          ${links.map(([s,href,label,ico])=>`<a data-seg="${s}" href="${href}" class="${s===seg?"active":""}">${ico}<span>${label}</span></a>`).join("")}
        </nav>
        <div class="spacer"></div>
        <form class="searchbox" id="searchform">
          ${I.search}
          <input id="searchinput" type="search" placeholder="Cari judul..." autocomplete="off">
        </form>
        <button class="icon-btn ${adultOn()?"on":""}" id="adultBtn" title="Konten 18+">18+</button>
        <button class="icon-btn" id="themeBtn" title="Ganti tema">${themeIco}</button>
      </header>
      <main id="view">${inner}</main>
      <footer>
        Ditenagai <a href="https://github.com/risqinf/apiku" target="_blank" rel="noopener">apiku</a> &middot;
        <a href="#/docs">API Docs</a> &middot; <a href="#/explorer">Explorer</a> &middot;
        <a href="/tester">Dev console</a><br>
        Konten berasal dari sumber pihak ketiga.
      </footer>`;

    // search
    const form = document.getElementById("searchform");
    const input = document.getElementById("searchinput");
    form.addEventListener("submit", (e)=>{ e.preventDefault(); const q=input.value.trim(); if(q) go(`#/search/${encodeURIComponent(q)}`); });
    const m = location.hash.match(/^#\/search\/([^/]+)/);
    if (m) input.value = decodeURIComponent(m[1]);

    // theme
    const toggleTheme = ()=>{ store.theme = store.theme==="dark"?"light":"dark"; applyTheme(); window.__apiku.router(); };
    document.getElementById("themeBtn").onclick = toggleTheme;
    document.getElementById("themeBtnD").onclick = toggleTheme;

    // adult
    const toggleAdult = ()=>{
      if(!adultOn()){ if(!confirm("Aktifkan konten 18+? Hanya untuk pengguna dewasa (18+).")) return; }
      store.adult = !store.adult;
      const sub = (location.hash.replace(/^#\//,"").split("/")[1]) || "";
      if(!adultOn() && (sub==="cosplay"||sub==="doujin")){ go("#/"); return; }
      window.__apiku.router();
    };
    document.getElementById("adultBtn").onclick = toggleAdult;
    document.getElementById("adultBtnD").onclick = toggleAdult;

    // drawer
    const drawer = document.getElementById("drawer");
    const scrim = document.getElementById("scrim");
    const openDrawer = ()=>{ drawer.classList.add("open"); scrim.classList.add("open"); };
    const closeDrawer = ()=>{ drawer.classList.remove("open"); scrim.classList.remove("open"); };
    document.getElementById("hamburger").onclick = openDrawer;
    document.getElementById("drawerClose").onclick = closeDrawer;
    scrim.onclick = closeDrawer;
    drawer.querySelectorAll("nav a").forEach(a => a.addEventListener("click", closeDrawer));
  }

  // ---- Cards --------------------------------------------------------------
  function cardHtml(item){
    const prov = Object.values(PROVIDERS).find(p=>p.kind===item.kind) || {};
    const tags = (item.tags||[]).slice(0,2).map(t=>`<span>${h(t)}</span>`).join("");
    return `
      <div class="card" data-go="#/detail/${encodeURIComponent(item.kind)}/${encodeURIComponent(item.id)}">
        <div class="poster">${imgTag(item.thumbnail,"",item.title)}<span class="badge src">${h(prov.label||item.source)}</span></div>
        <div class="meta"><div class="t">${h(item.title)}</div><div class="sub">${tags}</div></div>
      </div>`;
  }
  const grid = (items) => (!items||!items.length) ? `<div class="empty">Tidak ada hasil.</div>` : `<div class="grid">${items.map(cardHtml).join("")}</div>`;

  document.addEventListener("click", (e)=>{ const el=e.target.closest("[data-go]"); if(el){ e.preventDefault(); go(el.dataset.go); } });

  function crumbs(items){
    return `<div class="crumbs">`+items.map((it,i)=> i<items.length-1?`<a href="${it.href}">${h(it.label)}</a><span>/</span>`:`<b>${h(it.label)}</b>`).join("")+`</div>`;
  }

  // ===========================================================================
  // Home / Browse / Search
  // ===========================================================================
  async function routeHome(){
    shell(`
      <div class="hero-banner">
        <h1>Nonton &amp; Baca, satu tempat.</h1>
        <p>Streaming donghua, baca manga &amp; novel, galeri cosplay - ditenagai apiku.</p>
      </div>
      <div id="rows"></div>
    `);
    const rows = document.getElementById("rows");
    let sections = [
      { title:"Donghua Terbaru", prov:"anichin",     feed:"home",          seg:"donghua" },
      { title:"Manga Populer",   prov:"mangaball",   feed:"popular",       seg:"manga" },
      { title:"Novel Terbaru",   prov:"novelid",     feed:"home",          seg:"novel" },
      { title:"Cosplay Terbaru", prov:"cosplaytele", feed:"home",          seg:"cosplay", adult:true },
      { title:"Doujin Hari Ini", prov:"nhentai",     feed:"popular-today", seg:"doujin",  adult:true },
    ].filter(s=>!s.adult||adultOn());
    rows.innerHTML = sections.map((s,i)=>`
      <div class="row-head"><h2><span class="dot"></span>${h(s.title)}</h2><a class="more" href="#/browse/${s.seg}">Lihat semua ${I.arrow}</a></div>
      <div id="row-${i}">${skelGrid(6)}</div>`).join("");
    sections.forEach(async (s,i)=>{
      try { const data=await api(`/browse/${s.prov}?${qs({feed:s.feed})}`); document.getElementById(`row-${i}`).innerHTML=grid((data.items||[]).slice(0,12)); }
      catch(e){ const el=document.getElementById(`row-${i}`); if(el) el.innerHTML=`<div class="errbox">Gagal memuat.</div>`; }
    });
  }

  async function routeBrowse(seg, feed, page){
    const prov = PROVIDERS[seg];
    if(!prov) return routeHome();
    if(prov.adult && !adultOn()) return routeHome();
    page = parseInt(page||"1",10);
    const feeds = FEEDS[prov.api] || [["home","Semua"]];
    feed = feed || feeds[0][0];
    shell(`
      <div class="row-head"><h2><span class="dot"></span>${h(prov.label)}</h2></div>
      <div class="chips">${feeds.map(([v,l])=>`<a class="chip ${v===feed?"active":""}" href="#/browse/${seg}/${v}">${h(l)}</a>`).join("")}</div>
      <div id="list">${skelGrid(18)}</div>
      <div id="pager"></div>
    `);
    try{
      const data = await api(`/browse/${prov.api}?${qs({feed,page})}`);
      document.getElementById("list").innerHTML = grid(data.items);
      document.getElementById("pager").innerHTML = `
        <div class="pager">
          ${page>1?`<a class="btn sm" href="#/browse/${seg}/${feed}/${page-1}">&larr; Sebelumnya</a>`:""}
          <span>Halaman ${page}</span>
          ${(data.items&&data.items.length)?`<a class="btn sm" href="#/browse/${seg}/${feed}/${page+1}">Berikutnya &rarr;</a>`:""}
        </div>`;
    }catch(e){ document.getElementById("list").innerHTML=`<div class="errbox">${h(e.message)}</div>`; }
  }

  // Search with a source filter (grouped, not all-at-once clutter)
  async function routeSearch(query, src){
    src = src || "all";
    shell(`
      <div class="row-head"><h2><span class="dot"></span>Hasil: &ldquo;${h(query)}&rdquo;</h2></div>
      <div class="chips" id="srcChips"></div>
      <div id="list">${skelGrid(12)}</div>
    `);
    // Build the source filter chips (hide adult unless enabled)
    const sources = [["all","Semua"],["donghua","Donghua"],["manga","Manga"],["novel","Novel"]];
    if(adultOn()){ sources.push(["cosplay","Cosplay"]); sources.push(["doujin","Doujin"]); }
    try{
      const data = await api(`/search?${qs({q:query, source:"all", page:1})}`);
      let items = (data.items||[]).filter(it=>providerVisible(it.kind));
      // counts per kind
      const counts = {};
      items.forEach(it=>{ counts[it.kind]=(counts[it.kind]||0)+1; });
      const chips = document.getElementById("srcChips");
      chips.innerHTML = sources.map(([v,l])=>{
        const c = v==="all" ? items.length : (counts[v]||0);
        return `<button class="chip ${v===src?"active":""}" data-src="${v}">${h(l)} <span class="cnt">${c}</span></button>`;
      }).join("");
      const render = () => {
        const filtered = src==="all" ? items : items.filter(it=>it.kind===src);
        document.getElementById("list").innerHTML = grid(filtered);
      };
      chips.querySelectorAll(".chip").forEach(ch => ch.addEventListener("click", ()=>{
        src = ch.dataset.src;
        chips.querySelectorAll(".chip").forEach(c=>c.classList.toggle("active", c.dataset.src===src));
        // reflect in URL without reloading
        history.replaceState(null,"",`#/search/${encodeURIComponent(query)}/${src}`);
        render();
      }));
      render();
    }catch(e){ document.getElementById("list").innerHTML=`<div class="errbox">${h(e.message)}</div>`; }
  }

  // ===========================================================================
  // Detail / watch / read
  // ===========================================================================
  const setD = (html) => { const el=document.getElementById("d"); if(el) el.innerHTML=html; };

  function heroHtml(kind, label, data, facts, actions, syn, cover){
    return `
      ${crumbs([{href:"#/",label:"Home"},{href:`#/browse/${kind}`,label},{label:data.title||"Detail"}])}
      <div class="detail-hero">
        <div class="poster">${imgTag(cover,"",data.title)}</div>
        <div class="info">
          <h1>${h(data.title)}</h1>
          <div class="facts">${facts}</div>
          ${syn?`<p class="syn">${h(syn)}</p>`:""}
          <div class="actions">${actions}</div>
        </div>
      </div>`;
  }

  async function routeDetail(kind, id){
    shell(`<div id="d">${spinner}</div>`);
    const ep = DETAIL_EP[kind];
    if(!ep) return setD(`<div class="errbox">Tipe tidak dikenal: ${h(kind)}</div>`);
    try{
      if(kind==="cosplay") return renderCosplay(id);
      if(kind==="doujin") return renderDoujin(id);
      const data = await api(`/${ep}/${encodeURIComponent(id)}?${qs({page:1,size:CHAPTER_SIZE})}`);
      if(kind==="donghua") return renderDonghuaSeries(id, data);
      return renderReadableSeries(kind, id, data, 1);
    }catch(e){ setD(`<div class="errbox">${h(e.message)}</div>`); }
  }

  function renderDonghuaSeries(id, data){
    const eps = data.episodes||[];
    const facts = [
      data.status?`<span class="pill ok">${h(data.status)}</span>`:"",
      `<span class="pill">${data.episode_count} episode</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = eps[0];
    const actions = first?`<a class="btn primary" href="#/watch/${encodeURIComponent(first.id)}">${I.play} Tonton Eps ${first.number}</a>`:"";
    const list = eps.length?`<div class="ep-list">${eps.map(e=>`<button class="ep-btn center" data-go="#/watch/${encodeURIComponent(e.id)}">Eps ${e.number}</button>`).join("")}</div>`:`<div class="empty">Belum ada episode.</div>`;
    setD(heroHtml("donghua","Donghua",data,facts,actions,data.synopsis,data.cover)+`<div class="row-head"><h2><span class="dot"></span>Episode</h2></div>${list}`);
  }

  // Manga/novel — with LANGUAGE GROUPING for manga (translations).
  async function renderReadableSeries(kind, id, data, page){
    const label = kind==="manga"?"Manga":"Novel";
    const chs = data.chapters||[];
    const totalPages = data.chapter_total_pages||1;
    const facts = [
      data.status?`<span class="pill ok">${h(data.status)}</span>`:"",
      data.author?`<span class="pill">&#9997; ${h(data.author)}</span>`:"",
      data.rating?`<span class="pill">&#9733; ${h(data.rating)}</span>`:"",
      `<span class="pill">${data.chapter_count} bab</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const readPath = kind==="manga"?"read/manga":"read/novel";

    // --- language detection (manga translations) ---
    let languages = [];
    if (kind === "manga") {
      const set = new Map(); // lang -> count
      chs.forEach(c => {
        const tr = c.translations || [];
        if (tr.length) tr.forEach(t => { const l = t.language || "Lainnya"; set.set(l, (set.get(l)||0)+1); });
      });
      languages = [...set.entries()].sort((a,b)=>b[1]-a[1]); // [lang, count]
    }
    const langState = { active: "__all__" };

    const first = chs[0];
    const firstReadId = (() => {
      if (kind !== "manga" || !first) return first ? first.id : null;
      // prefer a translation in the active language, else the chapter's own id
      return first.id;
    })();
    const actions = firstReadId?`<a class="btn primary" href="#/${readPath}/${encodeURIComponent(firstReadId)}">${I.book} Mulai Baca</a>`:"";
    const syn = data.description || data.synopsis;

    const langTabs = (kind==="manga" && languages.length>1)
      ? `<div class="lang-tabs" id="langTabs">
          <button class="lang-tab active" data-lang="__all__">Semua <span class="cnt">${chs.length}</span></button>
          ${languages.map(([l,c])=>`<button class="lang-tab" data-lang="${h(l)}">${h(l)} <span class="cnt">${c}</span></button>`).join("")}
         </div>`
      : "";

    function chapterRowsFor(lang){
      return chs.map(c=>{
        if (kind === "manga" && lang !== "__all__") {
          const tr = (c.translations||[]).filter(t => (t.language||"Lainnya") === lang);
          if (!tr.length) return ""; // hide chapters lacking this language
          // link to the translation in that language
          const t = tr[0];
          const grp = t.group ? ` &middot; ${h(t.group)}` : "";
          return `<button class="ep-btn" data-go="#/${readPath}/${encodeURIComponent(t.id)}">
            <span>Bab ${h(c.number)}${c.title?` &middot; ${h(c.title)}`:""}</span>
            <span class="tag">${h(lang)}${grp}</span></button>`;
        }
        // "all" view (or novel): one row per chapter, show language count if any
        const langCount = (c.translations||[]).length;
        const tag = (kind==="manga" && langCount>1) ? `<span class="tag">${langCount} bahasa</span>` : "";
        return `<button class="ep-btn" data-go="#/${readPath}/${encodeURIComponent(c.id)}">
          <span>Bab ${h(c.number)}${c.title?` &middot; ${h(c.title)}`:""}</span>${tag}</button>`;
      }).join("");
    }

    const pager = totalPages>1?`
      <div class="pager">
        ${page>1?`<button class="btn sm" id="ch-prev">&larr; Bab sebelumnya</button>`:""}
        <span>Halaman ${page} / ${totalPages}</span>
        ${page<totalPages?`<button class="btn sm" id="ch-next">Bab berikutnya &rarr;</button>`:""}
      </div>`:"";

    setD(
      heroHtml(kind,label,data,facts,actions,syn,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>Daftar Bab</h2></div>${langTabs}${pager}
       <div class="ep-list wide" id="chList">${chs.length?chapterRowsFor(langState.active):`<div class="empty">Belum ada bab.</div>`}</div>${pager}`
    );

    // wire language tabs
    const tabsEl = document.getElementById("langTabs");
    if (tabsEl) {
      tabsEl.querySelectorAll(".lang-tab").forEach(tab => tab.addEventListener("click", ()=>{
        langState.active = tab.dataset.lang;
        tabsEl.querySelectorAll(".lang-tab").forEach(t=>t.classList.toggle("active", t===tab));
        document.getElementById("chList").innerHTML = chapterRowsFor(langState.active) || `<div class="empty">Tidak ada bab untuk bahasa ini.</div>`;
      }));
    }

    // wire chapter pager
    const ep = DETAIL_EP[kind];
    const load = async (p)=>{
      document.querySelectorAll("#d .ep-list").forEach(n=>n.innerHTML=`<div class="spinner"></div>`);
      const fresh = await api(`/${ep}/${encodeURIComponent(id)}?${qs({page:p,size:CHAPTER_SIZE})}`);
      renderReadableSeries(kind, id, fresh, p);
    };
    const pv=document.getElementById("ch-prev"); if(pv) pv.onclick=()=>load(page-1);
    const nx=document.getElementById("ch-next"); if(nx) nx.onclick=()=>load(page+1);
  }

  async function renderCosplay(id){
    const data = await api(`/cosplay/${encodeURIComponent(id)}`);
    const facts = [
      data.cosplayer?`<span class="pill">${h(data.cosplayer)}</span>`:"",
      data.character?`<span class="pill">${h(data.character)}</span>`:"",
      data.series?`<span class="pill">${h(data.series)}</span>`:"",
      data.photo_count?`<span class="pill">${data.photo_count} foto</span>`:"",
      ...(data.tags||[]).slice(0,4).map(t=>`<span class="pill">${h(t)}</span>`),
    ].join("");
    const dls = (data.downloads||[]).map(d=>`<a class="btn sm" target="_blank" rel="noopener" href="${h(d.url)}">${h(d.name)}</a>`).join("");
    const actions = dls + (data.unzip_password?`<span class="pill">&#128273; ${h(data.unzip_password)}</span>`:"");
    const imgs = (data.images||[]).map(u=>`<a href="${h(u)}" target="_blank" rel="noopener">${imgTag(u,"","")}</a>`).join("");
    setD(heroHtml("cosplay","Cosplay",data,facts,actions,null,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>${(data.images||[]).length} Foto</h2></div><div class="gallery">${imgs}</div>`);
  }

  async function renderDoujin(id){
    const data = await api(`/nhentai/${encodeURIComponent(id)}`);
    const facts = [ data.author?`<span class="pill">${h(data.author)}</span>`:"", ...(data.genres||[]).slice(0,6).map(g=>`<span class="pill">${h(g)}</span>`) ].join("");
    const first = (data.chapters||[])[0];
    const actions = first?`<a class="btn primary" href="#/read/nhentai/${encodeURIComponent(first.id)}">${I.book} Baca</a>`:"";
    setD(heroHtml("doujin","Doujin",data,facts,actions,data.description,data.cover));
  }

  async function routeWatch(id){
    shell(`<div id="d">${spinner}</div>`);
    try{
      const e = await api(`/donghua/episode/${encodeURIComponent(id)}`);
      const servers = e.servers||[];
      const seriesLink = e.series_id?`#/detail/donghua/${encodeURIComponent(e.series_id)}`:"#/";
      const player = servers.length?`<div class="player-wrap"><div class="frame"><iframe id="player" src="${h(servers[0].embed_url)}" allowfullscreen allow="autoplay; encrypted-media; picture-in-picture"></iframe></div></div>`:`<div class="empty">Tidak ada server video.</div>`;
      const bar = servers.length?`<div class="server-bar"><span class="lbl">Server:</span>${servers.map((s,i)=>`<button class="srv ${i===0?"active":""}" data-src="${h(s.embed_url)}">${h(s.label)}${s.format?` &middot; ${h(s.format)}`:""}</button>`).join("")}</div>`:"";
      const dls = (e.downloads||[]).map(g=>`<div class="dl-group"><div class="q">${h(g.quality)}</div><div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div></div>`).join("");
      const nav = `<div class="server-bar" style="margin-top:8px">
        ${e.prev_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.prev_id)}">&larr; Eps sebelumnya</a>`:""}
        <a class="btn sm" href="${seriesLink}">&#9776; Semua episode</a>
        ${e.next_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.next_id)}">Eps berikutnya &rarr;</a>`:""}</div>`;
      setView(
        crumbs([{href:"#/",label:"Home"},{href:"#/browse/donghua",label:"Donghua"},{label:`${e.series_title||"Episode"} - Eps ${e.episode_number}`}])+
        `<div class="row-head"><h2><span class="dot"></span>${h(e.series_title||"Episode")} - Episode ${e.episode_number}</h2></div>`+
        player+bar+nav+(dls?`<div class="row-head"><h2><span class="dot"></span>Unduh</h2></div>${dls}`:"")
      );
      document.querySelectorAll(".server-bar .srv").forEach(btn=>{ btn.onclick=()=>{ document.getElementById("player").src=btn.dataset.src; document.querySelectorAll(".server-bar .srv").forEach(b=>b.classList.remove("active")); btn.classList.add("active"); }; });
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  async function routeRead(kind, id){
    shell(`<div id="d">${spinner}</div>`);
    try{
      if(kind==="novel") return renderNovelChapter(id);
      const ep = kind==="nhentai"?"nhentai/chapter":"manga/chapter";
      const c = await api(`/${ep}/${encodeURIComponent(id)}`);
      const pages = c.pages||[];
      const imgs = pages.map(p=>`<img loading="lazy" referrerpolicy="no-referrer" src="${h(p.url)}" alt="page ${p.index}" onerror="this.style.opacity=.25">`).join("");
      setView(`<div class="row-head"><h2><span class="dot"></span>${h(c.series_title||"Baca")} ${c.chapter_number?`&middot; Ch ${c.chapter_number}`:""}</h2></div><div class="reader">${pages.length?imgs:`<div class="empty">Tidak ada halaman.</div>`}</div>`);
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  async function renderNovelChapter(id){
    const c = await api(`/novel/chapter/${encodeURIComponent(id)}`);
    const paras = (c.body||"").split(/\n{2,}/).map(s=>s.trim()).filter(Boolean).map(p=>`<p>${h(p)}</p>`).join("");
    const nav = `<div class="reader-nav">
      ${c.prev_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.prev_id)}">&larr; Sebelumnya</a>`:""}
      ${c.series_id?`<a class="btn sm" href="#/detail/novel/${encodeURIComponent(c.series_id)}">&#9776; Daftar bab</a>`:""}
      ${c.next_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.next_id)}">Berikutnya &rarr;</a>`:""}</div>`;
    setView(`<div class="row-head"><h2><span class="dot"></span>${h(c.series_title||"Novel")} &middot; Bab ${c.chapter_number}</h2></div>`+
      (c.chapter_title?`<p style="color:var(--muted);margin-top:-8px">${h(c.chapter_title)}</p>`:"")+
      `<div class="novel-body">${paras||"<p>(kosong)</p>"}</div>`+nav);
  }

  // route dispatch is defined in part 2 (appended)
  window.__apiku = { shell, setView, viewEl, h, qs, api, apiRaw, spinner, go, I,
    routeHome, routeBrowse, routeSearch, routeDetail, routeWatch, routeRead };
})();

// ===========================================================================
// Docs + Explorer + Router
// ===========================================================================
(function () {
  "use strict";
  const A = window.__apiku;
  const { shell, setView, h, apiRaw, I, go,
    routeHome, routeBrowse, routeSearch, routeDetail, routeWatch, routeRead } = A;

  // ---- API Docs -----------------------------------------------------------
  const ENDPOINTS = [
    ["GET", "/api/v1/health", "Liveness probe"],
    ["GET", "/api/v1/info", "Server info, providers, endpoints"],
    ["GET", "/api/v1/search?q={query}&source={all|donghua|manga|novel|cosplay|doujin}&page={n}", "Cross-provider search"],
    ["GET", "/api/v1/browse/{provider}?feed={feed}&page={n}", "Home / popular / latest feed"],
    ["GET", "/api/v1/manga/{id}?page={n}&size={N}", "Manga series (paginated chapters)"],
    ["GET", "/api/v1/manga/chapter/{id}", "Manga chapter pages"],
    ["GET", "/api/v1/donghua/{id}?page={n}&size={N}", "Donghua series (paginated episodes)"],
    ["GET", "/api/v1/donghua/episode/{id}", "Donghua episode + servers + downloads"],
    ["GET", "/api/v1/novel/{id}?page={n}&size={N}", "Novel series (paginated chapters)"],
    ["GET", "/api/v1/novel/chapter/{id}", "Novel chapter (text body)"],
    ["GET", "/api/v1/cosplay/{id}", "Cosplay post (gallery + downloads)"],
    ["GET", "/api/v1/nhentai/{id}", "Doujin gallery"],
    ["GET", "/api/v1/nhentai/chapter/{id}", "Doujin pages"],
    ["GET", "/img?p={payload}&s={signature}", "HMAC-signed image proxy"],
  ];

  function codeSamples(origin) {
    const url = `${origin}/api/v1/search?q=one+piece&source=manga`;
    return {
      curl:
`# Cari manga "one piece"
curl '${url}'

# Pretty-print dengan jq
curl '${url}' | jq .`,
      javascript:
`// Browser / Node 18+
const res = await fetch('${origin}/api/v1/search?q=one piece&source=manga');
const json = await res.json();
if (!json.ok) throw new Error(json.error.code + ': ' + json.error.message);
console.log(\`\${json.data.total} hasil (\${json.meta.took_ms}ms)\`);
for (const it of json.data.items) console.log(it.source, it.title, it.id);`,
      python:
`import requests
BASE = '${origin}'

def api_get(path, **params):
    r = requests.get(BASE + path, params=params)
    r.raise_for_status()
    body = r.json()
    if not body['ok']:
        raise RuntimeError(f"{body['error']['code']}: {body['error']['message']}")
    return body['data']

data = api_get('/api/v1/search', q='one piece', source='manga')
print(data['total'], 'hasil')
for it in data['items']:
    print(it['source'], it['title'], it['id'])`,
      php:
`<?php
$BASE = '${origin}';
$res  = file_get_contents($BASE . '/api/v1/search?' . http_build_query([
    'q' => 'one piece', 'source' => 'manga',
]));
$json = json_decode($res, true);
if (!$json['ok']) {
    throw new RuntimeException($json['error']['code'] . ': ' . $json['error']['message']);
}
echo $json['data']['total'] . " hasil\\n";
foreach ($json['data']['items'] as $it) {
    echo $it['source'] . ' ' . $it['title'] . "\\n";
}`,
      go:
`package main

import ("encoding/json"; "fmt"; "io"; "net/http"; "net/url")

const Base = "${origin}"

func main() {
    qs := url.Values{"q": {"one piece"}, "source": {"manga"}}
    resp, _ := http.Get(Base + "/api/v1/search?" + qs.Encode())
    defer resp.Body.Close()
    body, _ := io.ReadAll(resp.Body)
    var env struct {
        Ok   bool \`json:"ok"\`
        Data struct {
            Total int \`json:"total"\`
            Items []struct{ ID, Source, Title string } \`json:"items"\`
        } \`json:"data"\`
    }
    json.Unmarshal(body, &env)
    fmt.Printf("%d hasil\\n", env.Data.Total)
    for _, it := range env.Data.Items { fmt.Println(it.Source, it.Title) }
}`,
      rust:
`// reqwest = { version = "0.12", features = ["json"] }
// serde = { version = "1", features = ["derive"] }
use serde::Deserialize;

#[derive(Deserialize)] struct Env<T> { ok: bool, data: Option<T> }
#[derive(Deserialize)] struct Search { total: usize, items: Vec<Item> }
#[derive(Deserialize)] struct Item { id: String, source: String, title: String }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env: Env<Search> = reqwest::get(
        "${origin}/api/v1/search?q=one+piece&source=manga"
    ).await?.json().await?;
    let d = env.data.unwrap();
    println!("{} hasil", d.total);
    for it in d.items { println!("{} {}", it.source, it.title); }
    Ok(())
}`,
    };
  }

  function codeBlock(lang, code) {
    return `<div class="codeblock" data-lang="${lang}">
      <div class="cb-head"><span class="cb-lang">${lang}</span><button class="cb-copy">Salin</button></div>
      <pre><code>${h(code)}</code></pre>
    </div>`;
  }

  function routeDocs() {
    const origin = location.origin;
    const samples = codeSamples(origin);
    const langs = [["curl","cURL"],["javascript","JavaScript"],["python","Python"],["php","PHP"],["go","Go"],["rust","Rust"]];
    shell(`
      <div class="docs">
        <div class="hero-banner"><h1>Dokumentasi API</h1><p>Semua endpoint mengembalikan envelope JSON <code>{ status, ok, data, meta }</code>. Tidak perlu API key.</p></div>

        <h2>Base URL</h2>
        <p><code>${h(origin)}</code> &middot; base path <code>/api/v1</code></p>

        <h2>Contoh request</h2>
        <div class="lang-pills" id="langPills">
          ${langs.map(([v,l],i)=>`<button class="${i===0?"active":""}" data-lang="${v}">${l}</button>`).join("")}
        </div>
        <div id="sampleBox">${codeBlock("curl", samples.curl)}</div>

        <h2>Envelope respons</h2>
        ${codeBlock("json", `{
  "status": 200,
  "ok": true,
  "data": { /* payload spesifik endpoint */ },
  "meta": { "took_ms": 123, "cached": false, "request_id": "..." }
}`)}

        <h2>Endpoint</h2>
        <table>
          <thead><tr><th>Method</th><th>Path</th><th>Keterangan</th></tr></thead>
          <tbody>
            ${ENDPOINTS.map(([m,p,d])=>`<tr><td><span class="method">${m}</span></td><td><code>${h(p)}</code></td><td>${h(d)}</td></tr>`).join("")}
          </tbody>
        </table>

        <h2>Provider &amp; feed</h2>
        <table>
          <thead><tr><th>Provider</th><th>Jenis</th><th>Feed</th></tr></thead>
          <tbody>
            <tr><td>anichin</td><td>donghua</td><td>home, popular, rating, title</td></tr>
            <tr><td>mangaball</td><td>manga</td><td>home, popular, latest, recommend</td></tr>
            <tr><td>novelid</td><td>novel</td><td>home, popular, &lt;genre&gt;</td></tr>
            <tr><td>cosplaytele</td><td>cosplay (18+)</td><td>home, popular</td></tr>
            <tr><td>nhentai</td><td>doujin (18+)</td><td>popular-today, popular-week, popular, home</td></tr>
          </tbody>
        </table>

        <h2>Status &amp; error code</h2>
        <table>
          <thead><tr><th>Code</th><th>Arti</th></tr></thead>
          <tbody>
            <tr><td><code>200</code></td><td>Sukses</td></tr>
            <tr><td><code>400 invalid_id</code></td><td>Opaque ID rusak / tanda tangan salah</td></tr>
            <tr><td><code>400 missing_query</code></td><td>Search tanpa <code>q</code></td></tr>
            <tr><td><code>403 host_not_allowed</code></td><td>Host gambar di luar allowlist proxy</td></tr>
            <tr><td><code>404 not_found</code></td><td>Route tidak ada</td></tr>
            <tr><td><code>502 upstream_error</code></td><td>Sumber upstream gagal</td></tr>
          </tbody>
        </table>

        <p style="margin-top:24px">Butuh konsol penuh? Buka <a href="#/explorer">Explorer</a> atau <a href="/tester">dev console</a>.</p>
      </div>
    `);

    // language pill switching
    const pills = document.getElementById("langPills");
    const box = document.getElementById("sampleBox");
    pills.querySelectorAll("button").forEach(b => b.addEventListener("click", ()=>{
      pills.querySelectorAll("button").forEach(x=>x.classList.toggle("active", x===b));
      box.innerHTML = codeBlock(b.dataset.lang, samples[b.dataset.lang]);
      bindCopy();
    }));
    bindCopy();
  }

  function bindCopy() {
    document.querySelectorAll(".cb-copy").forEach(btn => {
      if (btn._bound) return; btn._bound = true;
      btn.addEventListener("click", () => {
        const code = btn.closest(".codeblock").querySelector("code").textContent;
        navigator.clipboard.writeText(code).then(
          ()=>{ btn.textContent="Tersalin"; setTimeout(()=>btn.textContent="Salin",1200); },
          ()=>{ btn.textContent="Gagal"; setTimeout(()=>btn.textContent="Salin",1200); }
        );
      });
    });
  }

  // ---- Explorer -----------------------------------------------------------
  const EXP = [
    "/api/v1/info",
    "/api/v1/search?q=one+piece&source=all&page=1",
    "/api/v1/browse/anichin?feed=home",
    "/api/v1/browse/mangaball?feed=popular",
    "/api/v1/browse/novelid?feed=home",
    "/api/v1/browse/nhentai?feed=popular-today",
    "/api/v1/manga/{id}?page=1&size=60",
    "/api/v1/manga/chapter/{id}",
    "/api/v1/donghua/{id}",
    "/api/v1/donghua/episode/{id}",
    "/api/v1/novel/{id}?page=1&size=60",
    "/api/v1/novel/chapter/{id}",
    "/api/v1/nhentai/{id}",
  ];

  function routeExplorer() {
    shell(`
      <div class="explorer">
        <div class="hero-banner"><h1>API Explorer</h1><p>Uji endpoint <code>/api/v1/*</code> langsung dan lihat JSON mentah.</p></div>
        <div class="exp-controls">
          <select id="exp-preset">
            <option value="">— pilih endpoint —</option>
            ${EXP.map(p=>`<option value="${h(p)}">${h(p)}</option>`).join("")}
          </select>
          <div class="exp-row">
            <input id="exp-path" type="text" value="/api/v1/info" spellcheck="false">
            <button class="btn primary" id="exp-send">Kirim</button>
          </div>
          <p style="color:var(--muted);font-size:12px;margin:8px 0 0">Tip: ganti <code>{id}</code> dengan opaque id dari hasil search/browse.</p>
        </div>
        <div class="exp-meta" id="exp-meta"></div>
        <pre class="exp-out" id="exp-out">// Respons akan tampil di sini.</pre>
      </div>
    `);
    const pathInput = document.getElementById("exp-path");
    const preset = document.getElementById("exp-preset");
    preset.addEventListener("change", ()=>{ if(preset.value) pathInput.value = preset.value; });
    const send = async ()=>{
      let path = pathInput.value.trim();
      const rel = path.replace(/^.*\/api\/v1/, "").replace(/^\/?/, "/");
      const meta = document.getElementById("exp-meta");
      const out = document.getElementById("exp-out");
      meta.innerHTML = `<span class="pill">...</span>`;
      out.textContent = "Loading...";
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
    pathInput.addEventListener("keydown", (e)=>{ if(e.key==="Enter") send(); });
  }

  // ---- Router -------------------------------------------------------------
  function router() {
    const parts = location.hash.replace(/^#\//,"").split("/").map(decodeURIComponent);
    const seg = parts[0] || "";
    window.scrollTo(0,0);
    switch (seg) {
      case "":
      case "home":     return routeHome();
      case "browse":   return routeBrowse(parts[1], parts[2], parts[3]);
      case "search":   return routeSearch(parts[1]||"", parts[2]);
      case "detail":   return routeDetail(parts[1], parts[2]);
      case "watch":    return routeWatch(parts[1]);
      case "read":     return routeRead(parts[1], parts[2]);
      case "docs":     return routeDocs();
      case "explorer": return routeExplorer();
      default:         return routeHome();
    }
  }

  A.router = router;
  window.addEventListener("hashchange", router);
  window.addEventListener("load", router);
  router();
})();
