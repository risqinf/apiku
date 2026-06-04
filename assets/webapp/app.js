// apiku consumer SPA — dependency-free hash router.
// Home / browse / search / detail / watch / read / gallery / docs / explorer.
(function () {
  "use strict";

  const API = "/api/v1";
  const app = document.getElementById("app");
  const CHAPTER_SIZE = 60;

  // ---- Branding (injected by server via window.__BRAND) -------------------
  const BRAND = Object.assign(
    { name: "apiku", tagline: "Stream donghua, read comics & novels, browse cosplay galleries - all in one platform.", logo: "", footer: "", ads: {} },
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

  // ---- Lite mode (very old / low-end devices) -----------------------------
  // Detect weak hardware or a data-saving / reduced-motion preference, then
  // run a stripped-down experience: all CSS animations & transitions off
  // (see [data-lite] rules in app.css) and no background prefetch (saves RAM,
  // CPU and bandwidth on a 2008 PC or a 2015 phone). The user can still force
  // it on/off; the choice is remembered.
  const LITE = (function detectLite(){
    let forced = null;
    try { forced = localStorage.getItem("apiku_lite"); } catch(_){}
    if(forced === "1") return true;
    if(forced === "0") return false;
    const mem = navigator.deviceMemory;            // GiB, where exposed
    const cpu = navigator.hardwareConcurrency;     // logical cores
    const conn = navigator.connection || {};
    const saveData = conn.saveData === true;
    const slowNet = /(^|\b)(2g|slow-2g)$/i.test(conn.effectiveType || "");
    const reduceMotion = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
    const reduceData = window.matchMedia && matchMedia("(prefers-reduced-data: reduce)").matches;
    return (typeof mem === "number" && mem <= 2)
        || (typeof cpu === "number" && cpu <= 2)
        || saveData || slowNet || reduceData || reduceMotion;
  })();
  if(LITE){ try { document.documentElement.setAttribute("data-lite", "1"); } catch(_){} }

  // Persist + apply a manual lite-mode choice, then reload so every render
  // path picks up the new setting cleanly.
  function setLite(on){
    try { localStorage.setItem("apiku_lite", on ? "1" : "0"); } catch(_){}
    location.reload();
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
      s.onerror = ()=>reject(new Error("failed to load player"));
      document.head.appendChild(s);
    });
    return _hlsLoading;
  }

  // Resolve a signed cosplay-video URL -> HLS stream, then attach to <video>.
  async function attachHls(wrap){
    const resolveUrl = wrap.dataset.resolve;
    const idx = wrap.dataset.idx;
    const video = wrap.querySelector("video");
    const clear = ()=>{ const s = document.getElementById(`hls-state-${idx}`); if(s) s.remove(); };
    const fail = (msg, src)=>{
      const state = document.getElementById(`hls-state-${idx}`);
      if(state) state.innerHTML = `<div class="hls-err">${escHtml(msg)}${src?`<br><a class="btn sm" href="${escAttr(src)}" target="_blank" rel="noopener noreferrer">Open directly</a>`:""}</div>`;
    };
    try{
      // resolveUrl is /api/v1/cosplay-video?... ; strip the /api/v1 prefix for api()
      const rel = resolveUrl.replace(/^.*\/api\/v1/, "");
      const res = await api(rel);
      const src = res && res.url;
      if(!src){ fail("Stream not found"); return; }

      // IMPORTANT: prefer hls.js (Media Source Extensions) wherever it works.
      // Android Chrome advertises native HLS via canPlayType() but its native
      // playback is unreliable — it stalls in a load/ended/reload loop stuck at
      // 00:00. hls.js (MSE) plays correctly on Android + desktop. Only fall
      // back to native HLS when MSE isn't available (essentially iOS Safari /
      // WebKit, where hls.js can't run).
      const Hls = await loadHlsJs().catch(()=>null);
      if(Hls && Hls.isSupported()){
        const hls = new Hls({
          // Tuned for mobile: smaller buffers + generous retries so a flaky
          // cell connection recovers instead of giving up.
          maxBufferLength: 20,
          maxMaxBufferLength: 40,
          manifestLoadingMaxRetry: 6,
          manifestLoadingRetryDelay: 800,
          levelLoadingMaxRetry: 6,
          fragLoadingMaxRetry: 8,
          fragLoadingRetryDelay: 800,
        });
        let recoveredMedia = false;
        hls.loadSource(src);
        hls.attachMedia(video);
        hls.on(Hls.Events.MANIFEST_PARSED, clear);
        hls.on(Hls.Events.ERROR, (_e, d)=>{
          if(!d || !d.fatal) return;
          // Recover from transient errors rather than tearing down (the
          // teardown-and-retry was what produced the mobile loop).
          if(d.type === Hls.ErrorTypes.NETWORK_ERROR){
            hls.startLoad();
          } else if(d.type === Hls.ErrorTypes.MEDIA_ERROR){
            if(!recoveredMedia){ recoveredMedia = true; hls.recoverMediaError(); }
            else { hls.destroy(); fail("Failed to play video", src); }
          } else {
            hls.destroy();
            fail("Failed to play video", src);
          }
        });
        // Safety net: drop the spinner once the browser has real frames.
        video.addEventListener("loadeddata", clear, { once:true });
        return;
      }
      if(video.canPlayType("application/vnd.apple.mpegurl")){
        // iOS Safari / WebKit: native HLS is the only option (and works well).
        video.src = src;
        video.addEventListener("loadedmetadata", clear, { once:true });
        video.addEventListener("error", ()=>fail("Failed to play video", src), { once:true });
        return;
      }
      // No HLS support at all.
      fail("Your browser cannot play this video", src);
    }catch(e){ fail(e.message || "Failed to load video"); }
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
    bolt:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2 4 14h7l-1 8 9-12h-7z"/></svg>',
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
    manga:   { label: "Comics",  api: "mangaball",   kind: "manga",   adult: false, icon: I.manga },
    novel:   { label: "Novel",   api: "novelid",     kind: "novel",   adult: false, icon: I.novel },
    cosplay: { label: "Cosplay", api: "cosplaytele", kind: "cosplay", adult: true,  icon: I.cosplay },
    doujin:  { label: "Doujin",  api: "nhentai",     kind: "doujin",  adult: true,  icon: I.doujin },
  };
  const EPISODE_SIZE = 5000; // donghua: fetch the whole episode list at once

  const FEEDS = {
    otakudesu:   [["ongoing","Ongoing"],["complete","Completed"],["action","Action"],["romance","Romance"],["comedy","Comedy"],["fantasy","Fantasy"],["adventure","Adventure"],["drama","Drama"]],
    anichin:     [["home","Latest"],["popular","Popular"],["rating","Rating"],["title","A-Z"]],
    mangaball:   [["home","Featured"],["popular","Popular"],["latest","Latest"],["recommend","Recommended"]],
    novelid:     [["home","All"],["popular","Completed"],["novel-translate","Translated"],["fantasi","Fantasy"],["romantis","Romance"],["aksi","Action"],["horror","Horror"]],
    cosplaytele: [["home","Latest"],["popular","Popular"]],
    nhentai:     [["popular-today","Today"],["popular-week","This Week"],["popular","All Time"],["home","Latest"]],
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
  // Synchronous cache peek: returns warmed data immediately (or undefined).
  // Lets a route render instantly with no spinner when the pointer/touch
  // prefetch already fetched what the user is opening.
  function peek(path){ return _cache.get(path); }
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
    if(LITE) return; // skip speculative work on weak devices
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
      ["manga", "#/browse/manga", "Comics", I.manga, false],
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
        <h3 id="ageTitle">Adult Content &middot; 18+</h3>
        <p>The <b>Cosplay</b> and <b>Doujin</b> sections contain adult material. By continuing, you confirm that you are at least 18 years old and agree to view this content.</p>
        <div class="modal-actions">
          <button class="btn" data-act="no">Cancel</button>
          <button class="btn primary" data-act="yes">Yes, I'm 18+</button>
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

  // ---- Persistent shell ---------------------------------------------------
  // The chrome (drawer, header, footer) is built ONCE. Navigation then only
  // swaps the <main id="view"> content instead of tearing down and rebuilding
  // the whole page (and re-wiring every listener) on each route change. That
  // rebuild was what made every navigation flash like a full reload.
  let _chromeBuilt = false;
  let _navAdult = null; // adult state the nav markup was last built with

  // Build the desktop nav markup (provider links + the "More" tools menu).
  function deskNavMarkup(links, tools, seg){
    return links.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")+
      `<div class="navmore">
          <button class="navmore-btn ${tools.some(t=>t[0]===seg)?"active":""}" id="moreBtn" aria-haspopup="true" aria-expanded="false">${I.dots}</button>
          <div class="navmore-menu" id="moreMenu">
            ${tools.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")}
            <a href="/tester">${I.explorer}<span>Dev Console</span></a>
            <div class="dsep"></div>
            <button class="switch ${LITE?"on":""}" id="liteBtnDesk" role="switch" aria-checked="${LITE}">
              <span class="switch-label">${I.bolt||""}<span>Lite Mode</span></span>
              <span class="switch-track"><span class="switch-thumb"></span></span>
            </button>
          </div>
        </div>`;
  }

  // Refresh the nav: rebuild link markup only when the adult set changed
  // (first paint or after the 18+ toggle); otherwise just move the active
  // highlight, which is cheap and never flashes.
  function renderNav(){
    const seg = activeNavSeg();
    const links = navLinks();
    const tools = toolLinks();
    const adult = adultOn();
    const desk = document.getElementById("deskNav");
    const dnav = document.getElementById("drawerNav");
    if(_navAdult !== adult){
      _navAdult = adult;
      if(desk) desk.innerHTML = deskNavMarkup(links, tools, seg);
      if(dnav){
        dnav.innerHTML =
          links.map(([s,href,label,ico,ad])=>navItem(s,href,label,ico,ad,seg)).join("")+
          `<div class="dsep"></div>`+
          tools.map(([s,href,label,ico,ad])=>navItem(s,href,label,ico,ad,seg)).join("")+
          `<a href="/tester"><span>${I.explorer}</span><span>Dev Console</span></a>`;
      }
      wireMore();
    }
    document.querySelectorAll("#deskNav a[data-seg], #drawerNav a[data-seg]").forEach(a=>{
      a.classList.toggle("active", a.dataset.seg===seg);
    });
    const moreBtn = document.getElementById("moreBtn");
    if(moreBtn) moreBtn.classList.toggle("active", tools.some(t=>t[0]===seg));
  }

  // Wire the desktop "More" overflow menu (re-run whenever deskNav is rebuilt).
  function wireMore(){
    const moreBtn = document.getElementById("moreBtn");
    const moreMenu = document.getElementById("moreMenu");
    if(!moreBtn || !moreMenu) return;
    const closeMore = ()=>{ moreMenu.classList.remove("open"); moreBtn.setAttribute("aria-expanded","false"); document.removeEventListener("click", onDocClick); };
    const onDocClick = (e)=>{ if(!moreMenu.contains(e.target) && e.target!==moreBtn && !moreBtn.contains(e.target)) closeMore(); };
    moreBtn.onclick = (e)=>{
      e.stopPropagation();
      const open = moreMenu.classList.toggle("open");
      moreBtn.setAttribute("aria-expanded", String(open));
      if(open) setTimeout(()=>document.addEventListener("click", onDocClick),0);
    };
    moreMenu.querySelectorAll("a").forEach(a => a.addEventListener("click", closeMore));
    // Wire lite mode toggle inside the desktop more menu
    const liteBtnDesk = document.getElementById("liteBtnDesk");
    if(liteBtnDesk) liteBtnDesk.onclick = (e)=>{ e.stopPropagation(); setLite(!LITE); };
  }

  // Build the chrome once and wire all the global (route-independent) listeners
  // against stable element IDs, so navigation never has to re-attach them.
  function buildChrome(){
    const themeIco = store.theme === "dark" ? I.sun : I.moon;
    app.innerHTML = `
      <div class="drawer-scrim" id="scrim"></div>
      <aside class="drawer" id="drawer">
        <div class="dhead">
          <span class="brand">${brandMark()}</span>
          <button class="icon-btn" id="drawerClose">${I.close}</button>
        </div>
        <nav id="drawerNav"></nav>
        <div class="dsep"></div>
        <div class="drow">
          <button class="switch ${adultOn()?"on":""}" id="adultBtnD" role="switch" aria-checked="${adultOn()}">
            <span class="switch-label"><span class="b18">18+</span> Adult content</span>
            <span class="switch-track"><span class="switch-thumb"></span></span>
          </button>
          <button class="switch ${store.theme==="dark"?"on":""}" id="themeBtnD" role="switch" aria-checked="${store.theme==="dark"}">
            <span class="switch-label">${themeIco}<span>Dark mode</span></span>
            <span class="switch-track"><span class="switch-thumb"></span></span>
          </button>
          <button class="switch ${LITE?"on":""}" id="liteBtnD" role="switch" aria-checked="${LITE}">
            <span class="switch-label">${I.bolt||""}<span>Lite mode</span></span>
            <span class="switch-track"><span class="switch-thumb"></span></span>
          </button>
        </div>
      </aside>

      <header class="hdr">
        <button class="icon-btn hamburger" id="hamburger">${I.menu}</button>
        <a class="brand" href="#/">${brandMark()}</a>
        <nav class="desktop" id="deskNav"></nav>
        <div class="spacer"></div>
        <form class="searchbox" id="searchform">
          ${I.search}
          <input id="searchinput" type="search" placeholder="Search titles..." autocomplete="off">
        </form>
        <button class="icon-btn ${adultOn()?"on":""}" id="adultBtn" title="18+ Content">18+</button>
        <button class="icon-btn" id="themeBtn" title="Toggle theme">${themeIco}</button>
      </header>
      <main id="view"></main>
      <footer>${footerHtml()}</footer>`;

    // search
    const form = document.getElementById("searchform");
    const input = document.getElementById("searchinput");
    form.addEventListener("submit", (e)=>{ e.preventDefault(); const q=input.value.trim(); if(q) go(`#/search/${encodeURIComponent(q)}`); });

    // theme — switch live (no re-render, so it never flickers)
    const themeBtn = document.getElementById("themeBtn");
    const themeBtnD = document.getElementById("themeBtnD");
    const toggleTheme = ()=>{
      store.theme = store.theme==="dark"?"light":"dark";
      applyTheme();
      if(themeBtn) themeBtn.innerHTML = store.theme==="dark" ? I.sun : I.moon;
      if(themeBtnD){
        themeBtnD.classList.toggle("on", store.theme==="dark");
        themeBtnD.setAttribute("aria-checked", String(store.theme==="dark"));
        const lab = themeBtnD.querySelector(".switch-label");
        if(lab) lab.innerHTML = `${store.theme==="dark"?I.sun:I.moon}<span>Dark mode</span>`;
      }
    };
    if(themeBtn) themeBtn.onclick = toggleTheme;
    if(themeBtnD) themeBtnD.onclick = toggleTheme;

    // adult — flip the switch, refresh nav + current content
    const adultBtn = document.getElementById("adultBtn");
    const adultBtnD = document.getElementById("adultBtnD");
    const enableAdult = ()=>{
      store.adult = true;
      if(adultBtnD){ adultBtnD.classList.add("on"); adultBtnD.setAttribute("aria-checked","true"); }
      if(adultBtn) adultBtn.classList.add("on");
      window.__apiku.router();
    };
    const toggleAdult = ()=>{
      if(!adultOn()){ showAgeModal(enableAdult); return; }
      store.adult = false;
      if(adultBtnD){ adultBtnD.classList.remove("on"); adultBtnD.setAttribute("aria-checked","false"); }
      if(adultBtn) adultBtn.classList.remove("on");
      const parts = location.hash.replace(/^#\//,"").split("/").map(decodeURIComponent);
      const inAdult = (parts[0]==="browse" && (parts[1]==="cosplay"||parts[1]==="doujin"))
        || (parts[0]==="detail" && (parts[1]==="cosplay"||parts[1]==="doujin"))
        || (parts[0]==="read" && parts[1]==="nhentai");
      if(inAdult){ go("#/"); return; }
      window.__apiku.router();
    };
    if(adultBtn) adultBtn.onclick = toggleAdult;
    if(adultBtnD) adultBtnD.onclick = toggleAdult;

    // lite mode — toggle + persist, then reload to re-render cleanly
    const liteBtnD = document.getElementById("liteBtnD");
    if(liteBtnD) liteBtnD.onclick = ()=> setLite(!LITE);

    // drawer (delegated link-close so it survives nav re-renders)
    const drawer = document.getElementById("drawer");
    const scrim = document.getElementById("scrim");
    const openDrawer = ()=>{ drawer.classList.add("open"); scrim.classList.add("open"); };
    const closeDrawer = ()=>{ drawer.classList.remove("open"); scrim.classList.remove("open"); };
    document.getElementById("hamburger").onclick = openDrawer;
    document.getElementById("drawerClose").onclick = closeDrawer;
    scrim.onclick = closeDrawer;
    drawer.addEventListener("click", (e)=>{ if(e.target.closest("nav a")) closeDrawer(); });
  }

  // Swap the page content. Builds the chrome on first call, then only updates
  // <main id="view"> and the nav highlight — so navigation feels instant.
  function shell(inner){
    if(!_chromeBuilt){ buildChrome(); _chromeBuilt = true; _navAdult = null; }
    renderNav();
    const input = document.getElementById("searchinput");
    if(input){
      const m = location.hash.match(/^#\/search\/([^/]+)/);
      input.value = m ? decodeURIComponent(m[1]) : "";
    }
    setView(inner);
    // Quick GPU-only fade so the swap reads as smooth, not a hard cut. Cache-
    // first routes fill #view synchronously (before paint), so the fade plays
    // on the final content. Skipped in lite mode to stay perfectly still.
    if(!LITE){
      const v = viewEl();
      if(v){ v.classList.remove("view-in"); void v.offsetWidth; v.classList.add("view-in"); }
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
  const grid = (items) => (!items||!items.length) ? `<div class="empty">No results found.</div>` : `<div class="grid">${items.map(cardHtml).join("")}</div>`;

  // ---- Pagination (nhentai-style numbered pager) --------------------------
  // Builds `‹ 1 2 … 7 8 [9] 10 11 … 42 ›` given the current page, an optional
  // known total page count, and whether a next page exists. When `totalPages`
  // is unknown (0/null) we still render Prev/Next plus the current page so deep
  // paging works even when the upstream never tells us the count.
  //
  // Two navigation modes:
  //   - route mode: pass `hrefFor(n) -> "#/..."`; buttons are <a> links.
  //   - in-place mode: pass `opts.jsNav=true`; buttons become
  //     `<button class="page-btn" data-pg="N">` so the caller can re-render
  //     without a route change (used by the chapter list to keep the chosen
  //     language). Wire it with `wirePagerJs(container, onPage)`.
  //
  // Returns "" when there is only a single page (no count, no arrows shown).
  function buildPageList(current, totalPages){
    const out = [];
    const last = totalPages;
    const win = 2; // pages to show on each side of the current page
    const push = (n)=>{ if(!out.includes(n)) out.push(n); };
    push(1);
    for(let n = current - win; n <= current + win; n++){ if(n>=1 && n<=last) push(n); }
    push(last);
    out.sort((a,b)=>a-b);
    // Insert ellipsis markers (null) where there are gaps.
    const withGaps = [];
    for(let i=0;i<out.length;i++){
      if(i>0 && out[i] - out[i-1] > 1) withGaps.push(null);
      withGaps.push(out[i]);
    }
    return withGaps;
  }

  function pagerHtml(current, totalPages, hasNext, hrefFor, opts){
    opts = opts || {};
    current = Math.max(1, current|0);
    const known = totalPages && totalPages > 0;
    const last = known ? totalPages : 0;

    // Single page → no pager at all (hide count + arrows).
    if(known){ if(last <= 1) return ""; }
    else if(current <= 1 && !hasNext){ return ""; }

    const jsNav = !!opts.jsNav;
    const btn = (label, page, o={})=>{
      const cls = `page-btn${o.active?" active":""}${o.disabled?" disabled":""}`;
      if(o.disabled || page==null) return `<span class="${cls}">${label}</span>`;
      return jsNav
        ? `<button type="button" class="${cls}" data-pg="${page}">${label}</button>`
        : `<a class="${cls}" href="${hrefFor(page)}">${label}</a>`;
    };
    let inner = "";
    // Prev
    inner += btn("&larr;", current-1, {disabled: current<=1});
    if(known){
      for(const n of buildPageList(current, last)){
        if(n===null){ inner += `<span class="page-ellipsis">&hellip;</span>`; }
        else inner += btn(String(n), n, {active: n===current});
      }
    } else {
      // Unknown total: show a few pages around the current one.
      if(current>1) inner += btn("1", 1, {active: current===1});
      if(current>3) inner += `<span class="page-ellipsis">&hellip;</span>`;
      if(current>2) inner += btn(String(current-1), current-1);
      inner += btn(String(current), current, {active:true});
      if(hasNext) inner += btn(String(current+1), current+1);
    }
    // Next
    const nextDisabled = known ? current>=last : !hasNext;
    inner += btn("&rarr;", current+1, {disabled: nextDisabled});

    const status = known ? `Page ${current} of ${last}` : `Page ${current}`;
    return `<div class="pager"><div class="page-nav">${inner}</div><span class="page-status">${status}</span></div>`;
  }

  // Wire an in-place (jsNav) pager: every enabled page button calls `onPage(n)`.
  function wirePagerJs(root, onPage){
    if(!root) return;
    root.querySelectorAll(".page-btn[data-pg]").forEach(b=>{
      b.addEventListener("click", ()=>{ const n = parseInt(b.dataset.pg,10); if(n>=1) onPage(n); });
    });
  }

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
  // On touch, pointerenter only fires after the tap lands. Warm on pointerdown
  // too so the cache is primed the instant a finger presses a card, making the
  // open feel immediate on phones.
  document.addEventListener("pointerdown", (e)=>{
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
      { title:"Latest Donghua",  prov:"anichin",     feed:"home",          seg:"donghua" },
      { title:"Popular Comics",  prov:"mangaball",   feed:"popular",       seg:"manga" },
      { title:"Latest Novels",   prov:"novelid",     feed:"home",          seg:"novel" },
      { title:"Latest Cosplay",  prov:"cosplaytele", feed:"home",          seg:"cosplay", adult:true },
      { title:"Today's Doujin",  prov:"nhentai",     feed:"popular-today", seg:"doujin",  adult:true },
    ].filter(s=>!s.adult||adultOn());
    rows.innerHTML = sections.map((s,i)=>`
      <div class="row-head"><h2><span class="dot"></span>${h(s.title)}</h2><a class="more" href="#/browse/${s.seg}">View all ${I.arrow}</a></div>
      <div id="row-${i}">${skelGrid(6)}</div>`).join("");
    sections.forEach(async (s,i)=>{
      try { const data=await apiCached(`/browse/${s.prov}?${qs({feed:s.feed})}`); document.getElementById(`row-${i}`).innerHTML=grid((data.items||[]).slice(0,12)); }
      catch(e){ const el=document.getElementById(`row-${i}`); if(el) el.innerHTML=`<div class="errbox">Failed to load.</div>`; }
    });
  }

  async function routeBrowse(seg, feed, page){
    const prov = PROVIDERS[seg];
    if(!prov) return routeHome();
    if(prov.adult && !adultOn()) return routeHome();
    page = parseInt(page||"1",10);
    const feeds = FEEDS[prov.api] || [["home","All"]];
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
      const totalPages = data.total_pages || 0;
      const hasNext = data.has_next != null ? data.has_next : (data.items && data.items.length>0);
      document.getElementById("pager").innerHTML =
        pagerHtml(page, totalPages, hasNext, (n)=>`#/browse/${seg}/${feed}/${n}`);
      // warm the next page so paging feels instant
      if(hasNext) prefetch(`/browse/${prov.api}?${qs({feed,page:page+1})}`);
    }catch(e){ document.getElementById("list").innerHTML=`<div class="errbox">${h(e.message)}</div>`; }
  }

  // Search with a source filter + numbered pagination.
  // Route: #/search/{query}/{src}/{page}
  //   - src defaults to "all"; page defaults to 1
  //   - legacy "lock" token (from cosplayer/tag pills) is treated as page 1
  async function routeSearch(query, src, page){
    src = src || "all";
    if(page === "lock") page = 1;            // legacy precise-pill route
    page = parseInt(page||"1",10); if(!page||page<1) page = 1;

    shell(`
      <div class="row-head"><h2><span class="dot"></span>Results: &ldquo;${h(query)}&rdquo;</h2></div>
      <div class="chips" id="srcChips"></div>
      <div id="list">${skelGrid(12)}</div>
      <div id="pager"></div>
    `);
    const allSources = [["all","All"],["anime","Anime"],["donghua","Donghua"],["manga","Comics"],["novel","Novel"]];
    if(adultOn()){ allSources.push(["cosplay","Cosplay"]); allSources.push(["doujin","Doujin"]); }
    if(src!=="all" && !providerVisible(src)) src = "all";

    // Build the route hash for a given source (page 1) and for a page number.
    const srcHref = (s)=> s==="all"
      ? `#/search/${encodeURIComponent(query)}/all/1`
      : `#/search/${encodeURIComponent(query)}/${s}/1`;
    const pageBase = src==="all"
      ? `#/search/${encodeURIComponent(query)}/all`
      : `#/search/${encodeURIComponent(query)}/${src}`;

    try{
      const data = await apiCached(`/search?${qs({q:query, source:src, page})}`);
      const items = (data.items||[]).filter(it=>providerVisible(it.kind));
      const totalPages = data.total_pages || 0;
      const hasNext = data.has_next != null ? data.has_next : (items.length>0);

      // Per-kind counts for the current page (used to label/trim chips).
      const counts = {};
      items.forEach(it=>{ counts[it.kind]=(counts[it.kind]||0)+1; });

      // "All" view shows every source the user can see; a specific-source view
      // shows just "All" + the active source so you can jump back.
      const chipList = src==="all"
        ? allSources.filter(([v])=> v==="all" || (counts[v]||0) > 0 || items.length===0)
        : [["all","All"], (allSources.find(([v])=>v===src) || [src, src])];

      const chips = document.getElementById("srcChips");
      chips.innerHTML = chipList.map(([v,l])=>{
        const active = v===src;
        let cnt = "";
        if(v==="all"){ if(src==="all") cnt = items.length; }
        else if(counts[v]!=null) cnt = counts[v];
        const cntHtml = cnt!=="" ? ` <span class="cnt">${cnt}</span>` : "";
        return `<a class="chip ${active?"active":""}" href="${srcHref(v)}">${h(l)}${cntHtml}</a>`;
      }).join("");

      document.getElementById("list").innerHTML = grid(items);
      document.getElementById("pager").innerHTML =
        pagerHtml(page, totalPages, hasNext, (n)=>`${pageBase}/${n}`);

      // Warm the next page for instant paging.
      if(hasNext) prefetch(`/search?${qs({q:query, source:src, page:page+1})}`);
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
    const ep = DETAIL_EP[kind];
    if(!ep){ shell(`<div id="d"></div>`); return setD(`<div class="errbox">Unknown type: ${h(kind)}</div>`); }

    // Cache-first: if a hover/touch prefetch already warmed this detail, render
    // it synchronously with no spinner so the page appears instantly.
    if(kind==="anime"){
      const cached = peek(`/anime/${encodeURIComponent(id)}`);
      if(cached){ shell(`<div id="d"></div>`); return renderAnimeSeries(id, cached); }
    } else if(kind!=="cosplay" && kind!=="doujin"){
      const size = kind==="donghua" ? EPISODE_SIZE : CHAPTER_SIZE;
      const cached = peek(`/${ep}/${encodeURIComponent(id)}?${qs({page:1,size})}`);
      if(cached){
        shell(`<div id="d"></div>`);
        return kind==="donghua" ? renderDonghuaSeries(id, cached) : renderReadableSeries(kind, id, cached, 1);
      }
    } else {
      const cached = peek(kind==="cosplay" ? `/cosplay/${encodeURIComponent(id)}` : `/nhentai/${encodeURIComponent(id)}`);
      if(cached){ shell(`<div id="d"></div>`); return kind==="cosplay" ? renderCosplay(id) : renderDoujin(id); }
    }

    shell(`<div id="d">${spinner}</div>`);
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
      first?`<a class="btn primary" href="#/watchanime/${encodeURIComponent(first.id)}">${I.play} Watch Ep ${first.number??1}</a>`:"",
      (last && last!==first)?`<a class="btn" href="#/watchanime/${encodeURIComponent(last.id)}">Latest ep ${last.number??""}</a>`:"",
      ...(data.batch||[]).slice(0,1).map(b=>`<a class="btn sm" href="#/watchanime/${encodeURIComponent(b.id)}">${I.book} Batch</a>`),
    ].join("");
    const syn = data.synopsis || (data.japanese_title?`Japanese title: ${data.japanese_title}`:"");
    const epControls = eps.length>24
      ? `<div class="ep-tools"><input id="epSearch" type="search" inputmode="numeric" placeholder="Jump to episode..." autocomplete="off"></div>`
      : "";
    const epGrid = eps.length
      ? `<div class="ep-list" id="epList">${eps.map(e=>`<button class="ep-btn center" data-ep="${e.number??""}" data-go="#/watchanime/${encodeURIComponent(e.id)}">Ep ${e.number??(e.title||"?")}</button>`).join("")}</div>`
      : `<div class="empty">No episodes available.</div>`;

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
    host.innerHTML = `<div class="row-head"><h2><span class="dot"></span>Recommendations</h2><a class="more" href="#/browse/${kind}">View all ${I.arrow}</a></div><div id="recRow">${skelGrid(6)}</div>`;
    const d = document.getElementById("d");
    if(d) d.appendChild(host);
    try{
      const feed = (FEEDS[prov.api] && FEEDS[prov.api][1]) ? FEEDS[prov.api][1][0] : "popular";
      const data = await apiCached(`/browse/${prov.api}?${qs({feed})}`);
      let items = (data.items||[]).filter(it=>it.id!==excludeId).slice(0,12);
      const row = document.getElementById("recRow");
      if(row) row.innerHTML = grid(items);
    }catch(e){ const row=document.getElementById("recRow"); if(row) row.innerHTML=`<div class="empty">No recommendations.</div>`; }
  }

  function renderDonghuaSeries(id, data){
    const eps = data.episodes||[];
    const facts = [
      data.status?`<span class="pill ok">${h(data.status)}</span>`:"",
      `<span class="pill">${data.episode_count} episodes</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const first = eps[0];
    const last = eps[eps.length-1];
    const actions = [
      first?`<a class="btn primary" href="#/watch/${encodeURIComponent(first.id)}">${I.play} Watch Ep ${first.number}</a>`:"",
      (last && last!==first)?`<a class="btn" href="#/watch/${encodeURIComponent(last.id)}">Latest ep ${last.number}</a>`:"",
    ].join("");

    // Episode access: a jump-search (filters across ALL episodes) plus a
    // numbered pager that slices the fully-loaded list client-side. Paging is
    // instant (no refetch); the pager hides itself when everything fits on one
    // page.
    const EP_PAGE = 120;
    const epControls = eps.length>24
      ? `<div class="ep-tools"><input id="epSearch" type="search" inputmode="numeric" placeholder="Jump to episode... (e.g. 120)" autocomplete="off"></div>`
      : "";

    const epBtn = (e)=>`<button class="ep-btn center" data-ep="${e.number}" data-go="#/watch/${encodeURIComponent(e.id)}">Ep ${e.number}</button>`;

    setD(
      heroHtml("donghua","Donghua",data,facts,actions,data.synopsis,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>Episode <span class="cnt-badge">${eps.length}</span></h2></div>${epControls}
       <div class="ep-pager-top"></div>
       <div class="ep-list" id="epList">${eps.length?"":`<div class="empty">No episodes available.</div>`}</div>
       <div class="ep-pager-bot"></div>`
    );

    const listEl = document.getElementById("epList");
    const topPager = document.querySelector("#d .ep-pager-top");
    const botPager = document.querySelector("#d .ep-pager-bot");
    const totalEpPages = Math.max(1, Math.ceil(eps.length / EP_PAGE));
    let epPage = 1;
    let filtering = false;

    // Render one page-slice of the episode grid + its numbered pagers.
    const renderEpPage = (p)=>{
      epPage = Math.min(Math.max(1, p), totalEpPages);
      const start = (epPage-1)*EP_PAGE;
      const slice = eps.slice(start, start+EP_PAGE);
      listEl.innerHTML = slice.map(epBtn).join("");
      const pgr = pagerHtml(epPage, totalEpPages, epPage<totalEpPages, null, { jsNav:true });
      topPager.innerHTML = pgr;
      botPager.innerHTML = pgr;
      wirePagerJs(topPager, renderEpPage);
      wirePagerJs(botPager, (n)=>{ renderEpPage(n); listEl.scrollIntoView({block:"start", behavior:"instant"}); });
    };
    if(eps.length) renderEpPage(1);

    // wire episode search/jump — searches across ALL episodes; while a query
    // is active we show every match on one page and hide the pager.
    const epSearch = document.getElementById("epSearch");
    if(epSearch){
      epSearch.addEventListener("input", ()=>{
        const q = epSearch.value.trim().toLowerCase();
        if(!q){
          filtering = false;
          topPager.style.display = botPager.style.display = "";
          renderEpPage(epPage);
          return;
        }
        filtering = true;
        topPager.style.display = botPager.style.display = "none";
        const matches = eps.filter(e=>String(e.number??"").toLowerCase().includes(q));
        listEl.innerHTML = matches.length
          ? matches.map(epBtn).join("")
          : `<div class="empty">No matching episode.</div>`;
      });
    }

    // warm the first episode + recommendations
    if(first) prefetch(`/donghua/episode/${encodeURIComponent(first.id)}`);
    renderRecommendations("donghua", id);
  }

  // Manga/novel — with LANGUAGE GROUPING for manga (translations).
  async function renderReadableSeries(kind, id, data, page, activeLang){
    const label = kind==="manga"?"Comics":"Novel";
    const chs = data.chapters||[];
    const totalPages = data.chapter_total_pages||1;
    const facts = [
      data.status?`<span class="pill ok">${h(data.status)}</span>`:"",
      data.author?`<span class="pill">&#9997; ${h(data.author)}</span>`:"",
      data.rating?`<span class="pill">&#9733; ${h(data.rating)}</span>`:"",
      `<span class="pill">${data.chapter_count} chapters</span>`,
      ...(data.genres||[]).slice(0,5).map(g=>`<span class="pill">${h(g)}</span>`),
    ].join("");
    const readPath = kind==="manga"?"read/manga":"read/novel";

    // --- language detection (manga translations) ---
    let languages = [];
    if (kind === "manga") {
      const set = new Map(); // lang -> count
      chs.forEach(c => {
        const tr = c.translations || [];
        if (tr.length) tr.forEach(t => { const l = t.language || "Other"; set.set(l, (set.get(l)||0)+1); });
      });
      languages = [...set.entries()].sort((a,b)=>b[1]-a[1]); // [lang, count]
    }
    // Preserve the chosen language across pager reloads; default to "all".
    const validLang = activeLang && (activeLang==="__all__" || languages.some(([l])=>l===activeLang));
    const langState = { active: validLang ? activeLang : "__all__" };

    // chapters actually available in the active language (for first-read + count)
    const chsInLang = (lang)=> (kind!=="manga"||lang==="__all__")
      ? chs
      : chs.filter(c => (c.translations||[]).some(t => (t.language||"Other")===lang));

    const firstList = chsInLang(langState.active);
    const first = firstList[0];
    const firstReadId = first
      ? (kind==="manga" && langState.active!=="__all__"
          ? ((first.translations||[]).find(t=>(t.language||"Other")===langState.active)||first).id
          : first.id)
      : null;
    const actions = firstReadId?`<a class="btn primary" href="#/${readPath}/${encodeURIComponent(firstReadId)}">${I.book} Start Reading</a>`:"";
    const syn = data.description || data.synopsis;

    const langTabs = (kind==="manga" && languages.length>1)
      ? `<div class="lang-tabs" id="langTabs">
          <button class="lang-tab ${langState.active==="__all__"?"active":""}" data-lang="__all__">All <span class="cnt">${chs.length}</span></button>
          ${languages.map(([l,c])=>`<button class="lang-tab ${langState.active===l?"active":""}" data-lang="${h(l)}">${h(l)} <span class="cnt">${c}</span></button>`).join("")}
         </div>`
      : "";

    function chapterRowsFor(lang){
      return chs.map(c=>{
        if (kind === "manga" && lang !== "__all__") {
          const tr = (c.translations||[]).filter(t => (t.language||"Other") === lang);
          if (!tr.length) return ""; // hide chapters lacking this language
          // link to the translation in that language
          const t = tr[0];
          const grp = t.group ? ` &middot; ${h(t.group)}` : "";
          return `<button class="ep-btn" data-go="#/${readPath}/${encodeURIComponent(t.id)}">
            <span>Ch ${h(c.number)}${c.title?` &middot; ${h(c.title)}`:""}</span>
            <span class="tag">${h(lang)}${grp}</span></button>`;
        }
        // "all" view (or novel): one row per chapter, show language count if any
        const langCount = (c.translations||[]).length;
        const tag = (kind==="manga" && langCount>1) ? `<span class="tag">${langCount} langs</span>` : "";
        return `<button class="ep-btn" data-go="#/${readPath}/${encodeURIComponent(c.id)}">
          <span>Ch ${h(c.number)}${c.title?` &middot; ${h(c.title)}`:""}</span>${tag}</button>`;
      }).join("");
    }

    // Build and store a reading context: an ordered list of chapter IDs for
    // the active language so the reader's infinity scroll can advance to the
    // next same-language chapter without returning to this page.
    function storeReadCtx(lang){
      const ids = [];
      chs.forEach(c => {
        if(kind === "manga" && lang !== "__all__"){
          const tr = (c.translations||[]).filter(t => (t.language||"Other") === lang);
          if(tr.length) ids.push(tr[0].id);
        } else {
          ids.push(c.id);
        }
      });
      window.__readCtx = { ids, lang, kind };
    }
    // Store immediately for the initial language
    storeReadCtx(langState.active);

    const pager = pagerHtml(
      page,
      totalPages,
      page < totalPages,
      null,
      { jsNav: true }
    );

    setD(
      heroHtml(kind,label,data,facts,actions,syn,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>Chapter List</h2></div>${langTabs}
       <div class="ch-pager-top">${pager}</div>
       <div class="ep-list wide" id="chList">${chs.length?chapterRowsFor(langState.active):`<div class="empty">No chapters yet.</div>`}</div>
       <div class="ch-pager-bot">${pager}</div>`
    );

    // wire language tabs (persist selection)
    const tabsEl = document.getElementById("langTabs");
    if (tabsEl) {
      tabsEl.querySelectorAll(".lang-tab").forEach(tab => tab.addEventListener("click", ()=>{
        langState.active = tab.dataset.lang;
        tabsEl.querySelectorAll(".lang-tab").forEach(t=>t.classList.toggle("active", t===tab));
        document.getElementById("chList").innerHTML = chapterRowsFor(langState.active) || `<div class="empty">No chapters for this language.</div>`;
        storeReadCtx(langState.active);
      }));
    }

    // wire chapter pager — fetch the requested page and re-render, keeping the
    // active language. Numbered buttons (1 … N) jump anywhere, not just ±1.
    const ep = DETAIL_EP[kind];
    const load = async (p)=>{
      if(p === page) return;
      document.querySelectorAll("#d .ep-list").forEach(n=>n.innerHTML=`<div class="spinner"></div>`);
      const fresh = await apiCached(`/${ep}/${encodeURIComponent(id)}?${qs({page:p,size:CHAPTER_SIZE})}`);
      renderReadableSeries(kind, id, fresh, p, langState.active);
    };
    wirePagerJs(document.querySelector("#d .ch-pager-top"), load);
    wirePagerJs(document.querySelector("#d .ch-pager-bot"), load);

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
      data.photo_count?`<span class="pill">${data.photo_count} photos</span>`:"",
      data.video_count?`<span class="pill">${data.video_count} videos</span>`:"",
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
                <video id="hls-${i}" controls preload="metadata" playsinline webkit-playsinline></video>
                <div class="hls-state" id="hls-state-${i}">${spinner}</div>
              </div>`;
          }
          return `<div class="embed-wrap">
              <div class="embed-fallback">External video source. <a class="btn sm" href="${h(u)}" target="_blank" rel="noopener noreferrer">${I.play} Open video ${I.arrow}</a></div>
            </div>`;
        }).join("")
      : "";

    // Photos: natural aspect ratio masonry (not forced 2:3).
    const imgs = (data.images||[]).map(u=>`<a href="${h(u)}" target="_blank" rel="noopener">${imgNatural(u,"")}</a>`).join("");

    setD(
      heroHtml("cosplay","Cosplay",data,facts,actions,null,data.cover)+
      videoBlock+
      `<div class="row-head"><h2><span class="dot"></span>${(data.images||[]).length} Photos</h2></div>`+
      `<div class="gallery">${imgs||`<div class="empty">No photos.</div>`}</div>`
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
    const actions = first?`<a class="btn primary" href="#/read/nhentai/${encodeURIComponent(first.id)}">${I.book} Read</a>`:"";
    setD(
      heroHtml("doujin","Doujin",data,facts,actions,null,data.cover)+
      `<div class="row-head"><h2><span class="dot"></span>Page Preview</h2></div>
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
        const more = pages.length>24?`<a class="thumb more" href="${readHref}"><div class="poster"><div class="ph">+${pages.length-24} pages</div></div></a>`:"";
        const el = document.getElementById("preview");
        if(el) el.innerHTML = pages.length?cells+more:`<div class="empty">No pages.</div>`;
      }catch(e){ const el=document.getElementById("preview"); if(el) el.innerHTML=`<div class="errbox">${h(e.message)}</div>`; }
    } else {
      const el=document.getElementById("preview"); if(el) el.innerHTML=`<div class="empty">No pages.</div>`;
    }
    renderRecommendations("doujin", id);
  }

  async function routeWatch(id){
    const warmed = peek(`/donghua/episode/${encodeURIComponent(id)}`);
    shell(`<div id="d">${warmed?"":spinner}</div>`);
    try{
      const e = await apiCached(`/donghua/episode/${encodeURIComponent(id)}`);
      const servers = e.servers||[];
      const seriesLink = e.series_id?`#/detail/donghua/${encodeURIComponent(e.series_id)}`:"#/";
      const player = servers.length?`<div class="player-wrap"><div class="frame"><iframe id="player" src="${h(servers[0].embed_url)}" allowfullscreen allow="autoplay; encrypted-media; picture-in-picture"></iframe></div></div>`:`<div class="empty">No video servers available.</div>`;
      const bar = servers.length?`<div class="server-bar"><span class="lbl">Server:</span>${servers.map((s,i)=>`<button class="srv ${i===0?"active":""}" data-src="${h(s.embed_url)}">${h(s.label)}${s.format?` &middot; ${h(s.format)}`:""}</button>`).join("")}</div>`:"";
      const dls = (e.downloads||[]).map(g=>`<div class="dl-group"><div class="q">${h(g.quality)}</div><div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div></div>`).join("");
      const nav = `<div class="server-bar" style="margin-top:8px">
        ${e.prev_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.prev_id)}">&larr; Previous ep</a>`:""}
        <a class="btn sm" href="${seriesLink}">&#9776; All episodes</a>
        ${e.next_id?`<a class="btn sm" href="#/watch/${encodeURIComponent(e.next_id)}">Next ep &rarr;</a>`:""}</div>`;
      setView(
        `<div id="d">`+
        crumbs([{href:"#/",label:"Home"},{href:"#/browse/donghua",label:"Donghua"},{label:`${e.series_title||"Episode"} - Ep ${e.episode_number}`}])+
        `<div class="row-head"><h2><span class="dot"></span>${h(e.series_title||"Episode")} - Episode ${e.episode_number}</h2></div>`+
        player+bar+nav+(dls?`<div class="row-head"><h2><span class="dot"></span>Downloads</h2></div>${dls}`:"")+
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
    const warmed = peek(`/anime/episode/${encodeURIComponent(id)}`);
    shell(`<div id="d">${warmed?"":spinner}</div>`);
    try{
      const e = await apiCached(`/anime/episode/${encodeURIComponent(id)}`);
      const mirrors = e.mirrors||[];
      const seriesLink = e.series_id?`#/detail/anime/${encodeURIComponent(e.series_id)}`:"#/";
      const epLabel = e.episode_number!=null ? `Episode ${e.episode_number}` : "Episode";
      // Initial player = default embed if present, else nothing (resolved on click).
      const initial = e.default_embed || "";
      const player = `<div class="player-wrap"><div class="frame">${initial?`<iframe id="player" src="${h(initial)}" allowfullscreen allow="autoplay; encrypted-media; fullscreen; picture-in-picture"></iframe>`:`<div class="empty" id="playerEmpty">Select a server below.</div>`}</div></div>`;
      // Group mirrors by quality.
      const byQ = {};
      mirrors.forEach(m=>{ (byQ[m.quality]=byQ[m.quality]||[]).push(m); });
      const serverBars = Object.entries(byQ).map(([q, list])=>
        `<div class="server-bar"><span class="lbl">${h(q)}:</span>${list.map(m=>`<button class="srv" data-stream="${h(m.stream_id)}">${h(m.name)}</button>`).join("")}</div>`
      ).join("");
      const dls = (e.downloads||[]).map(g=>`<div class="dl-group"><div class="q">${h(g.quality)}${g.size?` &middot; ${h(g.size)}`:""}</div><div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener noreferrer" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div></div>`).join("");
      const nav = `<div class="server-bar" style="margin-top:8px">
        ${e.prev_id?`<a class="btn sm" href="#/watchanime/${encodeURIComponent(e.prev_id)}">&larr; Previous ep</a>`:""}
        <a class="btn sm" href="${seriesLink}">&#9776; All episodes</a>
        ${e.next_id?`<a class="btn sm" href="#/watchanime/${encodeURIComponent(e.next_id)}">Next ep &rarr;</a>`:""}</div>`;
      setView(
        `<div id="d">`+
        crumbs([{href:"#/",label:"Home"},{href:"#/browse/anime",label:"Anime"},{label:`${e.series_title||"Anime"} - ${epLabel}`}])+
        `<div class="row-head"><h2><span class="dot"></span>${h(e.series_title||"Anime")} - ${epLabel}</h2></div>`+
        player+
        `<div class="server-note">Streaming servers are third-party. If one fails, try another.</div>`+
        serverBars+nav+(dls?`<div class="row-head"><h2><span class="dot"></span>Downloads</h2></div>${dls}`:"")+
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
            frame.innerHTML = `<div class="empty">Failed to load server. Try another one.</div>`;
          }
        };
      });
      if(e.next_id) prefetch(`/anime/episode/${encodeURIComponent(e.next_id)}`);
      if(e.prev_id) prefetch(`/anime/episode/${encodeURIComponent(e.prev_id)}`);
      renderRecommendations("anime", e.series_id);
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  async function routeRead(kind, id){
    if(kind==="novel"){ shell(`<div id="d">${peek(`/novel/chapter/${encodeURIComponent(id)}`)?"":spinner}</div>`); return renderNovelChapter(id); }
    const ep = kind==="nhentai"?"nhentai/chapter":"manga/chapter";
    const warmed = peek(`/${ep}/${encodeURIComponent(id)}`);
    shell(`<div id="d">${warmed?"":spinner}</div>`);
    
    // --- Reading context: ordered chapter IDs in the same language ----------
    // Stored by the detail page when the user taps a chapter. Contains:
    //   { ids: [opaque_id, ...], lang: "English"|"__all__", kind: "manga" }
    // Used to navigate same-language chapters for infinity scroll + next/prev.
    const ctx = window.__readCtx || null;
    function findInCtx(chId){
      if(!ctx || ctx.kind!==kind) return -1;
      return ctx.ids.indexOf(chId);
    }
    function nextInCtx(chId){
      const i = findInCtx(chId);
      return (i>=0 && i<ctx.ids.length-1) ? ctx.ids[i+1] : null;
    }

    let currentId = id;
    let currentNextId = nextInCtx(id);
    let loadingNext = false;
    
    function renderNav(chId, c) {
      // Prev/Next are intentionally omitted for comics: infinity scroll loads
      // the next chapter automatically. Keep only a "back to list" link.
      return c.series_id
        ? `<a class="btn sm" href="#/detail/${kind==="nhentai"?"doujin":"manga"}/${encodeURIComponent(c.series_id)}">&#9776; Chapter list</a>`
        : "";
    }

    try{
      const c = await apiCached(`/${ep}/${encodeURIComponent(id)}`);
      // If no reading context, fall back to generic next_id (nhentai/single-lang)
      if(!currentNextId) currentNextId = c.next_id || null;
      const pages = c.pages||[];
      const imgs = pages.map(p=>`<img loading="lazy" referrerpolicy="no-referrer" src="${h(p.url)}" alt="page ${p.index}" onerror="this.style.opacity=.25">`).join("");
      const title = `${h(c.series_title||"Read")} ${c.chapter_number?`&middot; Ch ${c.chapter_number}`:""}`;
      const navTrim = renderNav(id, c).trim();
      setView(
        `<div class="reader-shell" id="readerShell">
           <div class="reader-bar">
             <div class="reader-title">${title}</div>
             <button class="btn sm" id="fsBtn">${I.expand} Fullscreen</button>
           </div>
           <div class="reader" id="readerPages">
             ${pages.length?imgs:`<div class="empty">No pages.</div>`}
             <div id="scrollSentinel" style="height:1px;"></div>
           </div>
           ${adSlot("reader")}
           <div class="reader-nav" id="readerNav">${navTrim}</div>
         </div>`
      );
      // fullscreen toggle
      const shellEl = document.getElementById("readerShell");
      const fsBtn = document.getElementById("fsBtn");
      if(fsBtn && shellEl){
        const sync = ()=>{ const on = document.fullscreenElement===shellEl; fsBtn.innerHTML = on?`${I.compress} Exit`:`${I.expand} Fullscreen`; };
        fsBtn.onclick = ()=>{ if(document.fullscreenElement===shellEl){ document.exitFullscreen&&document.exitFullscreen(); } else { shellEl.requestFullscreen&&shellEl.requestFullscreen().catch(()=>{}); } };
        document.addEventListener("fullscreenchange", sync);
        // Auto-enter fullscreen when the reader opens. Browsers only allow this
        // from a user gesture; the chapter tap usually still counts, but if the
        // request is rejected we silently stay windowed (the button still works).
        if(!document.fullscreenElement && shellEl.requestFullscreen){
          shellEl.requestFullscreen().then(sync).catch(()=>{});
        }
      }
      // warm next chapter for instant paging
      if(currentNextId) prefetch(`/${ep}/${encodeURIComponent(currentNextId)}`);
      
      // --- Infinity Scroll: auto-load next chapter (same language) ----------
      const sentinel = document.getElementById("scrollSentinel");
      const pagesEl = document.getElementById("readerPages");
      const navEl = document.getElementById("readerNav");
      if(window.IntersectionObserver && sentinel && pagesEl && currentNextId) {
        const obs = new IntersectionObserver(async (entries) => {
          if(entries[0].isIntersecting && currentNextId && !loadingNext) {
            loadingNext = true;
            const notice = document.createElement("div");
            notice.className = "ch-loading-notice";
            notice.innerHTML = `<div class="spinner"></div><span>Loading next chapter...</span>`;
            pagesEl.insertBefore(notice, sentinel);
            
            try {
              const nextC = await apiCached(`/${ep}/${encodeURIComponent(currentNextId)}`);
              notice.remove();
              
              // Chapter separator
              const sep = document.createElement("div");
              sep.className = "ch-separator";
              sep.innerHTML = `<span class="ch-sep-line"></span><span class="ch-sep-label">Chapter ${nextC.chapter_number||"Next"}</span><span class="ch-sep-line"></span>`;
              pagesEl.insertBefore(sep, sentinel);
              
              // Append next chapter's pages
              const nextPages = nextC.pages||[];
              const frag = document.createDocumentFragment();
              nextPages.forEach(p => {
                const img = document.createElement("img");
                img.loading = "lazy";
                img.referrerPolicy = "no-referrer";
                img.src = p.url;
                img.alt = "page " + p.index;
                img.onerror = function(){ this.style.opacity = ".25"; };
                frag.appendChild(img);
              });
              pagesEl.insertBefore(frag, sentinel);
              
              // Update state: advance to the next-next chapter (same language)
              currentId = currentNextId;
              currentNextId = nextInCtx(currentId) || nextC.next_id || null;
              if(navEl) navEl.innerHTML = renderNav(currentId, nextC).trim();
              history.replaceState(null, "", `#/read/${kind}/${encodeURIComponent(currentId)}`);
              
              if(currentNextId) prefetch(`/${ep}/${encodeURIComponent(currentNextId)}`);
            } catch(err) {
              notice.innerHTML = `<span class="ch-load-err">Failed to load chapter. <a href="javascript:void(0)" onclick="location.reload()">Retry</a></span>`;
            }
            loadingNext = false;
          }
        }, { rootMargin: "1500px" });
        obs.observe(sentinel);
      }
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  async function renderNovelChapter(id){
    try{
    const c = await apiCached(`/novel/chapter/${encodeURIComponent(id)}`);
    const paras = (c.body||"").split(/\n{2,}/).map(s=>s.trim()).filter(Boolean).map(p=>`<p>${h(p)}</p>`).join("");
    const nav = `<div class="reader-nav">
      ${c.prev_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.prev_id)}">&larr; Previous</a>`:""}
      ${c.series_id?`<a class="btn sm" href="#/detail/novel/${encodeURIComponent(c.series_id)}">&#9776; Chapter list</a>`:""}
      ${c.next_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.next_id)}">Next &rarr;</a>`:""}</div>`;
    setView(`<div class="row-head"><h2><span class="dot"></span>${h(c.series_title||"Novel")} &middot; Ch ${c.chapter_number}</h2></div>`+
      (c.chapter_title?`<p style="color:var(--muted);margin-top:-8px">${h(c.chapter_title)}</p>`:"")+
      `<div class="novel-body">${paras||"<p>(empty)</p>"}</div>`+nav);
    if(c.next_id) prefetch(`/novel/chapter/${encodeURIComponent(c.next_id)}`);
    }catch(e){ setView(`<div class="errbox">${h(e.message)}</div>`); }
  }

  // route dispatch is defined in part 2 (appended)
  window.__apiku = { shell, setView, viewEl, h, qs, api, apiRaw, apiCached, prefetch, spinner, go, I,
    LITE, setLite,
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
`# Search manga "one piece"
curl '${url}'

# Pretty-print dengan jq
curl '${url}' | jq .`,
      javascript:
`// Browser / Node 18+
const res = await fetch('${origin}/api/v1/search?q=one piece&source=manga');
const json = await res.json();
if (!json.ok) throw new Error(json.error.code + ': ' + json.error.message);
console.log(\`\${json.data.total} results (\${json.meta.took_ms}ms)\`);
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
echo $json['data']['total'] . " results\\n";
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
    fmt.Printf("%d results\\n", env.Data.Total)
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
    println!("{} results", d.total);
    for it in d.items { println!("{} {}", it.source, it.title); }
    Ok(())
}`,
    };
  }

  function codeBlock(lang, code) {
    return `<div class="codeblock" data-lang="${lang}">
      <div class="cb-head"><span class="cb-lang">${lang}</span><button class="cb-copy">Copy</button></div>
      <pre><code>${h(code)}</code></pre>
    </div>`;
  }

  function routeDocs() {
    const origin = location.origin;
    const samples = codeSamples(origin);
    const langs = [["curl","cURL"],["javascript","JavaScript"],["python","Python"],["php","PHP"],["go","Go"],["rust","Rust"]];
    shell(`
      <div class="docs">
        <div class="hero-banner"><h1>API Documentation</h1><p>All endpoints return a JSON envelope <code>{ status, ok, data, meta }</code>. No API key required.</p></div>

        <h2>Base URL</h2>
        <p><code>${h(origin)}</code> &middot; base path <code>/api/v1</code></p>

        <h2>Request examples</h2>
        <div class="lang-pills" id="langPills">
          ${langs.map(([v,l],i)=>`<button class="${i===0?"active":""}" data-lang="${v}">${l}</button>`).join("")}
        </div>
        <div id="sampleBox">${codeBlock("curl", samples.curl)}</div>

        <h2>Response envelope</h2>
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

        <h2>Status &amp; error codes</h2>
        <table>
          <thead><tr><th>Code</th><th>Meaning</th></tr></thead>
          <tbody>
            <tr><td><code>200</code></td><td>Success</td></tr>
            <tr><td><code>400 invalid_id</code></td><td>Opaque ID broken / bad signature</td></tr>
            <tr><td><code>400 missing_query</code></td><td>Search without <code>q</code></td></tr>
            <tr><td><code>403 host_not_allowed</code></td><td>Image host not on proxy allowlist</td></tr>
            <tr><td><code>404 not_found</code></td><td>Route does not exist</td></tr>
            <tr><td><code>502 upstream_error</code></td><td>Upstream source failed</td></tr>
          </tbody>
        </table>

        <p style="margin-top:24px">Need the full console? Open the <a href="#/explorer">Explorer</a> or <a href="/tester">dev console</a>.</p>

        <h2>About</h2>
        <p>Built by <a href="https://github.com/risqinf" target="_blank" rel="noopener"><b>@risqinf</b></a>. View source &amp; contribute on <a href="https://github.com/risqinf/apiku" target="_blank" rel="noopener">GitHub</a>.</p>
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
          ()=>{ btn.textContent="Copied"; setTimeout(()=>btn.textContent="Copy",1200); },
          ()=>{ btn.textContent="Failed"; setTimeout(()=>btn.textContent="Copy",1200); }
        );
      });
    });
  }

  // ---- Explorer -----------------------------------------------------------
  // Grouped preset endpoints: [group, [[label, path], ...]]
  const EXP_GROUPS = [
    ["General", [
      ["Server info", "/api/v1/info"],
      ["Health check", "/api/v1/health"],
    ]],
    ["Search", [
      ["Search all sources", "/api/v1/search?q=one+piece&source=all&page=1"],
      ["Search comics", "/api/v1/search?q=one+piece&source=manga&page=1"],
      ["Search donghua", "/api/v1/search?q=martial&source=donghua&page=1"],
      ["Search novel", "/api/v1/search?q=martial&source=novel&page=1"],
    ]],
    ["Browse / Feed", [
      ["Latest donghua", "/api/v1/browse/anichin?feed=home"],
      ["Popular comics", "/api/v1/browse/mangaball?feed=popular"],
      ["Latest novels", "/api/v1/browse/novelid?feed=home"],
      ["Today's doujin", "/api/v1/browse/nhentai?feed=popular-today"],
    ]],
    ["Detail (replace {id})", [
      ["Comics", "/api/v1/manga/{id}?page=1&size=60"],
      ["Comic chapter", "/api/v1/manga/chapter/{id}"],
      ["Donghua", "/api/v1/donghua/{id}"],
      ["Donghua episode", "/api/v1/donghua/episode/{id}"],
      ["Novel", "/api/v1/novel/{id}?page=1&size=60"],
      ["Novel chapter", "/api/v1/novel/chapter/{id}"],
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
        <div class="hero-banner"><h1>API Explorer</h1><p>Test <code>/api/v1/*</code> endpoints directly, view responses, and copy ready-to-use code samples.</p></div>

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
                <button class="btn primary" id="exp-send">${I.play} Send</button>
              </div>
              <p class="exp-tip">Tip: replace <code>{id}</code> with an opaque id from <a href="#/search/one piece">search</a> or browse results.</p>
            </div>

            <div class="exp-resp">
              <div class="exp-resp-head">
                <div class="exp-meta" id="exp-meta"><span class="pill">ready</span></div>
                <button class="btn sm" id="exp-copy">Copy JSON</button>
              </div>
              <pre class="exp-out" id="exp-out">// Press "Send" to see the response.</pre>
            </div>

            <div class="exp-code">
              <div class="exp-code-head">
                <h3>Code samples</h3>
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
      out.textContent = "Loading...";
      btn.disabled = true;
      try {
        const res = await apiRaw("GET", rel);
        const ok2 = res.status >= 200 && res.status < 300;
        const cls = ok2 ? "ok" : "bad";
        meta.innerHTML = `<span class="pill ${cls}">HTTP ${res.status}</span> <span class="pill">${res.ms} ms</span> <span class="pill">${ok2?"success":"failed"}</span>`;
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
        ()=>{ e.target.textContent="Copied"; setTimeout(()=>e.target.textContent="Copy JSON",1200); },
        ()=>{ e.target.textContent="Failed"; setTimeout(()=>e.target.textContent="Copy JSON",1200); }
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
