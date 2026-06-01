// apiku consumer SPA — dependency-free hash router.
// Home / browse / search / detail / watch / read / gallery / docs / explorer.
(function () {
  "use strict";

  const API = "/api/v1";
  const app = document.getElementById("app");
  const CHAPTER_SIZE = 60;

  // ---- Branding (injected by server via window.__BRAND) -------------------
  const BRAND = Object.assign(
    { name: "apiku", tagline: "Streaming donghua, baca komik & novel, galeri cosplay - semua dalam satu platform.", logo: "", footer: "", ads: {} },
    (window.__BRAND || {})
  );
  // tiny escapers usable before `h` is defined
  function escHtml(s){ return (s==null?"":String(s)).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;"); }
  function escAttr(s){ return escHtml(s).replace(/"/g,"&quot;").replace(/'/g,"&#39;"); }
  // Logo markup: custom image if configured, otherwise the gradient mark.
  function brandLogo(){
    return BRAND.logo
      ? `<span class="logo img"><img src="${escAttr(BRAND.logo)}" alt=""></span>`
      : `<span class="logo">&#128250;</span>`;
  }
  function brandMark(){ return `${brandLogo()}<b>${escHtml(BRAND.name)}</b>`; }
  // Ad slot HTML for a named position (empty string when unconfigured).
  function adSlot(name){
    const html = BRAND.ads && BRAND.ads[name];
    return html ? `<div class="ad-slot" data-slot="${escAttr(name)}">${html}</div>` : "";
  }

  // ---- HLS playback (cosplay videos) --------------------------------------
  // Lazily load hls.js from a CDN once, then reuse.
  let _hlsLoading = null;
  function loadHlsJs(){
    if(window.Hls) return Promise.resolve(window.Hls);
    if(_hlsLoading) return _hlsLoading;
    _hlsLoading = new Promise((resolve, reject)=>{
      const s = document.createElement("script");
      s.src = "https://cdn.jsdelivr.net/npm/hls.js@1.5.17/dist/hls.min.js";
      s.onload = ()=>resolve(window.Hls);
      s.onerror = ()=>reject(new Error("gagal memuat pemutar"));
      document.head.appendChild(s);
    });
    return _hlsLoading;
  }

  // Resolve a signed cosplay-video URL -> HLS stream, then attach to <video>.
  async function attachHls(wrap){
    const resolveUrl = wrap.dataset.resolve;
    const idx = wrap.dataset.idx;
    const video = wrap.querySelector("video");
    const state = document.getElementById(`hls-state-${idx}`);
    const fail = (msg, src)=>{
      if(state) state.innerHTML = `<div class="hls-err">${escHtml(msg)}${src?`<br><a class="btn sm" href="${escAttr(src)}" target="_blank" rel="noopener noreferrer">Buka langsung</a>`:""}</div>`;
    };
    try{
      // resolveUrl is /api/v1/cosplay-video?... ; strip the /api/v1 prefix for api()
      const rel = resolveUrl.replace(/^.*\/api\/v1/, "");
      const res = await api(rel);
      const src = res && res.url;
      if(!src){ fail("Stream tidak ditemukan"); return; }
      if(video.canPlayType("application/vnd.apple.mpegurl")){
        // Safari plays HLS natively.
        video.src = src;
        if(state) state.remove();
        return;
      }
      const Hls = await loadHlsJs();
      if(Hls && Hls.isSupported()){
        const hls = new Hls({ maxBufferLength: 30 });
        hls.loadSource(src);
        hls.attachMedia(video);
        hls.on(Hls.Events.MANIFEST_PARSED, ()=>{ if(state) state.remove(); });
        hls.on(Hls.Events.ERROR, (_e, d)=>{ if(d && d.fatal) fail("Gagal memutar video", src); });
      } else {
        video.src = src;
        if(state) state.remove();
      }
    }catch(e){ fail(e.message || "Gagal memuat video"); }
  }
  // Footer: operator-configurable. Empty -> minimal "name (c) year".
  function footerHtml(){
    if(BRAND.footer && BRAND.footer.trim()) return BRAND.footer;
    const year = new Date().getFullYear();
    return `<span>${escHtml(BRAND.name)} &copy; ${year}</span> &middot; <a href="#/docs">API</a> &middot; dev <a href="https://github.com/risqinf" target="_blank" rel="noopener">@risqinf</a>`;
  }

  // ---- SVG icons ----------------------------------------------------------
  const I = {
    home:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/></svg>',
    donghua:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="14" rx="3"/><path d="m10 9 5 3-5 3z" fill="currentColor"/><path d="M8 21h8"/></svg>',
    anime:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 8l8-5 8 5v8l-8 5-8-5z"/><path d="m10 10 4 2-4 2z" fill="currentColor"/></svg>',
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
    dots:'<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/></svg>',
    close:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>',
    play:'<svg viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>',
    book:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19V5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2z"/><path d="M6 17h13"/></svg>',
    arrow:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>',
    expand:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H5a2 2 0 0 0-2 2v3M16 3h3a2 2 0 0 1 2 2v3M21 16v3a2 2 0 0 1-2 2h-3M3 16v3a2 2 0 0 0 2 2h3"/></svg>',
    compress:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3"/></svg>',
  };

  // ---- Provider config ---------------------------------------------------
  const PROVIDERS = {
    anime:   { label: "Anime",   api: "otakudesu",   kind: "anime",   adult: false, icon: I.anime },
    donghua: { label: "Donghua", api: "anichin",     kind: "donghua", adult: false, icon: I.donghua },
    manga:   { label: "Komik",   api: "mangaball",   kind: "manga",   adult: false, icon: I.manga },
    novel:   { label: "Novel",   api: "novelid",     kind: "novel",   adult: false, icon: I.novel },
    cosplay: { label: "Cosplay", api: "cosplaytele", kind: "cosplay", adult: true,  icon: I.cosplay },
    doujin:  { label: "Doujin",  api: "nhentai",     kind: "doujin",  adult: true,  icon: I.doujin },
  };
  const EPISODE_SIZE = 5000; // donghua: fetch the whole episode list at once

  const FEEDS = {
    otakudesu:   [["ongoing","Ongoing"],["complete","Completed"],["action","Action"],["romance","Romance"],["comedy","Comedy"],["fantasy","Fantasy"],["adventure","Adventure"],["drama","Drama"]],
    anichin:     [["home","Terbaru"],["popular","Populer"],["rating","Rating"],["title","A-Z"]],
    mangaball:   [["home","Unggulan"],["popular","Populer"],["latest","Terbaru"],["recommend","Rekomendasi"]],
    novelid:     [["home","All"],["popular","Completed"],["novel-translate","Translated"],["fantasi","Fantasy"],["romantis","Romance"],["aksi","Action"],["horror","Horror"]],
    cosplaytele: [["home","Terbaru"],["popular","Populer"]],
    nhentai:     [["popular-today","Hari Ini"],["popular-week","Minggu Ini"],["popular","Sepanjang Masa"],["home","Terbaru"]],
  };

  const DETAIL_EP = { anime:"anime", donghua:"donghua", manga:"manga", novel:"novel", cosplay:"cosplay", doujin:"nhentai" };

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

  // ---- Client-side prefetch cache -----------------------------------------
  // A small in-memory cache + an idle prefetch queue. We use it to warm the
  // detail/episode/chapter that the user is most likely to open next so the
  // navigation feels instant. Requests are coalesced (single-flight).
  const _cache = new Map();      // path -> resolved data
  const _inflight = new Map();   // path -> Promise
  const _idleQ = [];
  let _idleRunning = false;
  const _idle = (cb)=> (window.requestIdleCallback ? requestIdleCallback(cb,{timeout:1500}) : setTimeout(cb,120));

  async function apiCached(path){
    if(_cache.has(path)) return _cache.get(path);
    if(_inflight.has(path)) return _inflight.get(path);
    const p = api(path).then(d=>{ _cache.set(path, d); _inflight.delete(path); return d; })
                       .catch(e=>{ _inflight.delete(path); throw e; });
    _inflight.set(path, p);
    return p;
  }
  function _drainIdle(){
    if(_idleRunning) return; _idleRunning = true;
    const step = ()=>{
      const path = _idleQ.shift();
      if(!path){ _idleRunning = false; return; }
      if(_cache.has(path) || _inflight.has(path)){ _idle(step); return; }
      apiCached(path).catch(()=>{}).finally(()=> _idle(step));
    };
    _idle(step);
  }
  // Queue one or more API paths to warm in the background (deduped).
  function prefetch(paths){
    (Array.isArray(paths)?paths:[paths]).forEach(p=>{
      if(p && !_cache.has(p) && !_inflight.has(p) && !_idleQ.includes(p)) _idleQ.push(p);
    });
    _drainIdle();
  }

  const go = (hash) => { location.hash = hash; };
  const viewEl = () => document.getElementById("view");
  const setView = (html) => { const v=viewEl(); if(v) v.innerHTML=html; };
  const spinner = `<div class="spinner"></div>`;
  const skelGrid = (n) => `<div class="grid">${Array.from({length:n||12}).map(()=>`<div class="skeleton poster"></div>`).join("")}</div>`;

  function imgTag(url, _cls, alt){
    if(!url) return `<div class="ph">${h(alt||"no image")}</div>`;
    return `<img loading="lazy" decoding="async" referrerpolicy="no-referrer" src="${h(url)}" alt="${h(alt||"")}" onerror="this.parentNode.innerHTML='<div class=ph>no image</div>'">`;
  }
  // Natural-ratio image: lets the browser size by the real image dimensions
  // (used for cosplay/doujin galleries where not everything is 2:3).
  function imgNatural(url, alt){
    if(!url) return `<div class="ph">${h(alt||"no image")}</div>`;
    return `<img class="nat" loading="lazy" decoding="async" referrerpolicy="no-referrer" src="${h(url)}" alt="${h(alt||"")}" onerror="this.style.display='none'">`;
  }

  // ---- Shell --------------------------------------------------------------
  // Primary content providers shown directly in the desktop nav bar.
  function navLinks(){
    const items = [
      ["home", "#/", "Home", I.home, false],
      ["anime", "#/browse/anime", "Anime", I.anime, false],
      ["donghua", "#/browse/donghua", "Donghua", I.donghua, false],
      ["manga", "#/browse/manga", "Komik", I.manga, false],
      ["novel", "#/browse/novel", "Novel", I.novel, false],
    ];
    if (adultOn()) {
      items.push(["cosplay", "#/browse/cosplay", "Cosplay", I.cosplay, true]);
      items.push(["doujin", "#/browse/doujin", "Doujin", I.doujin, true]);
    }
    return items;
  }
  // Developer tools — tucked into an overflow menu on desktop to keep the bar
  // uncluttered, listed in full in the mobile drawer.
  function toolLinks(){
    return [
      ["docs", "#/docs", "API Docs", I.docs, false],
      ["explorer", "#/explorer", "Explorer", I.explorer, false],
    ];
  }
  // Render a nav link; adult items get a red "18+" badge.
  function navItem(s, href, label, ico, adult, seg){
    const badge = adult ? `<span class="nv18">18+</span>` : "";
    return `<a data-seg="${s}" href="${href}" class="${s===seg?"active":""}">${ico}<span>${label}</span>${badge}</a>`;
  }

  // Map any route to the nav item that should appear "active".
  function activeNavSeg(){
    const parts = location.hash.replace(/^#\//,"").split("/").map(decodeURIComponent);
    const seg = parts[0] || "home";
    switch(seg){
      case "": case "home": return "home";
      case "browse": return parts[1] || "";
      case "detail": return parts[1] || "";          // kind: anime/donghua/manga/novel/cosplay/doujin
      case "watch":  return "donghua";
      case "watchanime": return "anime";
      case "read":   return parts[1] === "nhentai" ? "doujin" : (parts[1] || "");
      case "docs":   return "docs";
      case "explorer": return "explorer";
      default: return seg;                            // search etc -> nothing highlighted
    }
  }

  // Age-verification modal (replaces the bare confirm() dialog).
  function showAgeModal(onYes){
    const wrap = document.createElement("div");
    wrap.className = "modal-scrim";
    wrap.innerHTML = `
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="ageTitle">
        <div class="modal-ico">${I.doujin}</div>
        <h3 id="ageTitle">Konten Dewasa &middot; 18+</h3>
        <p>Bagian <b>Cosplay</b> dan <b>Doujin</b> berisi materi khusus dewasa. Dengan melanjutkan, kamu menyatakan berusia minimal 18 tahun dan setuju menampilkan konten ini.</p>
        <div class="modal-actions">
          <button class="btn" data-act="no">Batal</button>
          <button class="btn primary" data-act="yes">Ya, saya 18+</button>
        </div>
      </div>`;
    document.body.appendChild(wrap);
    requestAnimationFrame(()=>wrap.classList.add("open"));
    const close = ()=>{ wrap.classList.remove("open"); setTimeout(()=>wrap.remove(),200); document.removeEventListener("keydown", esc); };
    function esc(e){ if(e.key==="Escape") close(); }
    wrap.addEventListener("click",(e)=>{ if(e.target===wrap) close(); });
    wrap.querySelector('[data-act="no"]').onclick = close;
    wrap.querySelector('[data-act="yes"]').onclick = ()=>{ close(); onYes(); };
    document.addEventListener("keydown", esc);
  }

  function shell(inner){
    const seg = activeNavSeg();
    const links = navLinks();
    const tools = toolLinks();
    const themeIco = store.theme === "dark" ? I.sun : I.moon;

    app.innerHTML = `
      <div class="drawer-scrim" id="scrim"></div>
      <aside class="drawer" id="drawer">
        <div class="dhead">
          <span class="brand">${brandMark()}</span>
          <button class="icon-btn" id="drawerClose">${I.close}</button>
        </div>
        <nav>
          ${links.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")}
          <div class="dsep"></div>
          ${tools.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")}
          <a href="/tester"><span>${I.explorer}</span><span>Dev Console</span></a>
        </nav>
        <div class="dsep"></div>
        <div class="drow">
          <button class="switch ${adultOn()?"on":""}" id="adultBtnD" role="switch" aria-checked="${adultOn()}">
            <span class="switch-label"><span class="b18">18+</span> Konten dewasa</span>
            <span class="switch-track"><span class="switch-thumb"></span></span>
          </button>
          <button class="switch ${store.theme==="dark"?"on":""}" id="themeBtnD" role="switch" aria-checked="${store.theme==="dark"}">
            <span class="switch-label">${themeIco}<span>Mode gelap</span></span>
            <span class="switch-track"><span class="switch-thumb"></span></span>
          </button>
        </div>
      </aside>

      <header class="hdr">
        <button class="icon-btn hamburger" id="hamburger">${I.menu}</button>
        <a class="brand" href="#/">${brandMark()}</a>
        <nav class="desktop">
          ${links.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")}
          <div class="navmore">
            <button class="navmore-btn ${tools.some(t=>t[0]===seg)?"active":""}" id="moreBtn" aria-haspopup="true" aria-expanded="false">${I.dots}</button>
            <div class="navmore-menu" id="moreMenu">
              ${tools.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")}
              <a href="/tester">${I.explorer}<span>Dev Console</span></a>
            </div>
          </div>
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
      <footer>${footerHtml()}</footer>`;

    // search
    const form = document.getElementById("searchform");
    const input = document.getElementById("searchinput");
    form.addEventListener("submit", (e)=>{ e.preventDefault(); const q=input.value.trim(); if(q) go(`#/search/${encodeURIComponent(q)}`); });
    const m = location.hash.match(/^#\/search\/([^/]+)/);
    if (m) input.value = decodeURIComponent(m[1]);

    // theme — switch live (no full re-render, so it doesn't flicker)
    const themeBtn = document.getElementById("themeBtn");
    const themeBtnD = document.getElementById("themeBtnD");
    const toggleTheme = ()=>{
      store.theme = store.theme==="dark"?"light":"dark";
      applyTheme();
      // update header icon button
      if(themeBtn) themeBtn.innerHTML = store.theme==="dark" ? I.sun : I.moon;
      // update drawer switch state + icon
      if(themeBtnD){
        themeBtnD.classList.toggle("on", store.theme==="dark");
        themeBtnD.setAttribute("aria-checked", String(store.theme==="dark"));
        const lab = themeBtnD.querySelector(".switch-label");
        if(lab) lab.innerHTML = `${store.theme==="dark"?I.sun:I.moon}<span>Mode gelap</span>`;
      }
    };
    if(themeBtn) themeBtn.onclick = toggleTheme;
    if(themeBtnD) themeBtnD.onclick = toggleTheme;

    // adult — animate the switch first, then re-render after the transition
    const adultBtn = document.getElementById("adultBtn");
    const adultBtnD = document.getElementById("adultBtnD");
    const enableAdult = ()=>{
      store.adult = true;
      if(adultBtnD){ adultBtnD.classList.add("on"); adultBtnD.setAttribute("aria-checked","true"); }
      if(adultBtn) adultBtn.classList.add("on");
      setTimeout(()=>window.__apiku.router(), 200);
    };
    const toggleAdult = ()=>{
      if(!adultOn()){ showAgeModal(enableAdult); return; }
      // turning OFF: animate switch off first
      store.adult = false;
      if(adultBtnD){ adultBtnD.classList.remove("on"); adultBtnD.setAttribute("aria-checked","false"); }
      if(adultBtn) adultBtn.classList.remove("on");
      const parts = location.hash.replace(/^#\//,"").split("/").map(decodeURIComponent);
      const inAdult = (parts[0]==="browse" && (parts[1]==="cosplay"||parts[1]==="doujin"))
        || (parts[0]==="detail" && (parts[1]==="cosplay"||parts[1]==="doujin"))
        || (parts[0]==="read" && parts[1]==="nhentai");
      if(inAdult){ setTimeout(()=>go("#/"), 200); return; }
      setTimeout(()=>window.__apiku.router(), 200);
    };
    if(adultBtn) adultBtn.onclick = toggleAdult;
    if(adultBtnD) adultBtnD.onclick = toggleAdult;

    // drawer
    const drawer = document.getElementById("drawer");
    const scrim = document.getElementById("scrim");
    const openDrawer = ()=>{ drawer.classList.add("open"); scrim.classList.add("open"); };
    const closeDrawer = ()=>{ drawer.classList.remove("open"); scrim.classList.remove("open"); };
    document.getElementById("hamburger").onclick = openDrawer;
    document.getElementById("drawerClose").onclick = closeDrawer;
    scrim.onclick = closeDrawer;
    drawer.querySelectorAll("nav a").forEach(a => a.addEventListener("click", closeDrawer));

    // desktop "More" overflow menu (tools)
    const moreBtn = document.getElementById("moreBtn");
    const moreMenu = document.getElementById("moreMenu");
    if(moreBtn && moreMenu){
      const closeMore = ()=>{ moreMenu.classList.remove("open"); moreBtn.setAttribute("aria-expanded","false"); document.removeEventListener("click", onDocClick); };
      const onDocClick = (e)=>{ if(!moreMenu.contains(e.target) && e.target!==moreBtn && !moreBtn.contains(e.target)) closeMore(); };
      moreBtn.onclick = (e)=>{
        e.stopPropagation();
        const open = moreMenu.classList.toggle("open");
        moreBtn.setAttribute("aria-expanded", String(open));
        if(open) setTimeout(()=>document.addEventListener("click", onDocClick),0);
      };
      moreMenu.querySelectorAll("a").forEach(a => a.addEventListener("click", closeMore));
    }
  }

  // ---- Cards --------------------------------------------------------------
  function cardHtml(item){
    const prov = Object.values(PROVIDERS).find(p=>p.kind===item.kind) || {};
    const tags = (item.tags||[]).slice(0,2).map(t=>`<span>${h(t)}</span>`).join("");
    const detailHref = `#/detail/${encodeURIComponent(item.kind)}/${encodeURIComponent(item.id)}`;
    return `
      <div class="card" data-go="${detailHref}" data-prefetch-kind="${h(item.kind)}" data-prefetch-id="${h(item.id)}">
        <div class="poster">${imgTag(item.thumbnail,"",item.title)}<span class="badge src">${h(prov.label||item.source)}</span></div>
        <div class="meta"><div class="t">${h(item.title)}</div><div class="sub">${tags}</div></div>
      </div>`;
  }
  const grid = (items) => (!items||!items.length) ? `<div class="empty">Tidak ada hasil.</div>` : `<div class="grid">${items.map(cardHtml).join("")}</div>`;

  document.addEventListener("click", (e)=>{ const el=e.target.closest("[data-go]"); if(el){ e.preventDefault(); go(el.dataset.go); } });

  // Warm the detail endpoint when the pointer hovers a card (desktop) or on
  // first touch (mobile) so opening it feels instant.
  function _prefetchCard(el){
    const kind = el.dataset.prefetchKind, id = el.dataset.prefetchId;
    if(!kind || !id) return;
    const ep = DETAIL_EP[kind];
    if(!ep) return;
    if(kind==="cosplay") prefetch(`/cosplay/${encodeURIComponent(id)}`);
    else if(kind==="doujin") prefetch(`/nhentai/${encodeURIComponent(id)}`);
    else if(kind==="donghua") prefetch(`/${ep}/${encodeURIComponent(id)}?${qs({page:1,size:EPISODE_SIZE})}`);
    else prefetch(`/${ep}/${encodeURIComponent(id)}?${qs({page:1,size:CHAPTER_SIZE})}`);
  }
  document.addEventListener("pointerenter", (e)=>{
    const el = e.target.closest && e.target.closest("[data-prefetch-kind]");
    if(el) _prefetchCard(el);
  }, true);

  function crumbs(items){
    return `<div class="crumbs">`+items.map((it,i)=> i<items.length-1?`<a href="${it.href}">${h(it.label)}</a><span>/</span>`:`<b>${h(it.label)}</b>`).join("")+`</div>`;
  }

  // ===========================================================================
  // Home / Browse / Search
  // ===========================================================================
  async function routeHome(){
    shell(`
      <div class="hero-banner">
        <h1>${escHtml(BRAND.name)}</h1>
        <p>${escHtml(BRAND.tagline)}</p>
      </div>
      ${adSlot("home")}
      <div id="rows"></div>
    `);
    const rows = document.getElementById("rows");
    let sections = [
      { title:"Anime Ongoing",   prov:"otakudesu",   feed:"ongoing",       seg:"anime" },
      { title:"Donghua Terbaru", prov:"anichin",     feed:"home",          seg:"donghua" },
      { title:"Komik Populer",   prov:"mangaball",   feed:"popular",       seg:"manga" },
      { title:"Novel Terbaru",   prov:"novelid",     feed:"home",          seg:"novel" },
      { title:"Cosplay Terbaru", prov:"cosplaytele", feed:"home",          seg:"cosplay", adult:true },
      { title:"Doujin Hari Ini", prov:"nhentai",     feed:"popular-today", seg:"doujin",  adult:true },
    ].filter(s=>!s.adult||adultOn());
    rows.innerHTML = sections.map((s,i)=>`
      <div class="row-head"><h2><span class="dot"></span>${h(s.title)}</h2><a class="more" href="#/browse/${s.seg}">Lihat semua ${I.arrow}</a></div>
      <div id="row-${i}">${skelGrid(6)}</div>`).join("");
    sections.forEach(async (s,i)=>{
      try { const data=await apiCached(`/browse/${s.prov}?${qs({feed:s.feed})}`); document.getElementById(`row-${i}`).innerHTML=grid((data.items||[]).slice(0,12)); }
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
      ${adSlot("browse")}
      <div id="list">${skelGrid(18)}</div>
      <div id="pager"></div>
    `);
    try{
      const data = await apiCached(`/browse/${prov.api}?${qs({feed,page})}`);
      document.getElementById("list").innerHTML = grid(data.items);
      document.getElementById("pager").innerHTML = `
        <div class="pager">
          ${page>1?`<a class="btn sm" href="#/browse/${seg}/${feed}/${page-1}">&larr; Sebelumnya</a>`:""}
          <span>Halaman ${page}</span>
          ${(data.items&&data.items.length)?`<a class="btn sm" href="#/browse/${seg}/${feed}/${page+1}">Berikutnya &rarr;</a>`:""}
        </div>`;
      // warm the next page so paging feels instant
      if(data.items&&data.items.length) prefetch(`/browse/${prov.api}?${qs({feed,page:page+1})}`);
    }catch(e){ document.getElementById("list").innerHTML=`<div class="errbox">${h(e.message)}</div>`; }
  }

  // Search with a source filter (grouped, not all-at-once clutter)
  async function routeSearch(query, src, mode){
    src = src || "all";
    const locked = mode === "lock"; // came from a cosplayer/tag pill (precise)
    shell(`
      <div class="row-head"><h2><span class="dot"></span>Hasil: &ldquo;${h(query)}&rdquo;</h2></div>
      <div class="chips" id="srcChips"></div>
      <div id="list">${skelGrid(12)}</div>
    `);
    const allSources = [["all","Semua"],["anime","Anime"],["donghua","Donghua"],["manga","Komik"],["novel","Novel"]];
    if(adultOn()){ allSources.push(["cosplay","Cosplay"]); allSources.push(["doujin","Doujin"]); }
    if(src!=="all" && !providerVisible(src)) src = "all";
    try{
      if(locked && src!=="all"){
        // Locked single-source mode (tag/name pill): high precision, only this
        // provider's results, minimal chips.
        const data = await apiCached(`/search?${qs({q:query, source:src, page:1})}`);
        const items = (data.items||[]).filter(it=>providerVisible(it.kind));
        const label = (allSources.find(([v])=>v===src)||[src,src])[1];
        const chips = document.getElementById("srcChips");
        chips.innerHTML =
          `<button class="chip" data-go="#/search/${encodeURIComponent(query)}">${I.arrow} Semua sumber</button>`+
          `<button class="chip active">${h(label)} <span class="cnt">${items.length}</span></button>`;
        document.getElementById("list").innerHTML = grid(items);
        return;
      }

      // Normal cross-provider search: fetch ALL once, filter client-side so
      // every provider's count stays visible (e.g. "Donghua 12") and toggling
      // is instant without a re-fetch.
      const data = await apiCached(`/search?${qs({q:query, source:"all", page:1})}`);
      let items = (data.items||[]).filter(it=>providerVisible(it.kind));
      const counts = {};
      items.forEach(it=>{ counts[it.kind]=(counts[it.kind]||0)+1; });
      const visible = allSources.filter(([v])=> v==="all" || (counts[v]||0) > 0);
      // If the requested filter has no results, fall back to "all".
      if(src!=="all" && !(counts[src]>0)) src = "all";

      const chips = document.getElementById("srcChips");
      const renderChips = ()=>{
        chips.innerHTML = visible.map(([v,l])=>{
          const c = v==="all" ? items.length : (counts[v]||0);
          return `<button class="chip ${v===src?"active":""}" data-src="${v}">${h(l)} <span class="cnt">${c}</span></button>`;
        }).join("");
        chips.querySelectorAll(".chip").forEach(ch => ch.addEventListener("click", ()=>{
          src = ch.dataset.src;
          history.replaceState(null,"", src==="all" ? `#/search/${encodeURIComponent(query)}` : `#/search/${encodeURIComponent(query)}/${src}`);
          renderChips();
          renderList();
        }));
      };
      const renderList = ()=>{
        const filtered = src==="all" ? items : items.filter(it=>it.kind===src);
        document.getElementById("list").innerHTML = grid(filtered);
      };
      renderChips();
      renderList();
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
      if(kind==="anime"){ const data = await apiCached(`/anime/${encodeURIComponent(id)}`); return renderAnimeSeries(id, data); }
      const size = kind==="donghua" ? EPISODE_SIZE : CHAPTER_SIZE;
      const data = await apiCached(`/${ep}/${encodeURIComponent(id)}?${qs({page:1,size})}`);
      if(kind==="donghua") return renderDonghuaSeries(id, data);
      return renderReadableSeries(kind, id, data, 1);
    }catch(e){ setD(`<div class="errbox">${h(e.message)}</div>`); }
  }

  function renderAnimeSeries(id, data){
    const eps = data.episodes||[];
    const facts = [
      data.status?`<span class="pill ok">${h(data.status)}</span>`:"",
      data.score?`<span class="pill">&#9733; ${h(data.score)}</span>`:"",
      data.anime_type?`<span class="pill">${h(data.anime_type)}</span>`:"",
      data.total_episodes?`<span class="pill">${h(data.total_episodes)} eps</span>`:"",
      data.duration?`<span class="pill">${h(data.duration)}</span>`:"",
      data.studio?`<span class="pill">${h(data.studio)}</span>`:"",
      data.release_date?`<span class="pill">${h(data.release_date)}</span>`:"",
      ...(data.genres||[]).slice(0,6).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = eps[0];
    const last = eps[eps.length-1];
    const actions = [
      first?`<a class="btn primary" href="#/watchanime/${encodeURIComponent(first.id)}">${I.play} Tonton Eps ${first.number??1}</a>`:"",
      (last && last!==first)?`<a class="btn" href="#/watchanime/${encodeURIComponent(last.id)}">Eps terbaru ${last.number??""}</a>`:"",
      ...(data.batch||[]).slice(0,1).map(b=>`<a class="btn sm" href="#/watchanime/${encodeURIComponent(b.id)}">${I.book} Batch</a>`),
    ].join("");
    const syn = data.synopsis || (data.japanese_title?`Judul Jepang: ${data.japanese_title}`:"");
    const epControls = eps.length>24
      ? `<div class="ep-tools"><input id="epSearch" type="search" inputmode="numeric" placeholder="Lompat ke episode..." autocomplete="off"></div>`
      : "";
    const epGrid = eps.length
      ? `<div class="ep-list" id="epList">${eps.map(e=>`<button class="ep-btn center" data-ep="${e.number??""}" data-go="#/watchanime/${encodeURIComponent(e.id)}">Eps ${e.number??(e.title||"?")}</button>`).join("")}</div>`
      : `<div class="empty">Belum ada episode.</div>`;

    setD(
      heroHtml("anime","Anime",data,facts,actions,syn,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>Episode <span class="cnt-badge">${eps.length}</span></h2></div>${epControls}${epGrid}`
    );
    const epSearch = document.getElementById("epSearch");
    if(epSearch){
      epSearch.addEventListener("input", ()=>{
        const q = epSearch.value.trim().toLowerCase();
        document.querySelectorAll("#epList .ep-btn").forEach(b=>{
          const n = (b.dataset.ep||"").toLowerCase();
          b.style.display = (!q || n.includes(q)) ? "" : "none";
        });
      });
    }
    if(first) prefetch(`/anime/episode/${encodeURIComponent(first.id)}`);
    renderRecommendations("anime", id);
  }

  // Render a "Rekomendasi" row at the bottom of a detail page, sourced from
  // the provider's popular feed. Excludes the current item.
  async function renderRecommendations(kind, excludeId){
    const prov = PROVIDERS[kind];
    if(!prov) return;
    const host = document.createElement("div");
    host.className = "rec-block";
    host.innerHTML = `<div class="row-head"><h2><span class="dot"></span>Rekomendasi</h2><a class="more" href="#/browse/${kind}">Lihat semua ${I.arrow}</a></div><div id="recRow">${skelGrid(6)}</div>`;
    const d = document.getElementById("d");
    if(d) d.appendChild(host);
    try{
      const feed = (FEEDS[prov.api] && FEEDS[prov.api][1]) ? FEEDS[prov.api][1][0] : "popular";
      const data = await apiCached(`/browse/${prov.api}?${qs({feed})}`);
      let items = (data.items||[]).filter(it=>it.id!==excludeId).slice(0,12);
      const row = document.getElementById("recRow");
      if(row) row.innerHTML = grid(items);
    }catch(e){ const row=document.getElementById("recRow"); if(row) row.innerHTML=`<div class="empty">Tidak ada rekomendasi.</div>`; }
  }

  function renderDonghuaSeries(id, data){
    const eps = data.episodes||[];
    const facts = [
      data.status?`<span class="pill ok">${h(data.status)}</span>`:"",
      `<span class="pill">${data.episode_count} episode</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = eps[0];
    const last = eps[eps.length-1];
    const actions = [
      first?`<a class="btn primary" href="#/watch/${encodeURIComponent(first.id)}">${I.play} Tonton Eps ${first.number}</a>`:"",
      (last && last!==first)?`<a class="btn" href="#/watch/${encodeURIComponent(last.id)}">Eps terbaru ${last.number}</a>`:"",
    ].join("");

    // Episode access helper: search box (jump to a number) + a scrollable
    // grid. For very long series we keep the grid but make it searchable.
    const epControls = eps.length>24
      ? `<div class="ep-tools"><input id="epSearch" type="search" inputmode="numeric" placeholder="Lompat ke episode... (mis. 120)" autocomplete="off"></div>`
      : "";
    const epGrid = eps.length
      ? `<div class="ep-list" id="epList">${eps.map(e=>`<button class="ep-btn center" data-ep="${e.number}" data-go="#/watch/${encodeURIComponent(e.id)}">Eps ${e.number}</button>`).join("")}</div>`
      : `<div class="empty">Belum ada episode.</div>`;

    setD(
      heroHtml("donghua","Donghua",data,facts,actions,data.synopsis,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>Episode <span class="cnt-badge">${eps.length}</span></h2></div>${epControls}${epGrid}`
    );

    // wire episode search/jump
    const epSearch = document.getElementById("epSearch");
    if(epSearch){
      epSearch.addEventListener("input", ()=>{
        const q = epSearch.value.trim().toLowerCase();
        document.querySelectorAll("#epList .ep-btn").forEach(b=>{
          const n = (b.dataset.ep||"").toLowerCase();
          b.style.display = (!q || n.includes(q)) ? "" : "none";
        });
      });
    }

    // warm the first episode + recommendations
    if(first) prefetch(`/donghua/episode/${encodeURIComponent(first.id)}`);
    renderRecommendations("donghua", id);
  }

  // Manga/novel — with LANGUAGE GROUPING for manga (translations).
  async function renderReadableSeries(kind, id, data, page, activeLang){
    const label = kind==="manga"?"Komik":"Novel";
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
    // Preserve the chosen language across pager reloads; default to "all".
    const validLang = activeLang && (activeLang==="__all__" || languages.some(([l])=>l===activeLang));
    const langState = { active: validLang ? activeLang : "__all__" };

    // chapters actually available in the active language (for first-read + count)
    const chsInLang = (lang)=> (kind!=="manga"||lang==="__all__")
      ? chs
      : chs.filter(c => (c.translations||[]).some(t => (t.language||"Lainnya")===lang));

    const firstList = chsInLang(langState.active);
    const first = firstList[0];
    const firstReadId = first
      ? (kind==="manga" && langState.active!=="__all__"
          ? ((first.translations||[]).find(t=>(t.language||"Lainnya")===langState.active)||first).id
          : first.id)
      : null;
    const actions = firstReadId?`<a class="btn primary" href="#/${readPath}/${encodeURIComponent(firstReadId)}">${I.book} Mulai Baca</a>`:"";
    const syn = data.description || data.synopsis;

    const langTabs = (kind==="manga" && languages.length>1)
      ? `<div class="lang-tabs" id="langTabs">
          <button class="lang-tab ${langState.active==="__all__"?"active":""}" data-lang="__all__">Semua <span class="cnt">${chs.length}</span></button>
          ${languages.map(([l,c])=>`<button class="lang-tab ${langState.active===l?"active":""}" data-lang="${h(l)}">${h(l)} <span class="cnt">${c}</span></button>`).join("")}
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

    // wire language tabs (persist selection)
    const tabsEl = document.getElementById("langTabs");
    if (tabsEl) {
      tabsEl.querySelectorAll(".lang-tab").forEach(tab => tab.addEventListener("click", ()=>{
        langState.active = tab.dataset.lang;
        tabsEl.querySelectorAll(".lang-tab").forEach(t=>t.classList.toggle("active", t===tab));
        document.getElementById("chList").innerHTML = chapterRowsFor(langState.active) || `<div class="empty">Tidak ada bab untuk bahasa ini.</div>`;
      }));
    }

    // wire chapter pager — keep the active language when reloading a page
    const ep = DETAIL_EP[kind];
    const load = async (p)=>{
      document.querySelectorAll("#d .ep-list").forEach(n=>n.innerHTML=`<div class="spinner"></div>`);
      const fresh = await apiCached(`/${ep}/${encodeURIComponent(id)}?${qs({page:p,size:CHAPTER_SIZE})}`);
      renderReadableSeries(kind, id, fresh, p, langState.active);
    };
    const pv=document.getElementById("ch-prev"); if(pv) pv.onclick=()=>load(page-1);
    const nx=document.getElementById("ch-next"); if(nx) nx.onclick=()=>load(page+1);

    // warm the first chapter + next chapter page + recommendations
    if(firstReadId) prefetch(`/${kind==="manga"?"manga/chapter":"novel/chapter"}/${encodeURIComponent(firstReadId)}`);
    if(page<totalPages) prefetch(`/${ep}/${encodeURIComponent(id)}?${qs({page:page+1,size:CHAPTER_SIZE})}`);
    renderRecommendations(kind, id);
  }

  async function renderCosplay(id){
    const data = await apiCached(`/cosplay/${encodeURIComponent(id)}`);
    // Cosplayer name is clickable -> search that name, locked to cosplay.
    const searchChip = (kind, label) => `#/search/${encodeURIComponent(label)}/${kind}/lock`;
    const facts = [
      data.cosplayer?`<a class="pill link" href="${searchChip("cosplay", data.cosplayer)}">${h(data.cosplayer)}</a>`:"",
      data.character?`<span class="pill">${h(data.character)}</span>`:"",
      data.series?`<span class="pill">${h(data.series)}</span>`:"",
      data.photo_count?`<span class="pill">${data.photo_count} foto</span>`:"",
      data.video_count?`<span class="pill">${data.video_count} video</span>`:"",
      ...(data.tags||[]).slice(0,4).map(t=>`<a class="pill link" href="${searchChip("cosplay", t)}">${h(t)}</a>`),
    ].join("");
    const dls = (data.downloads||[]).map(d=>`<a class="btn sm" target="_blank" rel="noopener" href="${h(d.url)}">${h(d.name)}</a>`).join("");
    const actions = dls + (data.unzip_password?`<span class="pill">&#128273; ${h(data.unzip_password)}</span>`:"");

    // Video player(s) FIRST, before the photo flow.
    const vids = (data.videos||[]);
    const videoBlock = vids.length
      ? `<div class="row-head"><h2><span class="dot"></span>Video</h2></div>` +
        vids.map((u,i)=>{
          const isFile = /\.(mp4|webm|m4v|mov)(\?|$)/i.test(u);
          if(isFile){
            return `<div class="video-wrap"><video controls preload="metadata" playsinline src="${h(u)}"></video></div>`;
          }
          // Cosplaytele videos resolve through our backend (/api/v1/cosplay-video),
          // which decrypts the cossora embed and returns an HLS stream we proxy
          // and play with hls.js. Other embeds fall back to an iframe.
          if(/\/cosplay-video\?/.test(u)){
            return `<div class="video-wrap hls" data-resolve="${escAttr(u)}" data-idx="${i}">
                <video id="hls-${i}" controls preload="metadata" playsinline></video>
                <div class="hls-state" id="hls-state-${i}">${spinner}</div>
              </div>`;
          }
          return `<div class="embed-wrap">
              <div class="embed-fallback">Sumber video eksternal. <a class="btn sm" href="${h(u)}" target="_blank" rel="noopener noreferrer">${I.play} Buka video ${I.arrow}</a></div>
            </div>`;
        }).join("")
      : "";

    // Photos: natural aspect ratio masonry (not forced 2:3).
    const imgs = (data.images||[]).map(u=>`<a href="${h(u)}" target="_blank" rel="noopener">${imgNatural(u,"")}</a>`).join("");

    setD(
      heroHtml("cosplay","Cosplay",data,facts,actions,null,data.cover)+
      videoBlock+
      `<div class="row-head"><h2><span class="dot"></span>${(data.images||[]).length} Foto</h2></div>`+
      `<div class="gallery">${imgs||`<div class="empty">Tidak ada foto.</div>`}</div>`
    );
    // Resolve + attach HLS players
    document.querySelectorAll(".video-wrap.hls").forEach(el => attachHls(el));
    renderRecommendations("cosplay", id);
  }

  async function renderDoujin(id){
    const data = await apiCached(`/nhentai/${encodeURIComponent(id)}`);
    // Tags/genres are clickable -> search "[tag]" locked to doujin (nhentai
    // supports the `[tag]` syntax for an exact-tag match).
    const tagSearch = (t) => `#/search/${encodeURIComponent("["+t+"]")}/doujin/lock`;
    const nameSearch = (t) => `#/search/${encodeURIComponent(t)}/doujin/lock`;
    const facts = [
      data.author?`<a class="pill link" href="${nameSearch(data.author)}">&#9997; ${h(data.author)}</a>`:"",
      data.artist?`<a class="pill link" href="${nameSearch(data.artist)}">${h(data.artist)}</a>`:"",
      ...(data.genres||[]).slice(0,16).map(g=>`<a class="pill link" href="${tagSearch(g)}">${h(g)}</a>`)
    ].join("");
    const first = (data.chapters||[])[0];
    const actions = first?`<a class="btn primary" href="#/read/nhentai/${encodeURIComponent(first.id)}">${I.book} Baca</a>`:"";
    setD(
      heroHtml("doujin","Doujin",data,facts,actions,null,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>Pratinjau Halaman</h2></div>
       <div id="preview" class="thumb-grid">${skelGrid(8)}</div>`
    );
    // Lazy-load page thumbnails as a preview grid; click jumps into the reader.
    if(first){
      if(first.id) prefetch(`/nhentai/chapter/${encodeURIComponent(first.id)}`);
      try{
        const c = await apiCached(`/nhentai/chapter/${encodeURIComponent(first.id)}`);
        const pages = c.pages||[];
        const readHref = `#/read/nhentai/${encodeURIComponent(first.id)}`;
        const cells = pages.slice(0,24).map(p=>`
          <a class="thumb" href="${readHref}"><div class="poster">${imgTag(p.url,"","hal "+p.index)}<span class="badge">${p.index}</span></div></a>`).join("");
        const more = pages.length>24?`<a class="thumb more" href="${readHref}"><div class="poster"><div class="ph">+${pages.length-24} halaman</div></div></a>`:"";
        const el = document.getElementById("preview");
        if(el) el.innerHTML = pages.length?cells+more:`<div class="empty">Tidak ada halaman.</div>`;
      }catch(e){ const el=document.getElementById("preview"); if(el) el.innerHTML=`<div class="errbox">${h(e.message)}</div>`; }
    } else {
      const el=document.getElementById("preview"); if(el) el.innerHTML=`<div class="empty">Tidak ada halaman.</div>`;
    }
    renderRecommendations("doujin", id);
  }

  async function routeWatch(id){
    shell(`<div id="d">${spinner}</div>`);
    try{
      const e = await apiCached(`/donghua/episode/${encodeURIComponent(id)}`);
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
        `<div id="d">`+
        crumbs([{href:"#/",label:"Home"},{href:"#/browse/donghua",label:"Donghua"},{label:`${e.series_title||"Episode"} - Eps ${e.episode_number}`}])+
        `<div class="row-head"><h2><span class="dot"></span>${h(e.series_title||"Episode")} - Episode ${e.episode_number}</h2></div>`+
        player+bar+nav+(dls?`<div class="row-head"><h2><span class="dot"></span>Unduh</h2></div>${dls}`:"")+
        `</div>`
      );
      document.querySelectorAll(".server-bar .srv").forEach(btn=>{ btn.onclick=()=>{ document.getElementById("player").src=btn.dataset.src; document.querySelectorAll(".server-bar .srv").forEach(b=>b.classList.remove("active")); btn.classList.add("active"); }; });
      // warm adjacent episodes for instant nav, plus recommendations
      if(e.next_id) prefetch(`/donghua/episode/${encodeURIComponent(e.next_id)}`);
      if(e.prev_id) prefetch(`/donghua/episode/${encodeURIComponent(e.prev_id)}`);
      renderRecommendations("donghua", e.series_id);
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  async function routeWatchAnime(id){
    shell(`<div id="d">${spinner}</div>`);
    try{
      const e = await apiCached(`/anime/episode/${encodeURIComponent(id)}`);
      const mirrors = e.mirrors||[];
      const seriesLink = e.series_id?`#/detail/anime/${encodeURIComponent(e.series_id)}`:"#/";
      const epLabel = e.episode_number!=null ? `Episode ${e.episode_number}` : "Episode";
      // Initial player = default embed if present, else nothing (resolved on click).
      const initial = e.default_embed || "";
      const player = `<div class="player-wrap"><div class="frame">${initial?`<iframe id="player" src="${h(initial)}" allowfullscreen allow="autoplay; encrypted-media; fullscreen; picture-in-picture"></iframe>`:`<div class="empty" id="playerEmpty">Pilih server di bawah.</div>`}</div></div>`;
      // Group mirrors by quality.
      const byQ = {};
      mirrors.forEach(m=>{ (byQ[m.quality]=byQ[m.quality]||[]).push(m); });
      const serverBars = Object.entries(byQ).map(([q, list])=>
        `<div class="server-bar"><span class="lbl">${h(q)}:</span>${list.map(m=>`<button class="srv" data-stream="${h(m.stream_id)}">${h(m.name)}</button>`).join("")}</div>`
      ).join("");
      const dls = (e.downloads||[]).map(g=>`<div class="dl-group"><div class="q">${h(g.quality)}${g.size?` &middot; ${h(g.size)}`:""}</div><div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener noreferrer" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div></div>`).join("");
      const nav = `<div class="server-bar" style="margin-top:8px">
        ${e.prev_id?`<a class="btn sm" href="#/watchanime/${encodeURIComponent(e.prev_id)}">&larr; Eps sebelumnya</a>`:""}
        <a class="btn sm" href="${seriesLink}">&#9776; Semua episode</a>
        ${e.next_id?`<a class="btn sm" href="#/watchanime/${encodeURIComponent(e.next_id)}">Eps berikutnya &rarr;</a>`:""}</div>`;
      setView(
        `<div id="d">`+
        crumbs([{href:"#/",label:"Home"},{href:"#/browse/anime",label:"Anime"},{label:`${e.series_title||"Anime"} - ${epLabel}`}])+
        `<div class="row-head"><h2><span class="dot"></span>${h(e.series_title||"Anime")} - ${epLabel}</h2></div>`+
        player+
        `<div class="server-note">Server streaming dari pihak ketiga. Jika satu server gagal, coba server lain.</div>`+
        serverBars+nav+(dls?`<div class="row-head"><h2><span class="dot"></span>Unduh</h2></div>${dls}`:"")+
        `</div>`
      );
      // Resolve a mirror token to an embed URL, then swap the iframe.
      document.querySelectorAll(".server-bar .srv").forEach(btn=>{
        btn.onclick = async ()=>{
          document.querySelectorAll(".server-bar .srv").forEach(b=>b.classList.remove("active"));
          btn.classList.add("active");
          const frame = document.querySelector(".player-wrap .frame");
          frame.innerHTML = `<div class="empty">${spinner}</div>`;
          try{
            const r = await api(`/anime-stream?${qs({id: btn.dataset.stream})}`);
            frame.innerHTML = `<iframe id="player" src="${h(r.url)}" allowfullscreen allow="autoplay; encrypted-media; fullscreen; picture-in-picture"></iframe>`;
          }catch(err){
            frame.innerHTML = `<div class="empty">Gagal memuat server. Coba server lain.</div>`;
          }
        };
      });
      if(e.next_id) prefetch(`/anime/episode/${encodeURIComponent(e.next_id)}`);
      if(e.prev_id) prefetch(`/anime/episode/${encodeURIComponent(e.prev_id)}`);
      renderRecommendations("anime", e.series_id);
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  async function routeRead(kind, id){
    shell(`<div id="d">${spinner}</div>`);
    try{
      if(kind==="novel") return renderNovelChapter(id);
      const ep = kind==="nhentai"?"nhentai/chapter":"manga/chapter";
      const c = await apiCached(`/${ep}/${encodeURIComponent(id)}`);
      const pages = c.pages||[];
      // Pages render at natural width (no forced ratio); the reader column is
      // capped for readability and can go fullscreen so the navbar etc. don't
      // get in the way.
      const imgs = pages.map(p=>`<img loading="lazy" referrerpolicy="no-referrer" src="${h(p.url)}" alt="page ${p.index}" onerror="this.style.opacity=.25">`).join("");
      const title = `${h(c.series_title||"Baca")} ${c.chapter_number?`&middot; Ch ${c.chapter_number}`:""}`;
      const nav = `
        ${c.prev_id?`<a class="btn sm" href="#/read/${kind}/${encodeURIComponent(c.prev_id)}">&larr; Sebelumnya</a>`:""}
        ${c.series_id?`<a class="btn sm" href="#/detail/${kind==="nhentai"?"doujin":"manga"}/${encodeURIComponent(c.series_id)}">&#9776; Daftar</a>`:""}
        ${c.next_id?`<a class="btn sm" href="#/read/${kind}/${encodeURIComponent(c.next_id)}">Berikutnya &rarr;</a>`:""}`;
      const navTrim = nav.trim();
      setView(
        `<div class="reader-shell" id="readerShell">
           <div class="reader-bar">
             <div class="reader-title">${title}</div>
             <button class="btn sm" id="fsBtn">${I.expand} Layar penuh</button>
           </div>
           <div class="reader" id="readerPages">${pages.length?imgs:`<div class="empty">Tidak ada halaman.</div>`}</div>
           ${adSlot("reader")}
           ${navTrim?`<div class="reader-nav">${nav}</div>`:""}
         </div>`
      );
      // fullscreen toggle
      const shellEl = document.getElementById("readerShell");
      const fsBtn = document.getElementById("fsBtn");
      if(fsBtn && shellEl){
        const sync = ()=>{ const on = document.fullscreenElement===shellEl; fsBtn.innerHTML = on?`${I.compress} Keluar`:`${I.expand} Layar penuh`; };
        fsBtn.onclick = ()=>{ if(document.fullscreenElement===shellEl){ document.exitFullscreen&&document.exitFullscreen(); } else { shellEl.requestFullscreen&&shellEl.requestFullscreen().catch(()=>{}); } };
        document.addEventListener("fullscreenchange", sync);
      }
      // warm next chapter for instant paging
      if(c.next_id) prefetch(`/${ep}/${encodeURIComponent(c.next_id)}`);
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  async function renderNovelChapter(id){
    const c = await apiCached(`/novel/chapter/${encodeURIComponent(id)}`);
    const paras = (c.body||"").split(/\n{2,}/).map(s=>s.trim()).filter(Boolean).map(p=>`<p>${h(p)}</p>`).join("");
    const nav = `<div class="reader-nav">
      ${c.prev_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.prev_id)}">&larr; Sebelumnya</a>`:""}
      ${c.series_id?`<a class="btn sm" href="#/detail/novel/${encodeURIComponent(c.series_id)}">&#9776; Daftar bab</a>`:""}
      ${c.next_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.next_id)}">Berikutnya &rarr;</a>`:""}</div>`;
    setView(`<div class="row-head"><h2><span class="dot"></span>${h(c.series_title||"Novel")} &middot; Bab ${c.chapter_number}</h2></div>`+
      (c.chapter_title?`<p style="color:var(--muted);margin-top:-8px">${h(c.chapter_title)}</p>`:"")+
      `<div class="novel-body">${paras||"<p>(kosong)</p>"}</div>`+nav);
    if(c.next_id) prefetch(`/novel/chapter/${encodeURIComponent(c.next_id)}`);
  }

  // route dispatch is defined in part 2 (appended)
  window.__apiku = { shell, setView, viewEl, h, qs, api, apiRaw, apiCached, prefetch, spinner, go, I,
    routeHome, routeBrowse, routeSearch, routeDetail, routeWatch, routeWatchAnime, routeRead };
})();

// ===========================================================================
// Docs + Explorer + Router
// ===========================================================================
(function () {
  "use strict";
  const A = window.__apiku;
  const { shell, setView, h, apiRaw, I, go,
    routeHome, routeBrowse, routeSearch, routeDetail, routeWatch, routeWatchAnime, routeRead } = A;

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

        <h2>Informasi</h2>
        <p>Dikembangkan oleh <a href="https://github.com/risqinf" target="_blank" rel="noopener"><b>@risqinf</b></a>. Lihat kode sumber &amp; kontribusi di <a href="https://github.com/risqinf/apiku" target="_blank" rel="noopener">GitHub</a>.</p>
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
  // Grouped preset endpoints: [group, [[label, path], ...]]
  const EXP_GROUPS = [
    ["Umum", [
      ["Info server", "/api/v1/info"],
      ["Health check", "/api/v1/health"],
    ]],
    ["Pencarian", [
      ["Cari semua sumber", "/api/v1/search?q=one+piece&source=all&page=1"],
      ["Cari komik", "/api/v1/search?q=one+piece&source=manga&page=1"],
      ["Cari donghua", "/api/v1/search?q=martial&source=donghua&page=1"],
      ["Cari novel", "/api/v1/search?q=martial&source=novel&page=1"],
    ]],
    ["Browse / Feed", [
      ["Donghua terbaru", "/api/v1/browse/anichin?feed=home"],
      ["Komik populer", "/api/v1/browse/mangaball?feed=popular"],
      ["Novel terbaru", "/api/v1/browse/novelid?feed=home"],
      ["Doujin hari ini", "/api/v1/browse/nhentai?feed=popular-today"],
    ]],
    ["Detail (ganti {id})", [
      ["Komik", "/api/v1/manga/{id}?page=1&size=60"],
      ["Bab komik", "/api/v1/manga/chapter/{id}"],
      ["Donghua", "/api/v1/donghua/{id}"],
      ["Episode donghua", "/api/v1/donghua/episode/{id}"],
      ["Novel", "/api/v1/novel/{id}?page=1&size=60"],
      ["Bab novel", "/api/v1/novel/chapter/{id}"],
      ["Doujin", "/api/v1/nhentai/{id}"],
    ]],
  ];

  // Build copy-ready code samples for an arbitrary API path, across languages.
  function codeSamplesForPath(origin, relPath) {
    const full = origin + relPath;
    return {
      curl:
`curl '${full}'

# pretty-print
curl -s '${full}' | jq .`,
      javascript:
`const res = await fetch('${full}');
const json = await res.json();
if (!json.ok) throw new Error(json.error.code + ': ' + json.error.message);
console.log(json.data);`,
      python:
`import requests

r = requests.get('${full}')
r.raise_for_status()
body = r.json()
if not body['ok']:
    raise RuntimeError(body['error']['code'])
print(body['data'])`,
      php:
`<?php
$res  = file_get_contents('${full}');
$json = json_decode($res, true);
if (!$json['ok']) {
    throw new RuntimeException($json['error']['code']);
}
var_dump($json['data']);`,
      go:
`package main

import ("encoding/json"; "fmt"; "io"; "net/http")

func main() {
    resp, _ := http.Get("${full}")
    defer resp.Body.Close()
    body, _ := io.ReadAll(resp.Body)
    var env map[string]any
    json.Unmarshal(body, &env)
    fmt.Println(env["data"])
}`,
      rust:
`// reqwest = { version = "0.12", features = ["json"] }
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let v: serde_json::Value = reqwest::get("${full}")
        .await?.json().await?;
    println!("{:#}", v["data"]);
    Ok(())
}`,
    };
  }

  const EXP_LANGS = [["curl","cURL"],["javascript","JavaScript"],["python","Python"],["php","PHP"],["go","Go"],["rust","Rust"]];

  function routeExplorer() {
    const origin = location.origin;
    const initial = "/api/v1/info";
    shell(`
      <div class="explorer">
        <div class="hero-banner"><h1>API Explorer</h1><p>Uji endpoint <code>/api/v1/*</code> langsung, lihat respons, dan salin contoh kode siap pakai.</p></div>

        <div class="exp-grid">
          <aside class="exp-side">
            <div class="exp-side-title">Endpoint</div>
            ${EXP_GROUPS.map(([g,items])=>`
              <div class="exp-group">
                <div class="exp-group-name">${h(g)}</div>
                ${items.map(([label,path])=>`<button class="exp-ep" data-path="${h(path)}">${h(label)}<code>${h(path.replace(/^\/api\/v1/,""))}</code></button>`).join("")}
              </div>`).join("")}
          </aside>

          <div class="exp-main">
            <div class="exp-panel">
              <div class="exp-bar">
                <span class="exp-method">GET</span>
                <input id="exp-path" type="text" value="${h(initial)}" spellcheck="false" placeholder="/api/v1/...">
                <button class="btn primary" id="exp-send">${I.play} Kirim</button>
              </div>
              <p class="exp-tip">Tip: ganti <code>{id}</code> dengan opaque id dari hasil <a href="#/search/one piece">search</a> atau browse.</p>
            </div>

            <div class="exp-resp">
              <div class="exp-resp-head">
                <div class="exp-meta" id="exp-meta"><span class="pill">siap</span></div>
                <button class="btn sm" id="exp-copy">Salin JSON</button>
              </div>
              <pre class="exp-out" id="exp-out">// Tekan "Kirim" untuk melihat respons.</pre>
            </div>

            <div class="exp-code">
              <div class="exp-code-head">
                <h3>Contoh kode</h3>
                <div class="lang-pills" id="expLangs">
                  ${EXP_LANGS.map(([v,l],i)=>`<button class="${i===0?"active":""}" data-lang="${v}">${l}</button>`).join("")}
                </div>
              </div>
              <div id="expSample"></div>
            </div>
          </div>
        </div>
      </div>
    `);
    const pathInput = document.getElementById("exp-path");
    let curLang = "curl";

    const renderSample = ()=>{
      const raw = pathInput.value.trim();
      const safeRel = raw.startsWith("/api/v1")
        ? raw
        : ("/api/v1" + raw.replace(/^.*\/api\/v1/, "").replace(/^\/?/, "/"));
      const samples = codeSamplesForPath(origin, safeRel);
      document.getElementById("expSample").innerHTML = codeBlock(curLang, samples[curLang]);
      bindCopy();
    };

    const send = async ()=>{
      let path = pathInput.value.trim();
      if(!path) return;
      const rel = path.replace(/^.*\/api\/v1/, "").replace(/^\/?/, "/");
      const meta = document.getElementById("exp-meta");
      const out = document.getElementById("exp-out");
      const btn = document.getElementById("exp-send");
      meta.innerHTML = `<span class="pill">...</span>`;
      out.textContent = "Memuat...";
      btn.disabled = true;
      try {
        const res = await apiRaw("GET", rel);
        const ok2 = res.status >= 200 && res.status < 300;
        const cls = ok2 ? "ok" : "bad";
        meta.innerHTML = `<span class="pill ${cls}">HTTP ${res.status}</span> <span class="pill">${res.ms} ms</span> <span class="pill">${ok2?"sukses":"gagal"}</span>`;
        out.textContent = typeof res.json === "string" ? res.json : JSON.stringify(res.json, null, 2);
      } catch (e) {
        meta.innerHTML = `<span class="pill bad">error</span>`;
        out.textContent = String(e.message || e);
      } finally { btn.disabled = false; }
      renderSample();
    };

    document.getElementById("exp-send").addEventListener("click", send);
    pathInput.addEventListener("input", renderSample);
    pathInput.addEventListener("keydown", (e)=>{ if(e.key==="Enter"){ e.preventDefault(); send(); } });

    // sidebar endpoint buttons
    document.querySelectorAll(".exp-ep").forEach(b => b.addEventListener("click", ()=>{
      document.querySelectorAll(".exp-ep").forEach(x=>x.classList.remove("active"));
      b.classList.add("active");
      pathInput.value = b.dataset.path;
      renderSample();
      send();
    }));

    // language pills
    document.querySelectorAll("#expLangs button").forEach(b => b.addEventListener("click", ()=>{
      document.querySelectorAll("#expLangs button").forEach(x=>x.classList.toggle("active", x===b));
      curLang = b.dataset.lang;
      renderSample();
    }));

    // copy response JSON
    document.getElementById("exp-copy").addEventListener("click", (e)=>{
      const txt = document.getElementById("exp-out").textContent;
      navigator.clipboard.writeText(txt).then(
        ()=>{ e.target.textContent="Tersalin"; setTimeout(()=>e.target.textContent="Salin JSON",1200); },
        ()=>{ e.target.textContent="Gagal"; setTimeout(()=>e.target.textContent="Salin JSON",1200); }
      );
    });

    renderSample();
    send();
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
      case "search":   return routeSearch(parts[1]||"", parts[2], parts[3]);
      case "detail":   return routeDetail(parts[1], parts[2]);
      case "watch":    return routeWatch(parts[1]);
      case "watchanime": return routeWatchAnime(parts[1]);
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
