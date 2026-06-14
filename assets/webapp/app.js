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
    try{
      // resolveUrl is /api/v1/cosplay-video?... ; strip the /api/v1 prefix for api()
      const rel = resolveUrl.replace(/^.*\/api\/v1/, "");
      const res = await api(rel);
      const src = res && res.url;
      if(!src){ hlsFail(idx, "Stream not found"); return; }
      attachHlsSrc(video, idx, src);
    }catch(e){ hlsFail(idx, e.message || "Failed to load video"); }
  }
  function hlsClear(idx){ const s = document.getElementById(`hls-state-${idx}`); if(s) s.remove(); }
  function hlsFail(idx, msg, src){
    const state = document.getElementById(`hls-state-${idx}`);
    if(state) state.innerHTML = `<div class="hls-err">${escHtml(msg)}${src?`<br><a class="btn sm" href="${escAttr(src)}" target="_blank" rel="noopener noreferrer">Open directly</a>`:""}</div>`;
  }
  // Attach an already-resolved HLS master URL to a <video> element. Shared by
  // the cosplay player and the movie player (which resolves the URL itself, so
  // it can avoid a second resolve round-trip).
  async function attachHlsSrc(video, idx, src){
    const clear = ()=>hlsClear(idx);
    const fail = (msg, s)=>hlsFail(idx, msg, s);
    try{
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

  // ---- Hardened player iframe (Brave-style ad/pop-up blocking) ------------
  // Third-party video hosts (playmogo/dood, streampoi, donghua/anime mirrors,
  // movie embeds) are ad-funded and fire pop-ups, pop-unders and forced
  // new-tab / top-window redirects. For embeds we can't resolve to a direct
  // stream we still have to iframe them, but we wrap them in a strict sandbox
  // that lets the player run and play video while blocking the ad behaviour:
  //   - NO allow-popups / allow-popups-to-escape-sandbox  -> window.open()/pop-unders blocked
  //   - NO allow-top-navigation*                          -> can't redirect or hijack our tab
  //   - NO allow-modals                                   -> no interstitial ad dialogs
  //   - allow-scripts + allow-same-origin                 -> player still runs & plays
  // This is the same principle Brave uses for cross-origin frames we can't
  // rewrite: contain them so ads can't escape the box.
  const PLAYER_SANDBOX = "allow-scripts allow-same-origin allow-forms allow-presentation allow-orientation-lock";
  function playerIframe(src, id){
    const idAttr = id ? ` id="${id}"` : "";
    return `<iframe${idAttr} src="${escAttr(src)}" loading="lazy" referrerpolicy="origin" allowfullscreen allow="autoplay; fullscreen; encrypted-media; picture-in-picture" sandbox="${PLAYER_SANDBOX}"></iframe>`;
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
    // Favorite control icon. Outline by default; the .fav-btn.on /
    // [aria-pressed="true"] CSS fills it (fill: currentColor) for the saved state.
    heart:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 21s-7-4.5-9.5-9A5.5 5.5 0 0 1 12 6a5.5 5.5 0 0 1 9.5 6c-2.5 4.5-9.5 9-9.5 9z"/></svg>',
    // Release-schedule icon: rounded calendar frame, two top binding ticks and
    // a divider line under the header (used by the Donghua Schedule nav item).
    calendar:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="16" rx="3"/><path d="M8 3v4M16 3v4M3 10h18"/></svg>',
  };

  // ---- Provider config ---------------------------------------------------
  const PROVIDERS = {
    anime:   { label: "Anime",   api: "anime",       kind: "anime",   adult: false, icon: I.anime },
    donghua: { label: "Donghua", api: "anichin",     kind: "donghua", adult: false, icon: I.donghua },
    lmanime: { label: "Anime EN", api: "lmanime",    kind: "lmanime", adult: false, icon: I.anime },
    movie:   { label: "Movies",   api: "lk21",       kind: "movie",   adult: false, icon: I.donghua },
    drama:   { label: "Drama",    api: "dramabox",   kind: "drama",   adult: false, icon: I.donghua },
    manga:   { label: "Comics",  api: "mangaball",   kind: "manga",   adult: false, icon: I.manga },
    novel:   { label: "Novel",   api: "novelid",     kind: "novel",   adult: false, icon: I.novel },
    cosplay: { label: "Cosplay", api: "cosplaytele", kind: "cosplay", adult: true,  icon: I.cosplay },
    doujin:  { label: "Doujin",  api: "nhentai",     kind: "doujin",  adult: true,  icon: I.doujin },
    nekopoi: { label: "Hentai",  api: "nekopoi",     kind: "nekopoi", adult: true,  icon: I.anime },
  };
  const EPISODE_SIZE = 5000; // donghua: fetch the whole episode list at once

  // anime-like providers share the mirror-resolve player pipeline. Each maps
  // to its API base, stream-resolver endpoint, watch route, and browse key.
  const ANIME_LIKE = {
    anime:   { api: "anime",   stream: "anime-stream",   watch: "watchanime", label: "Anime",    browse: "anime" },
    lmanime: { api: "lmanime", stream: "lmanime-stream", watch: "watchlm",    label: "Anime EN", browse: "lmanime" },
  };

  const FEEDS = {
    otakudesu:   [["ongoing","Ongoing"],["complete","Completed"],["action","Action"],["romance","Romance"],["comedy","Comedy"],["fantasy","Fantasy"],["adventure","Adventure"],["drama","Drama"],["horror","Horror"],["mystery","Mystery"],["sci-fi","Sci-Fi"],["slice-of-life","Slice of Life"],["supernatural","Supernatural"],["school","School"]],
    anime:       [["ongoing","Ongoing"],["completed","Completed"],["action","Action"],["romance","Romance"],["comedy","Comedy"],["fantasy","Fantasy"],["adventure","Adventure"],["drama","Drama"],["horror","Horror"],["mystery","Mystery"],["sci-fi","Sci-Fi"],["slice-of-life","Slice of Life"],["supernatural","Supernatural"],["school","School"]],
    anichin:     [["home","Latest"],["popular","Popular"],["rating","Rating"],["title","A-Z"]],
    lmanime:     [["ongoing","Ongoing"],["all","A-Z"],["action","Action"],["fantasy","Fantasy"],["romance","Romance"],["comedy","Comedy"],["adventure","Adventure"],["mystery","Mystery"],["isekai","Isekai"],["cultivation","Cultivation"],["drama","Drama"]],
    lk21:        [["populer","Popular"],["latest","Latest"],["rating","Rating"],["release","By Year"],["nontondrama","Series"],["action","Action"],["drama","Drama"],["horror","Horror"],["comedy","Comedy"],["thriller","Thriller"],["sci-fi","Sci-Fi"],["romance","Romance"],["animation","Animation"]],
    mangaball:   [["home","Featured"],["popular","Popular"],["latest","Latest"],["recommend","Recommended"]],
    novelid:     [["home","All"],["popular","Completed"],["novel-translate","Translated"],["fantasi","Fantasy"],["romantis","Romance"],["aksi","Action"],["horror","Horror"],["komedi","Comedy"],["religi","Religi"],["motivasi","Motivasi"],["sastra","Sastra"],["novel-anak","Anak"]],
    cosplaytele: [["home","Latest"]],
    nhentai:     [["popular-today","Popular Today"],["popular-week","Popular Week"],["popular","All Time"],["home","Recent"]],
    nekopoi:     [["latest","Latest"],["hentai","Hentai"],["3d","3D"],["2d","2D Animation"],["jav","JAV"],["jav-cosplay","JAV Cosplay"]],
    dramabox:    [["latest","Latest"]],
  };

  const DETAIL_EP = { anime:"anime", donghua:"donghua", lmanime:"lmanime", movie:"movie", manga:"manga", novel:"novel", cosplay:"cosplay", doujin:"nhentai", nekopoi:"nekopoi", drama:"drama" };

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

  // ---- Personalization store (favorites / search / browsing history) ------
  // Per-device, client-side store with graceful degradation. It manages three
  // collections across two real backends plus an in-memory fallback:
  //   - favorites       -> localStorage  (apiku.fav.v1)      sync
  //   - search history  -> localStorage  (apiku.search.v1)   sync
  //   - browsing history-> IndexedDB     (db "apiku")        async
  // When a real backend is unavailable/disabled/failing we swap in the matching
  // MemoryBackend, and every public method is wrapped so a thrown backend error
  // degrades to a safe default ([], false, or a no-op) instead of breaking a
  // render path (Req 6.3). Persisted blobs are version-tagged so the schema can
  // evolve; unknown versions migrate to an empty collection (Req 6.1, 6.2).
  //
  // NOTE: backends, versioned schema, migration, init-time probing, and the
  // safe-wrapping harness (task 2.1), the pure helpers (task 2.2), the
  // favorites + search-history bodies (task 2.3), and the IndexedDB
  // browsing-history bodies (task 2.4) are all in place.
  const pstore = (function(){
    const SCHEMA_VERSION = 1;
    const KEYS = { fav: "apiku.fav.v1", search: "apiku.search.v1" };
    const IDB  = { db: "apiku", version: 1, store: "history", keyPath: "opaqueId", index: "byTime", indexKey: "timestamp" };

    // stableKey(id): a content identity that survives opaque-ID rotation. An
    // opaque ID is `<src><kind><nonce>.<base64url(url)>.<mac>`; the nonce and
    // mac vary per issuance / signing-secret, but the source code and the
    // base64url(url) payload do NOT. We key favorites/history on
    // `<src>:<payload>` so a saved item still matches itself after a restart,
    // a re-fetch (fresh nonce), or a secret change — no more orphaned, no-image
    // entries.
    function stableKey(id){
      if(id == null) return "";
      const s = String(id);
      const dot = s.indexOf(".");
      if(dot < 2) return s; // not an opaque token; use as-is
      const src = s.slice(0, 2);
      let payload = s.slice(dot + 1);
      const dot2 = payload.indexOf(".");
      if(dot2 !== -1) payload = payload.slice(0, dot2);
      return payload ? (src + ":" + payload) : s;
    }

    // -- versioned blob wrapper + migration --------------------------------
    // localStorage collections are stored as { v:1, items:[...] }.
    function wrap(items){ return { v: SCHEMA_VERSION, items: Array.isArray(items) ? items : [] }; }
    // Return a usable items array for a known version. Unknown/future/malformed
    // versions degrade to an empty collection rather than throwing (Req 6.1).
    function migrate(v, data){
      if(v === SCHEMA_VERSION && data && Array.isArray(data.items)) return data.items.slice();
      return [];
    }

    // -- safe wrappers: never let a backend error escape to a caller (Req 6.3)
    function safe(fn, fallback){
      return function(){
        try { return fn.apply(this, arguments); }
        catch(_){ return typeof fallback === "function" ? fallback() : fallback; }
      };
    }
    function safeAsync(fn, fallback){
      return async function(){
        try { return await fn.apply(this, arguments); }
        catch(_){ return typeof fallback === "function" ? fallback() : fallback; }
      };
    }

    // -- pure helpers (task 2.2) -------------------------------------------
    // Side-effect-free and total (never throw). Shared by the store methods
    // (tasks 2.3/2.4) and the render/route code, and exposed on the returned
    // object so the Node test harnesses (tasks 2.5+) can reach them. The
    // persisted favorites/search/history method bodies layer on top of these.

    // Searchable text of a search-history element. Accepts either a SearchEntry
    // ({ query, norm, timestamp }) or a bare query string.
    function entryQuery(e){
      if(e && typeof e === "object") return e.query != null ? String(e.query) : "";
      return e == null ? "" : String(e);
    }

    // filterSearches(list, text): case-insensitive substring filter over the
    // search list, preserving original (most-recent-first) order. Blank text
    // matches everything. Returns the matching subset (Req 2.4).
    function filterSearches(list, text){
      const arr = Array.isArray(list) ? list : [];
      const needle = (text == null ? "" : String(text)).toLowerCase();
      if(needle === "") return arr.slice();
      return arr.filter(function(e){ return entryQuery(e).toLowerCase().indexOf(needle) !== -1; });
    }

    // visible(entries): drop entries whose kind is cosplay or doujin when the
    // adult toggle is off; return the list unchanged when adult is on
    // (Req 3.10, 4.7).
    function visible(entries){
      const arr = Array.isArray(entries) ? entries : [];
      if(adultOn()) return arr.slice();
      return arr.filter(function(e){
        const k = e && e.kind;
        return k !== "cosplay" && k !== "doujin" && k !== "nekopoi";
      });
    }

    // entryRoute(kind, id): canonical per-kind hash route. Every supported kind
    // (anime, donghua, manga/comics, novel, cosplay, doujin) opens its detail
    // view; readers/watch pages are reached from there, which keeps this a
    // total, pure mapping with the id URL-encoded (Req 5.5). "comics" is an
    // alias for the "manga" provider kind used by the detail route.
    function entryRoute(kind, id){
      const raw = (kind == null ? "" : String(kind)).trim().toLowerCase();
      const k = raw === "comics" ? "manga" : raw;
      return "#/detail/" + encodeURIComponent(k) + "/" + encodeURIComponent(id == null ? "" : String(id));
    }

    // Apply a key function defensively so a throwing keyFn cannot break a pure
    // helper (keeps the helpers total).
    function keyOf(keyFn, e){ try { return keyFn(e); } catch(_){ return undefined; } }

    // dedupeMoveToFront(list, keyFn, entry): return a NEW list with any element
    // sharing entry's key removed, and entry placed at the front (most-recent
    // first). Shared by both recency-keyed collections — search history and
    // browsing history (Req 2.2, 3.8).
    function dedupeMoveToFront(list, keyFn, entry){
      const arr = Array.isArray(list) ? list : [];
      const fn = typeof keyFn === "function" ? keyFn : function(x){ return x; };
      const key = keyOf(fn, entry);
      const rest = arr.filter(function(e){ return keyOf(fn, e) !== key; });
      return [entry].concat(rest);
    }

    // toggleMembership(list, keyFn, entry): pure favorite/membership toggle
    // decision, separated from persistence (the persisted toggleFavorite lands
    // in task 2.3). is-member -> remove (added=false); not-member -> add at the
    // front (added=true). Returns a NEW { list, added } pair (Req 4.2, 4.3).
    function toggleMembership(list, keyFn, entry){
      const arr = Array.isArray(list) ? list : [];
      const fn = typeof keyFn === "function" ? keyFn : function(x){ return x; };
      const key = keyOf(fn, entry);
      const present = arr.some(function(e){ return keyOf(fn, e) === key; });
      if(present) return { list: arr.filter(function(e){ return keyOf(fn, e) !== key; }), added: false };
      return { list: dedupeMoveToFront(arr, fn, entry), added: true };
    }

    // -- synchronous collection backends (favorites + search history) ------
    // localStorage-backed: a single versioned blob per key.
    function localBackend(key){
      return {
        kind: "local",
        load(){
          const raw = localStorage.getItem(key);
          if(raw == null) return [];
          let blob = null;
          try { blob = JSON.parse(raw); } catch(_){ return []; }
          return migrate(blob && blob.v, blob);
        },
        save(items){ localStorage.setItem(key, JSON.stringify(wrap(items))); },
        clear(){ localStorage.removeItem(key); },
      };
    }
    // in-memory fallback with the same (sync) interface.
    function memBackend(){
      let items = [];
      return {
        kind: "memory",
        load(){ return items.slice(); },
        save(next){ items = Array.isArray(next) ? next.slice() : []; },
        clear(){ items = []; },
      };
    }

    // -- asynchronous history backend (IndexedDB) --------------------------
    function idbOpen(){
      return new Promise(function(resolve, reject){
        let req;
        try { req = indexedDB.open(IDB.db, IDB.version); }
        catch(e){ reject(e); return; }
        req.onupgradeneeded = function(){
          const db = req.result;
          let os;
          if(!db.objectStoreNames.contains(IDB.store)){
            os = db.createObjectStore(IDB.store, { keyPath: IDB.keyPath });
          } else {
            os = req.transaction.objectStore(IDB.store);
          }
          if(os && !os.indexNames.contains(IDB.index)) os.createIndex(IDB.index, IDB.indexKey);
        };
        req.onsuccess = function(){ resolve(req.result); };
        req.onerror   = function(){ reject(req.error || new Error("indexedDB open failed")); };
      });
    }
    function idbBackend(){
      let dbp = null;
      function db(){ return dbp || (dbp = idbOpen()); }
      function objStore(mode){ return db().then(function(d){ return d.transaction(IDB.store, mode).objectStore(IDB.store); }); }
      function wait(req){ return new Promise(function(res, rej){ req.onsuccess=function(){ res(req.result); }; req.onerror=function(){ rej(req.error); }; }); }
      return {
        kind: "idb",
        async getAll(){ return wait((await objStore("readonly")).getAll()); },
        async put(entry){ return wait((await objStore("readwrite")).put(entry)); },
        async delete(id){ return wait((await objStore("readwrite")).delete(id)); },
        async clear(){ return wait((await objStore("readwrite")).clear()); },
      };
    }
    // in-memory async history fallback with the same interface.
    function memHistoryBackend(){
      const map = new Map();
      return {
        kind: "memory",
        async getAll(){ return Array.from(map.values()); },
        async put(entry){ if(entry && entry.opaqueId != null) map.set(entry.opaqueId, entry); },
        async delete(id){ map.delete(id); },
        async clear(){ map.clear(); },
      };
    }

    // -- backend probing + selection (graceful degradation, Req 6.3/6.5) ---
    function localStorageWorks(){
      try {
        const k = "apiku.__probe";
        localStorage.setItem(k, "1");
        localStorage.removeItem(k);
        return true;
      } catch(_){ return false; }
    }

    const lsOk = localStorageWorks();
    let favBackend     = lsOk ? localBackend(KEYS.fav)    : memBackend();
    let searchBackend  = lsOk ? localBackend(KEYS.search) : memBackend();

    // History starts on IndexedDB when present, then an async probe (a real
    // open + read) swaps in the memory fallback if IndexedDB is unusable.
    let historyBackend = (typeof indexedDB !== "undefined" && indexedDB) ? idbBackend() : memHistoryBackend();
    (async function probeHistory(){
      if(historyBackend.kind !== "idb") return;
      try { await historyBackend.getAll(); }
      catch(_){ historyBackend = memHistoryBackend(); }
    })();

    // Backend handles exposed for later tasks (2.3 favorites/search, 2.4 history).
    // Getters so callers always see the current backend after a probe swap.
    const backends = {
      get fav(){ return favBackend; },
      get search(){ return searchBackend; },
      get history(){ return historyBackend; },
    };

    // -- caps / eviction limits (Req 2.9, 3.9; HISTORY used by task 2.4) ----
    // Favorites are capped defensively; search history is an LRU capped at
    // SEARCH; HISTORY (the FIFO HistoryCap) is consumed by the IndexedDB
    // browsing-history bodies in task 2.4 but defined here as the single
    // source of truth.
    const CAPS = { FAVORITES: 1000, SEARCH: 50, HISTORY: 500 };

    // ===== favorites (localStorage via backends.fav) — task 2.3 ===========
    // Identity is the stable content key (survives opaque-ID rotation), not the
    // raw opaqueId. Stored most-recent-first (front = most recent).
    function favKey(e){ return e ? (e.key || stableKey(e.opaqueId)) : undefined; }

    // Build a persisted entry from a RichMetadata-shaped object, guaranteeing a
    // timestamp (preserved when the caller already supplied one, Req 4.5) and a
    // stable content key derived from the opaqueId.
    function withTimestamp(meta){
      const entry = Object.assign({}, (meta && typeof meta === "object") ? meta : {});
      if(entry.timestamp == null) entry.timestamp = Date.now();
      if(!entry.key && entry.opaqueId != null) entry.key = stableKey(entry.opaqueId);
      return entry;
    }

    function listFavoritesImpl(){ return backends.fav.load(); }

    function isFavoriteImpl(opaqueId){
      const k = stableKey(opaqueId);
      return listFavoritesImpl().some(function(e){ return favKey(e) === k; });
    }

    // Dedupe + move-to-front by stable key, then cap to CAPS.FAVORITES by
    // trimming the oldest (tail) entries.
    function addFavoriteImpl(meta){
      const entry = withTimestamp(meta);
      const next = dedupeMoveToFront(listFavoritesImpl(), favKey, entry).slice(0, CAPS.FAVORITES);
      backends.fav.save(next);
    }

    // Remove only the entry matching the stable key of opaqueId.
    function removeFavoriteImpl(opaqueId){
      const k = stableKey(opaqueId);
      const next = listFavoritesImpl().filter(function(e){ return favKey(e) !== k; });
      backends.fav.save(next);
    }

    // Use the pure membership decision, persist the result, and return the new
    // membership (true when the item is now a favorite — Req 4.2, 4.3).
    function toggleFavoriteImpl(meta){
      const entry = withTimestamp(meta);
      const res = toggleMembership(listFavoritesImpl(), favKey, entry);
      backends.fav.save(res.list.slice(0, CAPS.FAVORITES));
      return res.added;
    }

    // Self-heal rewrite: refresh the opaqueId (and any newer metadata) of the
    // favorite whose stable key matches, preserving its list position (Req 7.4).
    function updateFavoriteIdImpl(oldId, newId){
      const k = stableKey(oldId);
      const next = listFavoritesImpl().map(function(e){
        return favKey(e) === k
          ? Object.assign({}, e, { opaqueId: newId, key: stableKey(newId) })
          : e;
      });
      backends.fav.save(next);
    }

    // ===== search history (localStorage via backends.search) — task 2.3 ===
    // LRU, most-recent-first. Deduped by the normalized (trim+lowercase) query.
    function normQuery(query){ return (query == null ? "" : String(query)).trim().toLowerCase(); }
    function searchKey(e){ return e && e.norm; }

    function listSearchesImpl(){ return backends.search.load(); }

    // Normalize + store a SearchEntry, dedupe+move-to-front by normalized query,
    // then tail-trim to CAPS.SEARCH so the least-recently-used queries are
    // evicted (Req 2.1, 2.2, 2.9). Blank queries are ignored.
    function recordSearchImpl(query){
      const raw = (query == null ? "" : String(query)).trim();
      const norm = raw.toLowerCase();
      if(norm === "") return;
      const entry = { query: raw, norm: norm, timestamp: Date.now() };
      const next = dedupeMoveToFront(listSearchesImpl(), searchKey, entry).slice(0, CAPS.SEARCH);
      backends.search.save(next);
    }

    // Store member that wraps the pure `filterSearches` helper over the current
    // search list (most-recent-first order preserved — Req 2.4).
    function filterSearchesImpl(text){ return filterSearches(listSearchesImpl(), text); }

    // Remove only the entry whose normalized query matches (accepts either a
    // raw query or an already-normalized key — Req 2.6).
    function removeSearchImpl(query){
      const norm = normQuery(query);
      const next = listSearchesImpl().filter(function(e){ return searchKey(e) !== norm; });
      backends.search.save(next);
    }

    function clearSearchesImpl(){ backends.search.clear(); }

    // ===== browsing history (IndexedDB via backends.history) — task 2.4 ===
    // Keyed by opaqueId (the IndexedDB keyPath); each record holds the full
    // RichMetadata. All reads/writes are async. Writes go through `safeWrite`
    // so a quota error triggers an eviction + single retry and any other
    // failure degrades to a no-op — recordHistory is fire-and-forget from
    // render paths and must never throw (Req 6.3, 6.4).
    function historyKey(e){ return e && e.opaqueId; }

    // Quota-error classification. Browsers signal a full store with a
    // QuotaExceededError DOMException; we also accept the legacy numeric codes
    // (22 in most engines, 1014 / NS_ERROR_DOM_QUOTA_REACHED in Firefox) and
    // an IDB request's `.error` wrapper.
    function isQuotaError(e){
      if(!e) return false;
      const name = e.name || (e.error && e.error.name);
      if(name === "QuotaExceededError" || name === "NS_ERROR_DOM_QUOTA_REACHED") return true;
      const code = e.code != null ? e.code : (e.error && e.error.code);
      return code === 22 || code === 1014;
    }

    // Oldest-first (ascending timestamp) copy; missing timestamps sort oldest.
    // FIFO eviction always drops from the front of this ordering (Req 3.9).
    function byTimestampAsc(entries){
      const arr = Array.isArray(entries) ? entries : [];
      return arr.slice().sort(function(a, b){
        return ((a && a.timestamp) || 0) - ((b && b.timestamp) || 0);
      });
    }

    // Evict the `n` oldest history entries (by ascending timestamp). Best-effort
    // and total: used both by the FIFO cap and the quota-recovery path.
    async function evictOldestHistory(n){
      if(!(n > 0)) return;
      const all = await backends.history.getAll();
      const oldest = byTimestampAsc(all).slice(0, n);
      for(const e of oldest){
        const id = historyKey(e);
        if(id != null) await backends.history.delete(id);
      }
    }

    // safeWrite(writeFn): run an IndexedDB write; on a quota error free ~20% of
    // the cap (oldest first) and retry the write once. If the retry still fails
    // — or the original error was not a quota error — degrade to a no-op so a
    // failing backend never throws into a render / fire-and-forget path
    // (Req 6.3, 6.4).
    async function safeWrite(writeFn){
      try { return await writeFn(); }
      catch(e){
        if(isQuotaError(e)){
          try {
            await evictOldestHistory(Math.ceil(CAPS.HISTORY * 0.2));
            return await writeFn();
          } catch(_){ /* still failing after retry: degrade to no-op */ }
        }
        /* non-quota error: swallow to keep the app rendering */
      }
    }

    // recordHistory(meta): upsert by opaqueId. Because the store's keyPath is
    // opaqueId, `put` overwrites an existing record, so re-opening an item
    // updates the same entry. We stamp a strictly-increasing timestamp (greater
    // than any existing entry's, even within the same millisecond) so the
    // re-opened item always moves to the most-recent/front position (Req 3.8).
    // After the write, enforce the FIFO cap: when the count exceeds CAPS.HISTORY
    // evict the oldest entries by timestamp until within the cap (Req 3.9).
    // Entries without an opaqueId are ignored.
    async function recordHistoryImpl(meta){
      const base = (meta && typeof meta === "object") ? meta : {};
      if(base.opaqueId == null) return;
      const key = base.key || stableKey(base.opaqueId);
      const existing = await backends.history.getAll();
      let maxTs = 0;
      for(const e of existing){ const t = (e && e.timestamp) || 0; if(t > maxTs) maxTs = t; }
      // Find any prior record for the same content (possibly under a rotated id)
      // so we can both re-key it and carry its richer fields forward.
      const prior = existing.find(function(e){
        return e && (e.key || stableKey(e.opaqueId)) === key;
      }) || {};
      for(const e of existing){
        if(e && (e.key || stableKey(e.opaqueId)) === key && e.opaqueId !== base.opaqueId){
          await safeWrite(function(){ return backends.history.delete(e.opaqueId); });
        }
      }
      // Merge: new meta wins, but never let a missing/empty field erase a good
      // prior value (e.g. opening the series page must not wipe "Ep 14"
      // progress recorded while watching).
      const entry = Object.assign({}, prior, base, {
        key,
        timestamp: Math.max(Date.now(), maxTs + 1),
      });
      if((base.thumbnail == null || base.thumbnail === "") && prior.thumbnail) entry.thumbnail = prior.thumbnail;
      if(base.progress == null && prior.progress) entry.progress = prior.progress;
      if((base.title == null || base.title === "") && prior.title) entry.title = prior.title;
      await safeWrite(function(){ return backends.history.put(entry); });
      const after = await backends.history.getAll();
      if(after.length > CAPS.HISTORY) await evictOldestHistory(after.length - CAPS.HISTORY);
    }

    // listHistory(): most-recent-first (descending timestamp). The store has no
    // inherent order, so we sort on read (Req 3.8 ordering). Also collapses any
    // residual same-key duplicates (keeping the most recent) as a safety net.
    async function listHistoryImpl(){
      const all = await backends.history.getAll();
      const ordered = byTimestampAsc(all).reverse();
      const seen = new Set();
      const out = [];
      for(const e of ordered){
        const k = e && (e.key || stableKey(e.opaqueId));
        if(k != null && seen.has(k)) continue;
        if(k != null) seen.add(k);
        out.push(e);
      }
      return out;
    }

    // removeHistory(opaqueId): delete only the matching entry.
    async function removeHistoryImpl(opaqueId){
      if(opaqueId == null) return;
      await safeWrite(function(){ return backends.history.delete(opaqueId); });
    }

    // clearHistory(): empty the store.
    async function clearHistoryImpl(){
      await safeWrite(function(){ return backends.history.clear(); });
    }

    // updateHistoryId(oldId, newId): self-heal rewrite. opaqueId is the keyPath,
    // so changing it means re-keying — put the same metadata/timestamp under the
    // new opaqueId, then delete the old record (Req 7.4 helper). put-before-
    // delete avoids losing the entry if the second write fails. No-ops when the
    // old entry is absent or the ids are missing/identical.
    async function updateHistoryIdImpl(oldId, newId){
      if(oldId == null || newId == null || oldId === newId) return;
      const all = await backends.history.getAll();
      const found = all.find(function(e){ return historyKey(e) === oldId; });
      if(!found) return;
      const moved = Object.assign({}, found, { opaqueId: newId });
      await safeWrite(async function(){
        await backends.history.put(moved);
        await backends.history.delete(oldId);
      });
    }

    // ---- public API ------------------------------------------------------
    // Favorites + search history (2.3), browsing history (2.4), and `visible`
    // (2.2) are live. Every method is wrapped so a backend failure can never
    // throw into a render path (Req 6.3).
    return {
      // internals reused by later tasks + tests
      SCHEMA_VERSION: SCHEMA_VERSION, KEYS: KEYS, IDB: IDB, CAPS: CAPS,
      wrap: wrap, migrate: migrate, backends: backends,
      _safe: safe, _safeAsync: safeAsync,

      // pure helpers (task 2.2) — exposed for render/route code + test harnesses.
      // The list-based filter is exposed as `_filterSearches` so it does not
      // collide with the `filterSearches(text)` store member added in task 2.3
      // (which will wrap this pure helper over `listSearches()`).
      _filterSearches:  safe(filterSearches, function(){ return []; }),
      entryRoute:       safe(entryRoute, ""),
      dedupeMoveToFront: safe(dedupeMoveToFront, function(){ return []; }),
      toggleMembership: safe(toggleMembership, function(){ return { list: [], added: false }; }),

      // favorites (localStorage) — task 2.3
      isFavorite:       safe(isFavoriteImpl, false),
      addFavorite:      safe(addFavoriteImpl, undefined),
      removeFavorite:   safe(removeFavoriteImpl, undefined),
      toggleFavorite:   safe(toggleFavoriteImpl, false),
      listFavorites:    safe(listFavoritesImpl, function(){ return []; }),
      updateFavoriteId: safe(updateFavoriteIdImpl, undefined),

      // search history (localStorage) — task 2.3
      recordSearch:     safe(recordSearchImpl, undefined),
      listSearches:     safe(listSearchesImpl, function(){ return []; }),
      filterSearches:   safe(filterSearchesImpl, function(){ return []; }),
      removeSearch:     safe(removeSearchImpl, undefined),
      clearSearches:    safe(clearSearchesImpl, undefined),

      // browsing history (IndexedDB) — task 2.4
      recordHistory:    safeAsync(recordHistoryImpl, undefined),
      listHistory:      safeAsync(listHistoryImpl, function(){ return []; }),
      removeHistory:    safeAsync(removeHistoryImpl, undefined),
      clearHistory:     safeAsync(clearHistoryImpl, undefined),
      updateHistoryId:  safeAsync(updateHistoryIdImpl, undefined),

      // adult-gated visibility filter (task 2.2)
      visible:          safe(visible, function(){ return []; }),
      // stable content key derived from an opaque id (survives id rotation)
      stableKey:        safe(stableKey, ""),
    };
  })();

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
      ["lmanime", "#/browse/lmanime", "Anime EN", I.anime, false],
      ["movie", "#/browse/movie", "Movies", I.donghua, false],
      ["drama", "#/browse/drama", "Drama", I.donghua, false],
      ["manga", "#/browse/manga", "Comics", I.manga, false],
      ["novel", "#/browse/novel", "Novel", I.novel, false],
      ["schedule", "#/schedule", "Schedule", I.calendar, false],
      ["library", "#/library", "Library", I.book, false],
    ];
    if (adultOn()) {
      items.push(["cosplay", "#/browse/cosplay", "Cosplay", I.cosplay, true]);
      items.push(["doujin", "#/browse/doujin", "Doujin", I.doujin, true]);
      items.push(["nekopoi", "#/browse/nekopoi", "Hentai", I.anime, true]);
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
      case "watchlm": return "lmanime";
      case "read":   return parts[1] === "nhentai" ? "doujin" : (parts[1] || "");
      case "xref":   return "cosplay";                // cosplay cross-reference -> Cosplay nav
      case "schedule": return "schedule";             // donghua release schedule
      case "library": return "library";
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

  // Build the desktop nav markup: provider links split across two centered
  // rows (so a growing navbar stays tidy) plus the "More" tools menu.
  function deskNavMarkup(links, tools, seg){
    const half = Math.ceil(links.length / 2);
    const row1 = links.slice(0, half);
    const row2 = links.slice(half);
    const navmore = `<div class="navmore">
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
    return (
      `<div class="nav-row">${row1.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")}</div>` +
      `<div class="nav-row">${row2.map(([s,href,label,ico,adult])=>navItem(s,href,label,ico,adult,seg)).join("")}${navmore}</div>`
    );
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
        <div class="hdr-top">
          <button class="icon-btn hamburger" id="hamburger">${I.menu}</button>
          <a class="brand" href="#/">${brandMark()}</a>
          <form class="searchbox" id="searchform">
            ${I.search}
            <input id="searchinput" type="search" placeholder="Search titles..." autocomplete="off"
                   role="combobox" aria-autocomplete="list" aria-controls="searchSuggest" aria-expanded="false">
            <div id="searchSuggest" role="listbox" aria-label="Recent searches" hidden></div>
          </form>
          <button class="icon-btn ${adultOn()?"on":""}" id="adultBtn" title="18+ Content">18+</button>
          <button class="icon-btn" id="themeBtn" title="Toggle theme">${themeIco}</button>
        </div>
        <div class="hdr-nav">
          <nav class="desktop" id="deskNav"></nav>
        </div>
      </header>
      <main id="view"></main>
      <footer>${footerHtml()}</footer>`;

    // search + suggestions dropdown (Req 2.1–2.8)
    const form = document.getElementById("searchform");
    const input = document.getElementById("searchinput");
    const suggest = document.getElementById("searchSuggest");
    // suggRows is a unified, visual-order list of row descriptors:
    //   { type:"suggest", data:Suggestion }  — live catalog suggestion (no ×)
    //   { type:"history", data:SearchEntry } — local search history (has ×)
    // data-idx on each `.sugg-row` indexes into this array, so the keyboard
    // highlight + Enter + click handlers all traverse both kinds in order.
    let suggRows = [];
    let suggSel = -1;    // index of the aria-selected row, -1 = none highlighted
    let suggBlurT = null;
    let suggestT = null; // debounce timer for the live /suggest fetch

    const suggOpen = () => suggest && !suggest.hidden;

    function closeSuggest(){
      if(!suggest) return;
      suggest.hidden = true;
      suggest.innerHTML = "";
      suggRows = [];
      suggSel = -1;
      input.setAttribute("aria-expanded","false");
      input.removeAttribute("aria-activedescendant");
    }

    // Render the dropdown from a unified row list. Empty list hides the
    // dropdown entirely (Req 2.8). All text is escaped via h(...). Live
    // suggestion rows (`.sugg-row.suggest`) have no remove (×) button; history
    // rows keep theirs plus the "Clear all" footer.
    function renderRows(rows){
      if(!suggest) return;
      suggRows = Array.isArray(rows) ? rows : [];
      if(!suggRows.length){ closeSuggest(); return; }
      suggSel = -1;
      const hasSuggest = suggRows.some(r=>r.type==="suggest");
      const hasHistory = suggRows.some(r=>r.type==="history");
      let html = "";
      suggRows.forEach((r,i)=>{
        if(r.type==="suggest"){
          // Section head before the first suggestion row (only if mixed).
          if(hasHistory && i===0) html += `<div class="sugg-head">Suggestions</div>`;
          const d = r.data || {};
          if(d.type==="tag"){
            // Faceted tag: "Parody: Genshin Impact" -> chip + name.
            const m = String(d.label||"").split(/:\s+/);
            const facet = m.length>1 ? m[0] : "Tag";
            const name = m.length>1 ? m.slice(1).join(": ") : (d.label||d.value||"");
            html += `<div class="sugg-row suggest tag" role="option" id="sugg-${i}" data-idx="${i}" aria-selected="false">`
              + `<span class="sugg-ico">${I.search}</span>`
              + `<span class="sugg-facet">${h(facet)}</span>`
              + `<span class="sugg-q">${h(name)}</span>`
              + `</div>`;
          } else {
            const kp = Object.values(PROVIDERS).find(p=>p.kind===d.kind);
            const kindBadge = kp ? `<span class="sugg-kind">${h(kp.label)}</span>` : "";
            html += `<div class="sugg-row suggest" role="option" id="sugg-${i}" data-idx="${i}" aria-selected="false">`
              + `<span class="sugg-ico">${I.search}</span>`
              + `<span class="sugg-q">${h(d.label)}</span>`
              + kindBadge
              + `</div>`;
          }
        } else {
          // Section head before the first history row when suggestions precede it.
          const firstHist = hasSuggest && !suggRows.slice(0,i).some(x=>x.type==="history");
          if(firstHist) html += `<div class="sugg-head">Recent</div>`;
          html += `<div class="sugg-row" role="option" id="sugg-${i}" data-idx="${i}" aria-selected="false">`
            + `<span class="sugg-ico">${I.search}</span>`
            + `<span class="sugg-q">${h(r.data.query)}</span>`
            + `<button class="sugg-x" type="button" data-x="${i}" title="Remove" aria-label="Remove ${h(r.data.query)}">${I.close}</button>`
            + `</div>`;
        }
      });
      if(hasHistory) html += `<div class="sugg-foot"><button class="sugg-clear" type="button">Clear all</button></div>`;
      suggest.innerHTML = html;
      suggest.hidden = false;
      input.setAttribute("aria-expanded","true");
      input.removeAttribute("aria-activedescendant");
    }

    // Wrap a SearchEntry list as history rows.
    const histRows = (entries)=> (Array.isArray(entries)?entries:[]).map(e=>({ type:"history", data:e }));
    // Order live suggestions tag-first, then titles, as row descriptors.
    function suggestRows(list){
      const arr = Array.isArray(list) ? list : [];
      const tags = arr.filter(s=>s && s.type==="tag");
      const titles = arr.filter(s=>s && s.type!=="tag");
      return tags.concat(titles).map(s=>({ type:"suggest", data:s }));
    }

    // focus -> most-recent-first history (Req 2.3); input -> filtered (Req 2.4)
    // plus live catalog suggestions fetched (debounced) from /api/v1/suggest.
    const showRecent = () => renderRows(histRows(pstore.listSearches()));
    function refreshSuggest(){
      const v = input.value;
      const trimmed = v.trim();
      // Always render history immediately so typing never stalls on the network.
      renderRows(histRows(trimmed === "" ? pstore.listSearches() : pstore.filterSearches(v)));

      if(suggestT){ clearTimeout(suggestT); suggestT = null; }
      if(trimmed.length < 2) return; // too short to bother the catalog
      suggestT = setTimeout(async ()=>{
        const query = trimmed;
        let list = [];
        try {
          const res = await apiCached("/suggest?" + qs({ q: query, source: "all" }));
          // Out-of-order guard: bail if the user kept typing.
          if(input.value.trim() !== query) return;
          list = (res && Array.isArray(res.suggestions)) ? res.suggestions : [];
        } catch(_){
          return; // on failure just keep history rows — never break typing
        }
        if(input.value.trim() !== query) return;
        const sugg = suggestRows(list);
        const hist = histRows(pstore.filterSearches(input.value));
        renderRows(sugg.concat(hist));
      }, 200);
    }

    // Run a query as a normal search (records history + navigates).
    function runSearch(q){
      const query = (q==null?"":String(q)).trim();
      if(!query) return;
      pstore.recordSearch(query);
      closeSuggest();
      go(`#/search/${encodeURIComponent(query)}`);
    }

    // Act on a live suggestion row. Tags navigate to their exact-tag search
    // (scoped to doujin when that's the suggestion's kind); titles run a normal
    // search on the suggestion value. Both record the search for history.
    function selectSuggestion(s){
      if(!s) return;
      const value = (s.value==null?"":String(s.value)).trim();
      if(!value) return;
      pstore.recordSearch(value);
      closeSuggest();
      if(s.type==="tag" && s.kind==="doujin"){
        go(`#/search/${encodeURIComponent(value)}/doujin/1`);
      } else {
        go(`#/search/${encodeURIComponent(value)}`);
      }
    }

    // Dispatch a row (by its unified-list index) to the right action.
    function activateRow(i){
      const r = suggRows[i];
      if(!r) return;
      if(r.type==="suggest") selectSuggestion(r.data);
      else runSearch(r.data.query);
    }

    // Move the aria-selected highlight, wrapping at both ends.
    function highlight(idx){
      const rows = suggest.querySelectorAll(".sugg-row");
      if(!rows.length){ suggSel = -1; return; }
      if(idx < 0) idx = rows.length - 1;
      else if(idx >= rows.length) idx = 0;
      suggSel = idx;
      rows.forEach((r,i)=>{
        const on = i === idx;
        r.setAttribute("aria-selected", on ? "true" : "false");
        if(on){ input.setAttribute("aria-activedescendant", r.id); r.scrollIntoView({block:"nearest"}); }
      });
    }

    // Record the query BEFORE navigating to the search route (Req 2.1, 2.5).
    // (runSearch / selectSuggestion / activateRow are defined above.)

    form.addEventListener("submit", (e)=>{
      e.preventDefault();
      // Enter on a highlighted row activates it (suggestion vs history);
      // otherwise submit the typed text as a search.
      if(suggOpen() && suggSel >= 0 && suggRows[suggSel]){ activateRow(suggSel); return; }
      runSearch(input.value);
    });

    input.addEventListener("focus", ()=>{
      if(suggBlurT){ clearTimeout(suggBlurT); suggBlurT = null; }
      showRecent();
    });
    input.addEventListener("input", refreshSuggest);

    input.addEventListener("keydown", (e)=>{
      if(e.key === "ArrowDown"){
        if(!suggOpen()){ showRecent(); if(suggOpen()) highlight(0); }
        else highlight(suggSel + 1);
        e.preventDefault();
      } else if(e.key === "ArrowUp"){
        if(suggOpen()){ highlight(suggSel - 1); e.preventDefault(); }
      } else if(e.key === "Escape"){
        if(suggOpen()){ closeSuggest(); e.preventDefault(); }
      }
    });

    // Keep input focus on dropdown interaction so the blur timeout never fires
    // before a click is processed.
    suggest.addEventListener("mousedown", (e)=>{ e.preventDefault(); });
    // Delegated: per-row remove (×), footer clear-all, or pick a row (Req 2.5–2.7).
    suggest.addEventListener("click", (e)=>{
      const x = e.target.closest(".sugg-x");
      if(x){
        const r = suggRows[+x.getAttribute("data-x")];
        if(r && r.type==="history") pstore.removeSearch(r.data.query);
        refreshSuggest();
        return;
      }
      if(e.target.closest(".sugg-clear")){
        pstore.clearSearches();
        closeSuggest();
        return;
      }
      const row = e.target.closest(".sugg-row");
      if(row){ activateRow(+row.getAttribute("data-idx")); }
    });

    // Dismiss on blur (timeout so row clicks register) and on outside click.
    input.addEventListener("blur", ()=>{ suggBlurT = setTimeout(closeSuggest, 150); });
    document.addEventListener("click", (e)=>{ if(!e.target.closest(".searchbox")) closeSuggest(); });

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

  // Local port of the server's `split_cosplay_title` (src/web/search.rs). Used
  // only as a fallback when the cosplay search DTO did not carry a confident
  // `character`/`cosplayer` split. Pure, total, and panic-free: returns either
  // both names or neither, never exactly one side (Req 1.1, 1.9).
  function cleanCosplayTitle(raw){
    let t = (raw==null?"":String(raw)).trim();
    // strip " - Cosplaytele" / " – Cosplaytele" (en-dash) site suffix
    t = t.replace(/\s[-\u2013]\sCosplaytele$/i, "");
    // strip "<N> photos/images/pics/videos/clips/..." count clauses
    t = t.replace(/\s*\d+\s*(?:photos?|images?|pics?|pictures?|videos?|clips?|movies?)/gi, "");
    // drop dangling connectors left behind (e.g. "Foo Bar and")
    t = t.replace(/[\s,&+]+(?:and)?[\s,&+]*$/i, "");
    return t.split(/\s+/).filter(Boolean).join(" ");
  }
  function splitCosplayTitle(title, snippet){
    const cleaned = cleanCosplayTitle(title);
    const tokens = cleaned.split(/\s+/).filter(Boolean);
    const none = { character: null, cosplayer: null };
    if(tokens.length < 2) return none;

    // (2) explicit " by " separator (case-insensitive) — strongest signal.
    const byIdx = tokens.findIndex(t=>t.toLowerCase()==="by");
    if(byIdx >= 1 && byIdx + 1 < tokens.length){
      const character = tokens.slice(0, byIdx).join(" ");
      const cosplayer = tokens.slice(byIdx + 1).join(" ");
      if(character && cosplayer) return { character, cosplayer };
    }

    // (3) snippet/category prefix hint — longest title prefix the snippet
    // starts with becomes the character, the remainder the cosplayer.
    const snip = (snippet==null?"":String(snippet)).trim().toLowerCase();
    if(snip){
      for(let k = tokens.length - 1; k >= 1; k--){
        const prefix = tokens.slice(0, k).join(" ");
        if(snip.startsWith(prefix.toLowerCase())){
          const cosplayer = tokens.slice(k).join(" ");
          if(cosplayer) return { character: prefix, cosplayer };
        }
      }
    }

    // (4) exactly-two-token fallback: cosplaytele convention is character first.
    if(tokens.length === 2) return { character: tokens[0], cosplayer: tokens[1] };

    // (5) cannot confidently split -> plain title (Req 1.9).
    return none;
  }

  // Resolve the cosplay (character, cosplayer) split for a search card: prefer
  // the DTO fields the server already computed, else fall back to the local
  // heuristic over the title (+ snippet). Returns null when no confident split
  // is available, so the caller renders the plain title (Req 1.9).
  function cosplaySplit(item){
    if(!item || item.kind !== "cosplay") return null;
    let character = item.character, cosplayer = item.cosplayer;
    if(!(character && cosplayer)){
      const s = splitCosplayTitle(item.title, item.snippet);
      character = s.character; cosplayer = s.cosplayer;
    }
    return (character && cosplayer) ? { character, cosplayer } : null;
  }

  // Language badge: distinguishes ID-subbed vs EN-subbed entries (anime shows
  // up from both otakudesu = ID and lmanime = EN, sometimes the same title in
  // both), so the source badge alone isn't enough.
  function langBadge(item){
    const src = item.source || "";
    let lang = "";
    if(src==="lmanime") lang = "EN";
    else if(src==="otakudesu" || src==="otakudesufit" || src==="anichin") lang = "ID";
    if(!lang) return "";
    return `<span class="badge lang ${lang.toLowerCase()}">${lang}</span>`;
  }

  function cardHtml(item){
    const prov = Object.values(PROVIDERS).find(p=>p.kind===item.kind) || {};
    const tags = (item.tags||[]).slice(0,2).map(t=>`<span>${h(t)}</span>`).join("");
    const detailHref = `#/detail/${encodeURIComponent(item.kind)}/${encodeURIComponent(item.id)}`;
    // Cosplay cards with a confident split render the title slot as two tappable
    // cross-reference pills (character + cosplayer) instead of plain text
    // (Req 1.1–1.3). The pills are real anchors carrying `data-stop` so the
    // delegated [data-go] card handler does not also fire on them.
    const split = cosplaySplit(item);
    const titleHtml = split
      ? `<div class="t pills">`+
          `<a class="pill link" data-stop href="#/xref/character/${encodeURIComponent(split.character)}">${h(split.character)}</a>`+
          `<a class="pill link" data-stop href="#/xref/cosplayer/${encodeURIComponent(split.cosplayer)}">${h(split.cosplayer)}</a>`+
        `</div>`
      : `<div class="t">${h(item.title)}</div>`;
    return `
      <div class="card" data-go="${detailHref}" data-prefetch-kind="${h(item.kind)}" data-prefetch-id="${h(item.id)}">
        <div class="poster">${imgTag(item.thumbnail,"",item.title)}<span class="badge src">${h(prov.label||item.source)}</span>${langBadge(item)}</div>
        <div class="meta">${titleHtml}<div class="sub">${tags}</div></div>
      </div>`;
  }
  const grid = (items) => (!items||!items.length) ? `<div class="empty">No results found.</div>` : `<div class="grid">${items.map(cardHtml).join("")}</div>`;

  // ---- Genre pills --------------------------------------------------------
  // Detail pages list a series' genres. For providers whose /browse accepts an
  // arbitrary genre slug (otakudesu=anime, novelid=novel) we render each genre
  // as a tappable pill that jumps to that genre's browse feed, so visitors can
  // discover everything in e.g. "Romance" or "Horror" with one tap. Providers
  // without genre browse (donghua, manga) keep plain, non-clickable pills.
  const GENRE_BROWSE = { anime: true, novel: true, lmanime: true, movie: true };
  const genreSlug = (g) => String(g||"").trim().toLowerCase()
    .replace(/&/g, "and")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  function genrePill(kind, g){
    const slug = genreSlug(g);
    if(GENRE_BROWSE[kind] && slug){
      return `<a class="pill link" href="#/browse/${encodeURIComponent(kind)}/${encodeURIComponent(slug)}">${h(g)}</a>`;
    }
    return `<span class="pill">${h(g)}</span>`;
  }

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

  document.addEventListener("click", (e)=>{
    // Cross-reference pills (and any [data-stop] anchor) navigate via their own
    // href; let the browser handle the hash change and don't trigger the card's
    // data-go navigation underneath them.
    if(e.target.closest("[data-stop]")) return;
    const el=e.target.closest("[data-go]"); if(el){ e.preventDefault(); go(el.dataset.go); }
  });

  // Delegated favorite toggle (Req 4.2–4.4). One listener handles every
  // `.fav-btn` rendered into a detail hero. Toggling goes through the store and
  // the returned membership drives an IN-PLACE flip of the button's .on class,
  // aria-pressed, and label — no re-render, so scroll position and the rest of
  // the page are untouched.
  document.addEventListener("click", (e)=>{
    const btn = e.target.closest(".fav-btn");
    if(!btn) return;
    e.preventDefault();
    let meta = {};
    try{ meta = JSON.parse(btn.getAttribute("data-fav") || "{}"); }catch(_){ meta = {}; }
    const on = pstore.toggleFavorite(meta);
    btn.classList.toggle("on", on);
    btn.setAttribute("aria-pressed", on ? "true" : "false");
    const lbl = btn.querySelector("span");
    if(lbl) lbl.textContent = on ? "Saved" : "Save";
  });

  // Warm the detail endpoint when the pointer hovers a card (desktop) or on
  // first touch (mobile) so opening it feels instant.
  function _prefetchCard(el){
    const kind = el.dataset.prefetchKind, id = el.dataset.prefetchId;
    if(!kind || !id) return;
    const ep = DETAIL_EP[kind];
    if(!ep) return;
    const eid = encodeURIComponent(id);
    // Warm the EXACT path the detail route fetches for this kind, so the
    // cache-peek can hit it (a mismatched key both wastes the fetch and, worse,
    // can route the wrong renderer).
    if(kind==="anime" || kind==="lmanime"){ const cfg = ANIME_LIKE[kind]; prefetch(`/${cfg.api}/${eid}`); }
    else if(kind==="cosplay") prefetch(`/cosplay/${eid}`);
    else if(kind==="doujin") prefetch(`/nhentai/${eid}`);
    else if(kind==="movie") prefetch(`/movie/${eid}`);
    else if(kind==="nekopoi") prefetch(`/nekopoi/${eid}`);
    else if(kind==="drama") prefetch(`/drama/${eid}`);
    else if(kind==="donghua") prefetch(`/${ep}/${eid}?${qs({page:1,size:EPISODE_SIZE})}`);
    else prefetch(`/${ep}/${eid}?${qs({page:1,size:CHAPTER_SIZE})}`);
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
      { title:"Anime Ongoing",   prov:"anime",       feed:"ongoing",       seg:"anime", dedupe:true },
      { title:"Latest Donghua",  prov:"anichin",     feed:"home",          seg:"donghua" },
      { title:"Anime EN Ongoing",prov:"lmanime",     feed:"ongoing",       seg:"lmanime" },
      { title:"Popular Movies",  prov:"lk21",        feed:"populer",       seg:"movie" },
      { title:"Popular Comics",  prov:"mangaball",   feed:"popular",       seg:"manga" },
      { title:"Latest Novels",   prov:"novelid",     feed:"home",          seg:"novel" },
      { title:"Latest Cosplay",  prov:"cosplaytele", feed:"home",          seg:"cosplay", adult:true },
      { title:"Today's Doujin",  prov:"nhentai",     feed:"popular-today", seg:"doujin",  adult:true },
    ].filter(s=>!s.adult||adultOn());
    rows.innerHTML = sections.map((s,i)=>`
      <div class="row-head"><h2><span class="dot"></span>${h(s.title)}</h2><a class="more" href="#/browse/${s.seg}">View all ${I.arrow}</a></div>
      <div id="row-${i}">${skelGrid(6)}</div>`).join("");
    sections.forEach(async (s,i)=>{
      try { const data=await apiCached(`/browse/${s.prov}?${qs(Object.assign({feed:s.feed}, s.dedupe?{dedupe:1}:{}))}`); document.getElementById(`row-${i}`).innerHTML=grid((data.items||[]).slice(0,12)); }
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

  // ---- Cosplay cross-reference (#/xref/{scope}/{name}) --------------------
  // A pinned-context, cosplay-scoped lookup opened from a cosplay search card's
  // character/cosplayer pill. scope ∈ {character, cosplayer}. Cosplay is adult-
  // gated, so this redirects home when the 18+ toggle is off (Req 1.1–1.11).
  async function routeXref(scope, name){
    if(!adultOn()) return routeHome();              // cosplay is adult-gated
    scope = scope === "cosplayer" ? "cosplayer" : "character";
    name = (name==null?"":String(name));

    // Pinned-context header — visually distinct from the search results list
    // (Req 1.7) — plus a loading spinner while the live lookup runs (Req 1.8).
    const scopeLabel = scope === "cosplayer" ? "Cosplayer" : "Character";
    const heading = scope === "cosplayer"
      ? `More by ${h(name)}`
      : `Other cosplays of ${h(name)}`;
    const sub = scope === "cosplayer"
      ? "Other characters this cosplayer has done."
      : "Who else cosplayed this character.";
    shell(`
      ${crumbs([{href:"#/",label:"Home"},{href:"#/browse/cosplay",label:"Cosplay"},{label:scopeLabel}])}
      <div class="xref">
        <div class="xref-head">
          <div class="xref-ico">${I.cosplay}</div>
          <div class="xref-txt">
            <div class="xref-scope">${h(scopeLabel)}</div>
            <h1>${heading}</h1>
            <p class="xref-sub">${sub}</p>
          </div>
        </div>
        <div id="xrefResults">${spinner}</div>
      </div>
    `);

    // Live cosplay-scoped lookup. Results link to #/detail/cosplay/{id} via the
    // shared cardHtml/grid (Req 1.6). Empty -> empty-state (Req 1.10); failure
    // -> error message + retry control (Req 1.11).
    const results = document.getElementById("xrefResults");
    try{
      const data = await apiCached(`/search?${qs({q:name, source:"cosplay", page:1})}`);
      const items = (data.items||[]).filter(it=>it.kind==="cosplay");
      if(!results) return;
      if(!items.length){
        results.innerHTML = `<div class="empty xref-empty">No other cosplay posts found for &ldquo;${h(name)}&rdquo;.</div>`;
        return;
      }
      results.innerHTML = grid(items);
    }catch(e){
      if(!results) return;
      results.innerHTML =
        `<div class="errbox xref-err">`+
          `<p>Couldn&rsquo;t load cross-reference results: ${h(e.message||"request failed")}</p>`+
          `<button type="button" class="btn" id="xrefRetry">Retry</button>`+
        `</div>`;
      const retry = document.getElementById("xrefRetry");
      if(retry) retry.addEventListener("click", ()=>routeXref(scope, name));
    }
  }

  // ===========================================================================
  // Donghua release schedule (#/schedule)
  // ===========================================================================
  // Weekly release calendar. The server groups upcoming donghua episodes by
  // weekday; each item is already shaped like a search result (id/kind/title/
  // thumbnail) so the shared cardHtml/grid renders them as-is, with the
  // upcoming episode surfaced as a card sub-tag. Cards link to
  // #/detail/donghua/{id}. Mirrors routeXref for the empty / error states.
  //
  // Each item also carries `time_label` (e.g. "at 09:47" / "released") and an
  // optional `release_at` (unix SECONDS) for upcoming episodes. We surface both
  // the episode number and the timing as card sub-tags, and — when an episode
  // is still in the future — render a LIVE countdown that ticks in place.

  // Module-scoped countdown timer. Only one runs at a time: it is cleared at
  // the start of every routeSchedule() call and self-clears once the schedule
  // container leaves the DOM (the SPA swaps #view on each route).
  let _schedTimer = null;

  // Format remaining seconds as "0d 3h 36m". Non-positive input collapses to
  // "0d 0h 0m" (callers fall back to the time_label/"released" in that case).
  function fmtCountdown(rem){
    let r = Math.max(0, Math.floor(rem||0));
    const d = Math.floor(r/86400);
    const hours = Math.floor((r%86400)/3600);
    const mins = Math.floor((r%3600)/60);
    return `${d}d ${hours}h ${mins}m`;
  }

  // The timing label for one schedule item at "now": a live countdown for a
  // future release_at, otherwise the verbatim time_label (or "released").
  function schedTiming(it){
    const ra = (it && typeof it.release_at === "number") ? it.release_at : null;
    const future = ra != null && ra*1000 > Date.now();
    if(future) return fmtCountdown(ra - Math.floor(Date.now()/1000));
    return (it && it.time_label) ? String(it.time_label) : "released";
  }

  async function routeSchedule(){
    // Clear any countdown timer from a previous visit before we re-render.
    if(_schedTimer){ clearInterval(_schedTimer); _schedTimer = null; }

    shell(`
      ${crumbs([{href:"#/",label:"Home"},{label:"Donghua Schedule"}])}
      <div class="row-head"><h2><span class="dot"></span>Donghua Release Schedule</h2></div>
      <div id="scheduleBody">${spinner}</div>
    `);

    // Dedicated schedule-card render: same markup contract as cardHtml (so
    // [data-go] navigation + pointer prefetch work identically) but with timing
    // sub-tags that carry a `data-rls` hook for the live countdown. cardHtml's
    // generic `.sub` tags are plain escaped <span>s and can't carry that hook,
    // hence this small local renderer. All text stays XSS-safe via h().
    const schedCard = (it)=>{
      const prov = Object.values(PROVIDERS).find(p=>p.kind===it.kind) || {};
      const detailHref = `#/detail/${encodeURIComponent(it.kind)}/${encodeURIComponent(it.id)}`;
      const ra = (typeof it.release_at === "number") ? it.release_at : null;
      const epTag = it.episode ? `<span class="sched-eps">${h("Eps "+it.episode)}</span>` : "";
      const timeTag = `<span class="sched-time"${ra!=null?` data-rls="${h(ra)}"`:""}>${h(schedTiming(it))}</span>`;
      return `
      <div class="card" data-go="${detailHref}" data-prefetch-kind="${h(it.kind)}" data-prefetch-id="${h(it.id)}">
        <div class="poster">${imgTag(it.thumbnail,"",it.title)}<span class="badge src">${h(prov.label||it.source)}</span></div>
        <div class="meta"><div class="t">${h(it.title)}</div><div class="sub">${epTag}${timeTag}</div></div>
      </div>`;
    };
    const schedGrid = (items)=> (!items||!items.length)
      ? `<div class="empty">No results found.</div>`
      : `<div class="grid">${items.map(schedCard).join("")}</div>`;

    const body = document.getElementById("scheduleBody");
    try{
      const data = await apiCached("/donghua/schedule");
      if(!body) return;
      const days = (data && Array.isArray(data.days)) ? data.days : [];
      if(!days.length){
        body.innerHTML = `<div class="empty">No scheduled releases right now.</div>`;
        return;
      }
      // Highlight the current weekday (cheap, locale-based comparison).
      let today = "";
      try { today = new Date().toLocaleDateString("en-US",{weekday:"long"}); }catch(_){}
      let hasFuture = false;
      body.innerHTML = days.map(d=>{
        const items = (d.items||[]);
        items.forEach(it=>{ if(typeof it.release_at === "number" && it.release_at*1000 > Date.now()) hasFuture = true; });
        const isToday = today && d.day && String(d.day).toLowerCase()===today.toLowerCase();
        return `<div class="row-head${isToday?" sched-today":""}"><h2><span class="dot"></span>${h(d.day)}</h2></div>`+schedGrid(items);
      }).join("");

      // Live countdown: recompute the `[data-rls]` labels in place every 60s.
      // Self-cleans once #scheduleBody leaves the DOM (user navigated away).
      if(hasFuture){
        _schedTimer = setInterval(()=>{
          const el = document.getElementById("scheduleBody");
          if(!el){ clearInterval(_schedTimer); _schedTimer = null; return; }
          const now = Math.floor(Date.now()/1000);
          let anyFuture = false;
          el.querySelectorAll(".sched-time[data-rls]").forEach(span=>{
            const ra = +span.getAttribute("data-rls");
            if(!isFinite(ra)) return;
            const rem = ra - now;
            if(rem > 0){ anyFuture = true; span.textContent = fmtCountdown(rem); }
            else { span.textContent = "released"; span.removeAttribute("data-rls"); }
          });
          if(!anyFuture){ clearInterval(_schedTimer); _schedTimer = null; }
        }, 60000);
      }
    }catch(e){
      if(!body) return;
      body.innerHTML =
        `<div class="errbox">`+
          `<p>Couldn&rsquo;t load the schedule: ${h(e.message||"request failed")}</p>`+
          `<button type="button" class="btn" id="schedRetry">Retry</button>`+
        `</div>`;
      const retry = document.getElementById("schedRetry");
      if(retry) retry.addEventListener("click", ()=>routeSchedule());
    }
  }

  // ===========================================================================
  // Unified Library page (#/library)
  // ===========================================================================
  // Per-kind group order + display labels for the Library sections. "comics" is
  // the display label for the "manga" provider kind (Req 5.3, 5.4).
  const LIB_GROUPS = [
    ["anime",   "Anime"],
    ["donghua", "Donghua"],
    ["manga",   "Comics"],
    ["novel",   "Novel"],
    ["cosplay", "Cosplay"],
    ["doujin",  "Doujin"],
  ];

  // Build a single metadata-only Library card from a stored RichMetadata entry.
  // Unlike cardHtml (which expects a live search item with `id`), this renders
  // purely from what the store persisted: title, thumbnail, kind, opaqueId
  // (Req 5.7). Navigation uses the canonical per-kind route via entryRoute
  // (Req 5.5). A `.lib-rm` remove control is overlaid, carrying `data-stop` so
  // the card's own [data-go] navigation does not fire when it is tapped.
  // Compact relative time: "just now", "5m", "2h", "3d", "2w", else a date.
  function relTime(ts){
    const t = Number(ts);
    if(!t) return "";
    const diff = Date.now() - t;
    if(diff < 0) return "just now";
    const s = Math.floor(diff/1000);
    if(s < 60) return "just now";
    const m = Math.floor(s/60); if(m < 60) return m+"m ago";
    const hr = Math.floor(m/60); if(hr < 24) return hr+"h ago";
    const d = Math.floor(hr/24); if(d < 7) return d+"d ago";
    const w = Math.floor(d/7); if(w < 5) return w+"w ago";
    try { return new Date(t).toLocaleDateString(); } catch(_){ return ""; }
  }

  // The per-leaf watch/read route for a stored progress marker — opening a
  // history entry jumps straight back to the episode/chapter you last left off
  // at, rather than the series landing page.
  function progressRoute(kind, pid){
    if(!pid) return null;
    const id = encodeURIComponent(pid);
    switch(kind){
      case "donghua": return `#/watch/${id}`;
      case "anime":   return `#/watchanime/${id}`;
      case "lmanime": return `#/watchlm/${id}`;
      case "manga":   return `#/read/manga/${id}`;
      case "novel":   return `#/read/novel/${id}`;
      case "doujin":  return `#/read/nhentai/${id}`;
      default:        return null;   // cosplay / movie: no per-leaf resume
    }
  }

  function libCardHtml(entry, tab){
    const kind = entry && entry.kind;
    const prov = Object.values(PROVIDERS).find(p=>p.kind===kind) || {};
    const id = entry && entry.opaqueId;
    // Progress chip ("Ep 14" / "Ch. 1182") + last-opened relative time give the
    // library the "where was I" context the user asked for.
    const prog = entry && entry.progress;
    const progLabel = prog && (prog.label || (prog.number!=null ? ((prog.type==="chapter"?"Ch. ":"Ep ")+prog.number) : ""));
    const when = relTime(entry && entry.timestamp);
    // When a progress marker carries the leaf id, the card resumes straight to
    // that episode/chapter; otherwise it opens the series detail page.
    const resume = (prog && prog.id) ? progressRoute(kind, prog.id) : null;
    const route = resume || pstore.entryRoute(kind, id);
    const subBits = [];
    if(progLabel) subBits.push(`<span class="lib-prog${resume?" resume":""}">${prog && prog.type==="chapter" ? I.book : I.play}${resume?"Resume ":""}${h(progLabel)}</span>`);
    if(when) subBits.push(`<span class="lib-when">${h(when)}</span>`);
    const sub = subBits.length ? `<div class="lib-sub">${subBits.join("")}</div>` : "";
    const resumeBadge = resume ? `<span class="lib-resume-badge" title="Resume ${escAttr(progLabel||"")}">${I.play}</span>` : "";
    return `
      <div class="card" data-go="${escAttr(route)}" data-prefetch-kind="${h(kind)}" data-prefetch-id="${escAttr(id)}">
        <button type="button" class="lib-rm" data-stop data-lib-rm="${escAttr(id)}" data-lib-tab="${h(tab)}" aria-label="Remove">${I.close}</button>
        <div class="poster">${imgTag(entry&&entry.thumbnail,"",entry&&entry.title)}<span class="badge src">${h(prov.label||kind)}</span>${resumeBadge}</div>
        <div class="meta"><div class="t">${h(entry&&entry.title)}</div>${sub}</div>
      </div>`;
  }

  // Partition the visible entries by kind into the fixed group order and render
  // each non-empty group as a .lib-group (title + count + grid of cards). When
  // there are no visible entries at all, render the per-tab empty state
  // (Req 5.3, 5.4, 5.6, 5.7).
  function renderLibraryGroups(visibleEntries, tab){
    const body = document.getElementById("libBody");
    if(!body) return;
    const entries = Array.isArray(visibleEntries) ? visibleEntries : [];
    if(!entries.length){
      body.innerHTML = libEmptyHtml(tab);
      return;
    }
    const sections = [];
    for(const [kind, label] of LIB_GROUPS){
      const inGroup = entries.filter(e=>e && e.kind===kind);
      if(!inGroup.length) continue;
      sections.push(
        `<section class="lib-group">`+
          `<div class="lib-group-head"><h2><span class="dot"></span>${h(label)}</h2>`+
            `<span class="cnt-badge">${inGroup.length}</span></div>`+
          `<div class="grid">${inGroup.map(e=>libCardHtml(e, tab)).join("")}</div>`+
        `</section>`
      );
    }
    body.innerHTML = sections.join("");
  }

  function libEmptyHtml(tab){
    const isFav = tab === "favorites";
    const title = isFav ? "No favorites yet" : "No history yet";
    const msg = isFav
      ? "Tap Save on any title to keep it here."
      : "Content you open will show up here automatically.";
    return `<div class="lib-empty">`+
      `<div class="lib-empty-ico">${isFav ? I.heart : I.book}</div>`+
      `<h3>${title}</h3><p>${msg}</p></div>`;
  }

  // Library page (Req 5.1–5.7). Two tabs: Favorites (sync, from localStorage)
  // and History (async, from IndexedDB). Adult-gated kinds (cosplay/doujin) are
  // dropped via pstore.visible when the 18+ toggle is off (Req 3.10, 4.7).
  async function routeLibrary(tab){
    tab = tab === "history" ? "history" : "favorites";   // default Favorites (Req 5.2)

    const tabLink = (seg, label, ico) =>
      `<a class="lib-tab${seg===tab?" active":""}" role="tab" `+
        `aria-selected="${seg===tab?"true":"false"}" href="#/library/${seg}">`+
        `${ico}<span>${label}</span></a>`;

    // History gets a clear-all toolbar (Req 4.6 sibling: history clear).
    const toolbar = tab === "history"
      ? `<div class="lib-toolbar"><button type="button" class="lib-clear" id="libClear">`+
          `${I.close}<span>Clear all</span></button></div>`
      : "";

    shell(`
      ${crumbs([{href:"#/",label:"Home"},{label:"Library"}])}
      <div class="library" id="library">
        <div class="lib-tabs" role="tablist">
          ${tabLink("favorites", "Favorites", I.heart)}
          ${tabLink("history", "History", I.book)}
        </div>
        ${toolbar}
        <div id="libBody">${spinner}</div>
      </div>
    `);

    // Wire the History clear-all control (Req 4.6 history-clear).
    const clearBtn = document.getElementById("libClear");
    if(clearBtn){
      clearBtn.addEventListener("click", async ()=>{
        await pstore.clearHistory();
        routeLibrary("history");
      });
    }

    // Load the active tab's entries, drop adult-gated kinds when the toggle is
    // off, then group + render (Req 3.10, 4.7, 5.3, 5.4).
    const entries = tab === "favorites"
      ? pstore.listFavorites()                 // sync (localStorage)
      : await pstore.listHistory();            // async (IndexedDB)
    if(activeNavSeg() !== "library") return;   // user navigated away while awaiting
    renderLibraryGroups(pstore.visible(entries), tab);
  }

  // Delegated per-entry remove control for Library cards (Req 4.6 favorites /
  // history). `data-stop` already prevents the card's [data-go] navigation from
  // firing; here we remove from the right collection and re-render the tab.
  document.addEventListener("click", (e)=>{
    const rm = e.target.closest(".lib-rm");
    if(!rm) return;
    e.preventDefault();
    const id = rm.getAttribute("data-lib-rm");
    const tab = rm.getAttribute("data-lib-tab");
    if(tab === "history"){
      Promise.resolve(pstore.removeHistory(id)).then(()=>routeLibrary("history"));
    } else {
      pstore.removeFavorite(id);
      routeLibrary("favorites");
    }
  });


  // ===========================================================================
  // Detail / watch / read
  // ===========================================================================
  const setD = (html) => { const el=document.getElementById("d"); if(el) el.innerHTML=html; };

  // Favorite control (Req 4.1–4.5). Reads current membership from the store at
  // render time so the on/off state always reflects reality. The full
  // RichMetadata needed to toggle is carried on a `data-fav` attribute as
  // HTML-attribute-escaped JSON (escAttr neutralises quotes/angle-brackets, so
  // it is XSS-safe); the delegated handler below parses it back. The .on class
  // AND aria-pressed are both set so the CSS on-state holds either way.
  function favButton(meta){
    const on = pstore.isFavorite(meta && meta.opaqueId);
    return `<button type="button" class="btn fav-btn${on?" on":""}" `+
      `data-fav="${escAttr(JSON.stringify(meta||{}))}" aria-pressed="${on?"true":"false"}">`+
      `${I.heart}<span>${on?"Saved":"Save"}</span></button>`;
  }

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

  // ===========================================================================
  // Stale-ID self-healing (Req 7.2–7.6)
  // ===========================================================================
  // Saved favorites/history keep their opaque id alongside RichMetadata. When
  // the server secret rotates (APIKU_SECRET unset across a restart) a saved id
  // becomes a StaleId: the detail endpoint rejects it with the `invalid_id`
  // envelope. We detect exactly that error, re-resolve the entry by searching
  // its stored title, rewrite the saved id on a hit, and otherwise show an
  // unavailable state that keeps the stored metadata visible. Only saved-entry
  // navigation triggers this path; ordinary browsing is unchanged.

  // isStaleId(e): true only for the API `invalid_id` envelope (signature
  // mismatch / decode failure). `api()` throws Error("<code>: <message>"), so
  // we extract the code token (everything before the first ":") and match it
  // exactly. Network/upstream/not-found/other errors do NOT match, so they
  // never trigger re-resolution (Req 7.3 — gating).
  function isStaleId(e){
    if(!e) return false;
    const msg = (e.message != null ? String(e.message) : String(e)).trim();
    const code = msg.split(":", 1)[0].trim();
    return code === "invalid_id";
  }

  // detailPath(kind, id): the same endpoint routeDetail/renderers use for each
  // kind. Returns null for an unknown kind.
  function detailPath(kind, id){
    const ep = DETAIL_EP[kind];
    if(!ep) return null;
    if(kind==="anime")   return `/anime/${encodeURIComponent(id)}`;
    if(kind==="cosplay") return `/cosplay/${encodeURIComponent(id)}`;
    if(kind==="doujin")  return `/nhentai/${encodeURIComponent(id)}`;
    const size = kind==="donghua" ? EPISODE_SIZE : CHAPTER_SIZE;
    return `/${ep}/${encodeURIComponent(id)}?${qs({page:1,size})}`;
  }

  // fetchDetail(kind, id): resolve a detail document via the cached fetch path
  // (same call the renderers make). Throws on any API error, including the
  // `invalid_id` stale-id error that isStaleId classifies.
  async function fetchDetail(kind, id){
    const path = detailPath(kind, id);
    if(!path) throw new Error(`invalid_kind: ${kind}`);
    return apiCached(path);
  }

  // searchTitle(kind, title): re-resolve a stale entry by searching the
  // provider for its stored title. Scopes the search to the kind's provider
  // (PROVIDERS[kind].api, e.g. anime→otakudesu) and picks the best match:
  // an exact normalized-title match of the same kind if present, else the first
  // same-kind item. Returns null when the title is blank or nothing matches.
  async function searchTitle(kind, title){
    const t = (title==null ? "" : String(title)).trim();
    if(!t) return null;
    const prov = PROVIDERS[kind];
    const source = prov ? prov.api : kind;
    let data;
    try { data = await apiCached(`/search?${qs({q:t, source, page:1})}`); }
    catch(_){ return null; }
    const items = (data && data.items) || [];
    if(!items.length) return null;
    const norm = (s)=> (s==null ? "" : String(s)).trim().toLowerCase();
    const want = norm(t);
    const sameKind = items.filter(it=> it && it.kind === kind);
    const pool = sameKind.length ? sameKind : items;
    const exact = pool.find(it=> it && norm(it.title) === want);
    const hit = exact || pool[0];
    return (hit && hit.id) ? hit : null;
  }

  // renderUnavailable(kind, meta): no re-resolution match — keep the stored
  // RichMetadata (title/thumbnail) visible but flag the item as unavailable
  // rather than showing a bare error (Req 7.5). Reuses the existing
  // .card.unavailable / .badge.unavail / .lib-unavail-note styles.
  function renderUnavailable(kind, meta){
    const m = meta || {};
    const prov = Object.values(PROVIDERS).find(p=>p.kind===kind) || {};
    setD(
      crumbs([{href:"#/",label:"Home"},{href:"#/library",label:"Library"},{label:m.title||"Unavailable"}])+
      `<div class="lib-group"><div class="grid">`+
        `<div class="card unavailable">`+
          `<div class="poster">${imgTag(m.thumbnail,"",m.title)}<span class="badge unavail">Unavailable</span></div>`+
          `<div class="meta"><div class="t">${h(m.title||"Untitled")}</div>`+
            `<div class="lib-unavail-note">No longer available</div></div>`+
        `</div>`+
      `</div></div>`+
      `<div class="actions" style="margin-top:14px"><a class="btn" href="#/library">${I.arrow} Back to Library</a></div>`
    );
  }

  // paintMetaHero(kind, meta): instant first paint from stored metadata so a
  // saved entry never shows a blank flash while resolution runs (Req 7.6). A
  // spinner sits in the actions row until the resolved detail replaces it.
  function paintMetaHero(kind, meta){
    const m = meta || {};
    const prov = Object.values(PROVIDERS).find(p=>p.kind===kind) || {};
    const facts =
      `<span class="pill">${h(prov.label||kind)}</span>`+
      (m.cosplayer?`<span class="pill">${h(m.cosplayer)}</span>`:"")+
      (m.character?`<span class="pill">${h(m.character)}</span>`:"");
    setD(heroHtml(kind, prov.label||kind, { title: m.title }, facts, spinner, null, m.thumbnail));
  }

  // renderResolved(kind, id, data): hand a successfully-resolved detail to the
  // matching renderer. cosplay/doujin renderers fetch by id (cache-hit on the
  // already-warmed path); the rest take the resolved data directly.
  function renderResolved(kind, id, data){
    if(kind==="cosplay") return renderCosplay(id);
    if(kind==="doujin")  return renderDoujin(id);
    if(kind==="anime")   return renderAnimeSeries(id, data);
    if(kind==="lmanime") return renderAnimeSeries(id, data, "lmanime");
    if(kind==="movie")   return renderMovie(id, data);
    if(kind==="nekopoi") return renderNekopoi(id, data);
    if(kind==="drama")   return renderDrama(id, data);
    if(kind==="donghua") return renderDonghuaSeries(id, data);
    return renderReadableSeries(kind, id, data, 1);
  }

  // resolveSaved(kind, id, meta): the self-heal wrapper. Happy path returns the
  // resolved { id, data } (Req 7.2). On a stale-id error ONLY (Req 7.3) it
  // re-resolves via the stored title; a hit rewrites the saved id in both
  // favorites (sync) and history (async), updates the route hash so a reload
  // uses the fresh id, and refetches (Req 7.4). No match renders the unavailable
  // state and returns null (Req 7.5). Any non-stale error propagates unchanged.
  async function resolveSaved(kind, id, meta){
    try {
      const data = await fetchDetail(kind, id);
      return { id, data };
    } catch(e){
      if(!isStaleId(e)) throw e;                       // only heal stale ids (Req 7.3)
      // 1) Precise heal: re-sign the exact same content URL into a fresh id.
      const rid = await resolveById(id);
      if(rid){
        try {
          const data = await fetchDetail(kind, rid);
          pstore.updateFavoriteId(id, rid);
          await pstore.updateHistoryId(id, rid);
          try { history.replaceState(null, "", pstore.entryRoute(kind, rid)); } catch(_){}
          return { id: rid, data };
        } catch(_){ /* fall through to title search */ }
      }
      // 2) Fuzzy heal: re-resolve by title across the provider.
      const hit = await searchTitle(kind, meta && meta.title);
      if(hit && hit.id){
        pstore.updateFavoriteId(id, hit.id);           // sync rewrite (Req 7.4)
        await pstore.updateHistoryId(id, hit.id);       // async rewrite (Req 7.4)
        try { history.replaceState(null, "", pstore.entryRoute(kind, hit.id)); } catch(_){}
        const data = await fetchDetail(kind, hit.id);
        return { id: hit.id, data };
      }
      renderUnavailable(kind, meta);                    // keep metadata (Req 7.5)
      return null;
    }
  }

  // savedMetaFor(id): look up stored RichMetadata for an opaque id across
  // favorites (sync) then browsing history (async). Matches on the stable
  // content key, so a saved item is found even when the route's id is a
  // rotated/stale issuance of the same content. Returns null for a non-saved
  // id, so ordinary navigation never enters the self-heal path.
  async function savedMetaFor(id){
    if(id == null) return null;
    const k = pstore.stableKey(id);
    const fav = pstore.listFavorites().find(e=> e && (e.key || pstore.stableKey(e.opaqueId)) === k);
    if(fav) return fav;
    try {
      const hist = await pstore.listHistory();
      return hist.find(e=> e && (e.key || pstore.stableKey(e.opaqueId)) === k) || null;
    } catch(_){ return null; }
  }

  // resolveById(id): re-mint a fresh, valid opaque id for the SAME content by
  // re-signing its (secret-independent) source+url payload server-side. Used to
  // heal a saved id that no longer verifies (e.g. the signing secret changed).
  // Returns the new id, or null if the id isn't a re-signable provider token.
  async function resolveById(id){
    const s = String(id || "");
    const dot = s.indexOf(".");
    if(dot < 3) return null;
    const source = s.slice(0, 2);
    const kind = s.charAt(2);
    let payload = s.slice(dot + 1);
    const dot2 = payload.indexOf(".");
    if(dot2 !== -1) payload = payload.slice(0, dot2);
    if(!payload) return null;
    try {
      const r = await api(`/resolve?${qs({ source, kind, u: payload })}`);
      return (r && r.id) || null;
    } catch(_){ return null; }
  }

  async function routeDetail(kind, id){
    const ep = DETAIL_EP[kind];
    if(!ep){ shell(`<div id="d"></div>`); return setD(`<div class="errbox">Unknown type: ${h(kind)}</div>`); }

    // Cache-first: if a hover/touch prefetch already warmed this detail, render
    // it synchronously with no spinner so the page appears instantly.
    if(kind==="anime" || kind==="lmanime"){
      const cfg = ANIME_LIKE[kind];
      const cached = peek(`/${cfg.api}/${encodeURIComponent(id)}`);
      if(cached){ shell(`<div id="d"></div>`); return renderAnimeSeries(id, cached, kind); }
    } else if(kind==="donghua" || kind==="manga" || kind==="novel"){
      const size = kind==="donghua" ? EPISODE_SIZE : CHAPTER_SIZE;
      const cached = peek(`/${ep}/${encodeURIComponent(id)}?${qs({page:1,size})}`);
      if(cached){
        shell(`<div id="d"></div>`);
        return kind==="donghua" ? renderDonghuaSeries(id, cached) : renderReadableSeries(kind, id, cached, 1);
      }
    } else if(kind==="cosplay" || kind==="doujin" || kind==="movie" || kind==="nekopoi"){
      const path = kind==="cosplay" ? `/cosplay/${encodeURIComponent(id)}`
        : kind==="movie" ? `/movie/${encodeURIComponent(id)}`
        : kind==="nekopoi" ? `/nekopoi/${encodeURIComponent(id)}`
        : `/nhentai/${encodeURIComponent(id)}`;
      const cached = peek(path);
      if(cached){ shell(`<div id="d"></div>`); return kind==="cosplay" ? renderCosplay(id) : kind==="movie" ? renderMovie(id, cached) : kind==="nekopoi" ? renderNekopoi(id, cached) : renderDoujin(id); }
    }
    // drama (and any other kind): no cache-peek; handled by the main fetch below.

    shell(`<div id="d">${spinner}</div>`);

    // Saved-entry path (Req 7.2–7.6): if this id is a stored favorite/history
    // entry, paint instantly from its RichMetadata (no blank flash) and resolve
    // through the self-heal wrapper so a stale saved id re-resolves by title
    // transparently. Non-saved ids skip this entirely, so ordinary navigation
    // is unchanged.
    const savedMeta = await savedMetaFor(id);
    if(savedMeta){
      paintMetaHero(kind, savedMeta);                 // first paint from metadata (Req 7.6)
      try{
        const res = await resolveSaved(kind, id, savedMeta);
        if(res) return renderResolved(kind, res.id, res.data);
        return;                                        // unavailable already rendered (Req 7.5)
      }catch(e){ return setD(`<div class="errbox">${h(e.message)}</div>`); }
    }

    try{
      if(kind==="cosplay") return renderCosplay(id);
      if(kind==="doujin") return renderDoujin(id);
      if(kind==="movie"){ const data = await apiCached(`/movie/${encodeURIComponent(id)}`); return renderMovie(id, data); }
      if(kind==="nekopoi"){ const data = await apiCached(`/nekopoi/${encodeURIComponent(id)}`); return renderNekopoi(id, data); }
      if(kind==="drama"){ const data = await apiCached(`/drama/${encodeURIComponent(id)}`); return renderDrama(id, data); }
      if(kind==="anime" || kind==="lmanime"){ const cfg = ANIME_LIKE[kind]; const data = await apiCached(`/${cfg.api}/${encodeURIComponent(id)}`); return renderAnimeSeries(id, data, kind); }
      const size = kind==="donghua" ? EPISODE_SIZE : CHAPTER_SIZE;
      const data = await apiCached(`/${ep}/${encodeURIComponent(id)}?${qs({page:1,size})}`);
      if(kind==="donghua") return renderDonghuaSeries(id, data);
      return renderReadableSeries(kind, id, data, 1);
    }catch(e){ setD(`<div class="errbox">${h(e.message)}</div>`); }
  }

  function renderAnimeSeries(id, data, kind){
    kind = kind || "anime";
    const cfg = ANIME_LIKE[kind] || ANIME_LIKE.anime;
    const eps = data.episodes||[];
    const facts = [
      data.status?`<span class="pill ok">${h(data.status)}</span>`:"",
      data.score?`<span class="pill">&#9733; ${h(data.score)}</span>`:"",
      data.anime_type?`<span class="pill">${h(data.anime_type)}</span>`:"",
      data.total_episodes?`<span class="pill">${h(data.total_episodes)} eps</span>`:"",
      data.duration?`<span class="pill">${h(data.duration)}</span>`:"",
      data.studio?`<span class="pill">${h(data.studio)}</span>`:"",
      data.release_date?`<span class="pill">${h(data.release_date)}</span>`:"",
      ...(data.genres||[]).slice(0,6).map(g=>genrePill(kind, g)),
    ].join("");
    const first = eps[0];
    const last = eps[eps.length-1];
    const actions = [
      first?`<a class="btn primary" href="#/${cfg.watch}/${encodeURIComponent(first.id)}">${I.play} Watch Ep ${first.number??1}</a>`:"",
      (last && last!==first)?`<a class="btn" href="#/${cfg.watch}/${encodeURIComponent(last.id)}">Latest ep ${last.number??""}</a>`:"",
      ...(data.batch||[]).slice(0,1).map(b=>`<a class="btn sm" href="#/${cfg.watch}/${encodeURIComponent(b.id)}">${I.book} Batch</a>`),
    ].join("");
    const syn = data.synopsis || (data.japanese_title?`Japanese title: ${data.japanese_title}`:"");
    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.cover, kind, timestamp: Date.now() };
    // Record this open into browsing history (fire-and-forget; never blocks or
    // throws into the render path). Reuses the same RichMetadata shape as the
    // favorite control (Req 3.4, 3.7, 3.8).
    void pstore.recordHistory(favMeta);
    const epControls = eps.length>24
      ? `<div class="ep-tools"><input id="epSearch" type="search" inputmode="numeric" placeholder="Jump to episode..." autocomplete="off"></div>`
      : "";
    const epGrid = eps.length
      ? `<div class="ep-list" id="epList">${eps.map(e=>`<button class="ep-btn center" data-ep="${e.number??""}" data-go="#/${cfg.watch}/${encodeURIComponent(e.id)}">Ep ${e.number??(e.title||"?")}</button>`).join("")}</div>`
      : `<div class="empty">No episodes available.</div>`;

    setD(
      heroHtml(kind,cfg.label,data,facts,actions+favButton(favMeta),syn,data.cover)+
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
    if(first) prefetch(`/${cfg.api}/episode/${encodeURIComponent(first.id)}`);
    renderRecommendations(kind, id);
  }

  // Movie/film detail (single embed player) — LayarKaca21.
  function renderMovie(id, data){
    const slug = (l)=>String(l||"").trim().toLowerCase().replace(/&/g,"and").replace(/[^a-z0-9]+/g,"-").replace(/^-+|-+$/g,"");
    const genrePillM = (g)=>`<a class="pill link" href="#/browse/movie/${encodeURIComponent(slug(g))}">${h(g)}</a>`;
    const countryPill = (c)=>`<a class="pill link" href="#/browse/movie/${encodeURIComponent("country:"+slug(c))}">${h(c)}</a>`;
    const facts = [
      data.year?`<span class="pill">${h(data.year)}</span>`:"",
      data.rating?`<span class="pill">&#9733; ${h(data.rating)}</span>`:"",
      data.quality?`<span class="pill ok">${h(data.quality)}</span>`:"",
      data.duration?`<span class="pill">${h(data.duration)}</span>`:"",
      ...(data.countries||[]).slice(0,2).map(countryPill),
      ...(data.genres||[]).slice(0,6).map(genrePillM),
    ].join("");
    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.poster, kind: "movie", timestamp: Date.now() };
    void pstore.recordHistory(favMeta);

    const dlBtn = data.download_url
      ? `<a class="btn" target="_blank" rel="noopener noreferrer" href="${h(data.download_url)}">${I.book} Download</a>`
      : "";
    const actions = dlBtn + favButton(favMeta);

    const credits = [
      (data.directors||[]).length ? `<p class="movie-credit"><span>Director:</span> ${data.directors.map(h).join(", ")}</p>` : "",
      (data.cast||[]).length ? `<p class="movie-credit"><span>Cast:</span> ${data.cast.slice(0,8).map(h).join(", ")}</p>` : "",
      data.release_date ? `<p class="movie-credit"><span>Release:</span> ${h(data.release_date)}</p>` : "",
    ].join("");

    // lk21 exposes several player servers ("GANTI PLAYER": P2P / TURBOVIP /
    // CAST / HYDRAX). P2P resolves (server-side) to a proxied HLS master we play
    // inline with hls.js; the others are their own embeddable players we unwrap
    // to an inner iframe. The client only sends a server name to
    // /api/v1/movie-stream/{id}?server=… and renders whatever it gets back.
    const sid = encodeURIComponent(data.id||id);
    const servers = Array.isArray(data.servers) ? data.servers : [];
    const hasPlayer = servers.length || data.embed_url;
    const serverBtns = servers.length
      ? `<div class="server-switch"><span class="ss-label">${I.play} Server</span>`+
        servers.map((s,i)=>`<button class="ss-btn${i===0?" active":""}" data-server="${escAttr(s.name)}">${h(s.label)}</button>`).join("")+
        `</div>`
      : "";
    const playerBlock = hasPlayer
      ? serverBtns + `<div id="movie-player" class="movie-player">${spinner}</div>`
      : `<div class="server-note">No playable source found for this title.${data.download_url?" Use <b>Download</b> for an offline copy.":""}</div>`;

    // MOVIE TERKAIT — related suggestions rendered as standard movie cards.
    const related = (data.related||[]).map(r=>({
      id:r.id, title:r.title, thumbnail:r.poster, kind:"movie", source:"lk21",
      tags: r.year?[String(r.year)]:[],
    }));
    const relatedBlock = related.length
      ? `<div class="row-head"><h2><span class="dot"></span>Related Movies</h2></div>`+grid(related)
      : "";

    setD(
      heroHtml("movie","Movies",{title:data.title},facts,actions,data.synopsis,data.poster)+
      (credits?`<div class="movie-credits">${credits}</div>`:"")+
      `<div class="row-head"><h2><span class="dot"></span>Watch</h2></div>`+
      playerBlock+
      relatedBlock
    );

    if(hasPlayer){
      const dlNote = data.download_url
        ? ` <a class="btn sm" target="_blank" rel="noopener noreferrer" href="${escAttr(data.download_url)}">${I.book} Download</a>`
        : "";
      const loadServer = async (name, btn)=>{
        const host = document.getElementById("movie-player");
        if(!host) return;
        document.querySelectorAll(".ss-btn").forEach(b=>b.classList.toggle("active", b===btn));
        host.innerHTML = spinner;
        try{
          const q = name ? `?server=${encodeURIComponent(name)}` : "";
          const res = await api(`/movie-stream/${sid}${q}`);
          if(res && res.type==="hls" && res.url){
            host.innerHTML = `<div class="video-wrap hls">
                <video id="hls-m" controls preload="metadata" playsinline webkit-playsinline></video>
                <div class="hls-state" id="hls-state-m">${spinner}</div>
              </div>`;
            // We already resolved the master URL; attach it directly.
            attachHlsSrc(host.querySelector("video"), "m", res.url);
          } else if(res && res.type==="iframe" && res.url){
            host.innerHTML = `<div class="embed-frame">
                ${playerIframe(res.url)}
              </div>
              <div class="embed-help">Some servers only allow playback on their own site. If the video shows an "embedding blocked" message, <a href="${escAttr(res.url)}" target="_blank" rel="noopener noreferrer">open it in a new tab ${I.arrow}</a>. The <b>P2P</b> server always plays directly here.${dlNote}</div>`;
          } else {
            host.innerHTML = `<div class="hls-err">No playable source on this server.${dlNote}</div>`;
          }
        }catch(e){
          host.innerHTML = `<div class="hls-err">Failed to load this server. Try another.${dlNote}</div>`;
        }
      };
      document.querySelectorAll(".ss-btn").forEach(btn=>{
        btn.addEventListener("click", ()=>loadServer(btn.dataset.server, btn));
      });
      const first = document.querySelector(".ss-btn");
      loadServer(first ? first.dataset.server : "", first);
    }
    renderRecommendations("movie", id);
  }

  // NekoPoi adult-anime post: streaming server switcher (directly embeddable
  // players) + download groups + related posts. 18+ content.
  function renderNekopoi(id, data){
    const sid = encodeURIComponent(data.id||id);
    const facts = [
      data.date?`<span class="pill">${h(data.date)}</span>`:"",
      ...(data.genres||[]).slice(0,8).map(g=>`<a class="pill link" href="#/search/${encodeURIComponent(g)}/nekopoi">${h(g)}</a>`),
    ].join("");
    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.cover, kind: "nekopoi", timestamp: Date.now() };
    void pstore.recordHistory(favMeta);
    const actions = favButton(favMeta);

    const servers = Array.isArray(data.servers) ? data.servers : [];
    const playerBlock = servers.length
      ? `<div class="server-switch"><span class="ss-label">${I.play} Server</span>${servers.map((s,i)=>`<button class="ss-btn${i===0?" active":""}" data-resolve="${escAttr(s.resolve||"")}" data-embed="${escAttr(s.embed_url)}">${h(s.label||("Server "+(i+1)))}</button>`).join("")}</div>`+
        `<div id="neko-player" class="movie-player">${spinner}</div>`+
        `<div class="embed-help">If a server fails to play, switch to another above or <a id="neko-open" href="${escAttr(servers[0].embed_url)}" target="_blank" rel="noopener noreferrer">open the source in a new tab ${I.arrow}</a>.</div>`
      : `<div class="server-note">No streaming server found.</div>`;

    // Download groups -> quality + mirror buttons.
    const dlBlock = (data.downloads||[]).length
      ? `<div class="row-head"><h2><span class="dot"></span>Download</h2></div>`+
        (data.downloads||[]).map(g=>`<div class="dl-group"><div class="q">${h(g.quality)}</div><div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener noreferrer" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div></div>`).join("")
      : "";

    const related = (data.related||[]).map(r=>({
      id:r.id, title:r.title, thumbnail:r.poster, kind:"nekopoi", source:"nekopoi", tags:[],
    }));
    const relatedBlock = related.length
      ? `<div class="row-head"><h2><span class="dot"></span>Related</h2></div>`+grid(related)
      : "";

    // Series page: an episode list (each opens its own video post).
    const episodes = (data.episodes||[]).map(e=>({
      id:e.id, title:e.title, thumbnail:e.poster, kind:"nekopoi", source:"nekopoi", tags:[],
    }));
    const episodesBlock = episodes.length
      ? `<div class="row-head"><h2><span class="dot"></span>Episodes <span class="cnt-badge">${episodes.length}</span></h2></div>`+grid(episodes)
      : "";

    // Watch section only when this post actually carries players. Series pages
    // (episode lists) skip it and lead with the episode grid instead.
    const watchBlock = servers.length
      ? `<div class="row-head"><h2><span class="dot"></span>Watch</h2></div>`+playerBlock+dlBlock
      : (data.downloads||[]).length ? dlBlock : "";

    setD(
      heroHtml("nekopoi","Hentai",{title:data.title},facts,actions,data.synopsis,data.cover)+
      watchBlock+episodesBlock+relatedBlock
    );

    // Server resolver with a built-in ad-blocking policy: we only ever play
    // streams INLINE that we could crack server-side into a clean mp4/hls
    // (StreamWish/streampoi, and DoodStream when its WAF lets us through).
    // Ad-funded hosts that can only be iframed (playmogo/dood when blocked,
    // unknown players) are NEVER embedded — their iframes inject pop-ups and
    // self-navigate to unframeable ad pages (noxiousback.com etc.), breaking
    // the player. Instead we surface a clean "open in a new tab" action and,
    // on auto-load, skip straight to a server we can play inline & ad-free.
    if(!servers.length) return;
    const btns = Array.from(document.querySelectorAll(".server-switch .ss-btn"));
    const setActive = (btn)=> btns.forEach(b=>b.classList.toggle("active", b===btn));
    async function loadNekoServer(idx, autoAdvance){
      const btn = btns[idx];
      if(!btn) return;
      setActive(btn);
      const host = document.getElementById("neko-player");
      const open = document.getElementById("neko-open");
      if(open && btn.dataset.embed) open.href = btn.dataset.embed;
      if(!host) return;
      host.innerHTML = spinner;
      // This server can't be played inline (ad host / anti-embed). During an
      // auto-load, skip to the next server hunting for an inline one; only if
      // none is inline-playable do we offer the external-open card here.
      const notInline = ()=>{
        if(autoAdvance && idx + 1 < btns.length){ loadNekoServer(idx+1, true); return; }
        host.innerHTML = `<div class="server-note">Server <b>${escHtml((btn.textContent||"ini").trim())}</b> pakai proteksi anti-embed + iklan, jadi tak bisa diputar langsung di sini tanpa iklan.</div>`+
          `<a class="btn primary block" href="${escAttr(btn.dataset.embed)}" target="_blank" rel="noopener noreferrer nofollow">Buka player di tab baru ${I.arrow}</a>`+
          `<div class="embed-help">Pilih server lain di atas untuk menonton langsung di sini, bebas iklan.</div>`;
      };
      try{
        if(!btn.dataset.resolve){ notInline(); return; }
        const rel = btn.dataset.resolve.replace(/^.*\/api\/v1/, "");
        const res = await api(rel);
        if(res && res.type==="mp4" && res.url){
          host.innerHTML = `<div class="video-wrap"><video id="neko-video" controls preload="metadata" playsinline webkit-playsinline src="${escAttr(res.url)}"></video></div>`;
          const v=document.getElementById("neko-video");
          if(v){
            // If the proxied CDN file fails to play (e.g. its edge node is
            // unreachable / token expired), fall back gracefully instead of
            // leaving a broken player.
            v.onerror=()=>notInline();
            v.play().catch(()=>{});
          }
        } else if(res && res.type==="hls" && res.url){
          host.innerHTML = `<div class="video-wrap hls"><video id="neko-video" controls preload="metadata" playsinline webkit-playsinline></video><div class="hls-state" id="hls-state-neko">${spinner}</div></div>`;
          attachHlsSrc(document.getElementById("neko-video"), "neko", res.url);
        } else {
          // type === "iframe" (or anything non-inline): do NOT embed the ad host.
          notInline(); return;
        }
        void pstore.recordHistory(Object.assign({}, favMeta, { timestamp: Date.now() }));
      }catch(e){
        notInline();
      }
    }
    btns.forEach((btn,i)=> btn.addEventListener("click", ()=> loadNekoServer(i, false)));
    // Auto-load the first server, auto-advancing to the first inline-playable one.
    loadNekoServer(0, true);
  }

  // DramaBox / DramaWave (drachin) drama: a vertical short-drama with many
  // episodes, each an HLS (.m3u8) stream proxied through /hls. We play the
  // selected episode inline with hls.js (same engine as movies/cosplay) and
  // offer an episode grid. Mobile + desktop friendly (responsive, playsinline).
  function renderDrama(id, data){
    const eps = (data.episodes||[]).filter(e=>e && e.video_url);
    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.cover, kind: "drama", timestamp: Date.now() };
    void pstore.recordHistory(favMeta);
    const facts = [
      eps.length?`<span class="pill">${eps.length} eps</span>`:"",
    ].join("");
    const actions = favButton(favMeta);

    const playerBlock = eps.length
      ? `<div class="movie-player"><div class="video-wrap hls"><video id="drama-video" controls preload="metadata" playsinline webkit-playsinline></video><div class="hls-state" id="hls-state-drama">${spinner}</div></div></div>`+
        `<div class="ep-list" id="dramaEps">${eps.map((e,i)=>`<button class="ep-btn center${i===0?" active":""}" data-ep="${i}" data-src="${escAttr(e.video_url)}" data-num="${e.index}">Ep ${e.index}</button>`).join("")}</div>`
      : `<div class="server-note">No episodes available for this drama.</div>`;

    setD(
      heroHtml("drama","Drama",{title:data.title},facts,actions,data.description,data.cover)+
      (eps.length?`<div class="row-head"><h2><span class="dot"></span>Watch</h2></div>`:"")+
      playerBlock
    );

    if(!eps.length) return;

    // Episode switching: re-attach the HLS source + record progress so the
    // library shows the last episode watched.
    const video = document.getElementById("drama-video");
    const play = (src, num)=>{
      const st = document.getElementById("hls-state-drama");
      if(st) st.innerHTML = spinner;
      attachHlsSrc(video, "drama", src);
      void pstore.recordHistory(Object.assign({}, favMeta, { progress:{ type:"episode", number: Number(num), label:"Ep "+num }, timestamp: Date.now() }));
    };
    document.querySelectorAll("#dramaEps .ep-btn").forEach(btn=>{
      btn.addEventListener("click", ()=>{
        document.querySelectorAll("#dramaEps .ep-btn").forEach(b=>b.classList.toggle("active", b===btn));
        play(btn.dataset.src, btn.dataset.num);
        if(video) video.play().catch(()=>{});
      });
    });
    // Auto-load the first episode.
    play(eps[0].video_url, eps[0].index);
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
      ...(data.genres||[]).slice(0,5).map(g=>genrePill("donghua", g)),
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

    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.cover, kind: "donghua", timestamp: Date.now() };
    // Browsing-history record (fire-and-forget; Req 3.3, 3.7, 3.8).
    void pstore.recordHistory(favMeta);

    setD(
      heroHtml("donghua","Donghua",data,facts,actions+favButton(favMeta),data.synopsis,data.cover)+
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
      ...(data.genres||[]).slice(0,5).map(g=>genrePill(kind, g)),
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
    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.cover, kind, timestamp: Date.now() };
    // Browsing-history record for comics/novel detail (fire-and-forget). Upsert
    // by opaqueId means re-renders on pager nav just refresh the timestamp
    // rather than duplicating (Req 3.1, 3.2, 3.7, 3.8).
    void pstore.recordHistory(favMeta);

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
      heroHtml(kind,label,data,facts,actions+favButton(favMeta),syn,data.cover)+
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
    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.cover, kind: "cosplay", cosplayer: data.cosplayer, character: data.character, timestamp: Date.now() };
    // Browsing-history record; cosplay carries cosplayer/character (Req 3.5, 3.7).
    void pstore.recordHistory(favMeta);

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

    // "Suggestions for you" — related posts scraped from the bottom of the
    // gallery page (already opaque-encoded + image-proxied by the API). These
    // are post-specific, so prefer them over the generic popular-feed row.
    const recs = (data.recommendations||[]).filter(it=>providerVisible(it.kind));
    const suggBlock = recs.length
      ? `<div class="row-head"><h2><span class="dot"></span>Suggestions for you</h2></div>${grid(recs)}`
      : "";

    setD(
      heroHtml("cosplay","Cosplay",data,facts,actions+favButton(favMeta),null,data.cover)+
      videoBlock+
      `<div class="row-head"><h2><span class="dot"></span>${(data.images||[]).length} Photos</h2></div>`+
      `<div class="gallery">${imgs||`<div class="empty">No photos.</div>`}</div>`+
      suggBlock
    );
    // Resolve + attach HLS players
    document.querySelectorAll(".video-wrap.hls").forEach(el => attachHls(el));
    // Fall back to the generic popular-feed recommendations only when the post
    // page itself didn't surface any suggestions.
    if(!recs.length) renderRecommendations("cosplay", id);
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
    const favMeta = { opaqueId: data.id||id, title: data.title, thumbnail: data.cover, kind: "doujin", timestamp: Date.now() };
    // Browsing-history record for doujin detail (fire-and-forget; Req 3.6, 3.7).
    void pstore.recordHistory(favMeta);
    setD(
      heroHtml("doujin","Doujin",data,facts,actions+favButton(favMeta),null,data.cover)+
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
      // Browsing-history record. Point at the SERIES detail (series_id) so the
      // Library entry opens the series page, falling back to the episode id
      // when the series is unknown. Renders from metadata alone (Req 3.3, 3.8).
      void pstore.recordHistory({ opaqueId: e.series_id||id, title: e.series_title, kind: "donghua", thumbnail: e.thumbnail||e.cover||e.poster||null, progress: { type: "episode", number: e.episode_number, id, label: (e.episode_number!=null?("Ep "+e.episode_number):(e.title||null)) }, timestamp: Date.now() });
      const servers = e.servers||[];
      const seriesLink = e.series_id?`#/detail/donghua/${encodeURIComponent(e.series_id)}`:"#/";
      const player = servers.length?`<div class="player-wrap"><div class="frame">${playerIframe(servers[0].embed_url, "player")}</div></div>`:`<div class="empty">No video servers available.</div>`;
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
    }catch(err){
      // Stale leaf id (e.g. after a signing-secret change): re-mint it and retry
      // so resuming from History still lands on the episode.
      if(isStaleId(err)){
        const fresh = await resolveById(id);
        if(fresh && fresh!==id){ try{ history.replaceState(null,"",`#/watch/${encodeURIComponent(fresh)}`);}catch(_){} return routeWatch(fresh); }
      }
      setView(`<div class="errbox">${h(err.message)}</div>`);
    }
  }

  async function routeWatchAnime(id, kind){
    kind = kind || "anime";
    const cfg = ANIME_LIKE[kind] || ANIME_LIKE.anime;
    const warmed = peek(`/${cfg.api}/episode/${encodeURIComponent(id)}`);
    shell(`<div id="d">${warmed?"":spinner}</div>`);
    try{
      const e = await apiCached(`/${cfg.api}/episode/${encodeURIComponent(id)}`);
      // Browsing-history record pointing at the series detail (Req 3.4, 3.8).
      void pstore.recordHistory({ opaqueId: e.series_id||id, title: e.series_title, kind, thumbnail: e.thumbnail||e.cover||e.poster||null, progress: { type: "episode", number: e.episode_number, id, label: (e.episode_number!=null?("Ep "+e.episode_number):(e.title||null)) }, timestamp: Date.now() });
      const mirrors = e.mirrors||[];
      const seriesLink = e.series_id?`#/detail/${kind}/${encodeURIComponent(e.series_id)}`:"#/";
      const epLabel = e.episode_number!=null ? `Episode ${e.episode_number}` : "Episode";
      // Initial player = default embed if present, else nothing (resolved on click).
      const initial = e.default_embed || "";
      const player = `<div class="player-wrap"><div class="frame">${initial?playerIframe(initial, "player"):`<div class="empty" id="playerEmpty">Select a server below.</div>`}</div></div>`;
      // Group mirrors by quality.
      const byQ = {};
      mirrors.forEach(m=>{ (byQ[m.quality]=byQ[m.quality]||[]).push(m); });
      const serverBars = Object.entries(byQ).map(([q, list])=>
        `<div class="server-bar"><span class="lbl">${h(q||"Servers")}:</span>${list.map(m=>`<button class="srv" data-stream="${h(m.stream_id)}">${h(m.name)}</button>`).join("")}</div>`
      ).join("");
      const dls = (e.downloads||[]).map(g=>`<div class="dl-group"><div class="q">${h(g.quality)}${g.size?` &middot; ${h(g.size)}`:""}</div><div class="mirrors">${(g.mirrors||[]).map(m=>`<a class="btn sm" target="_blank" rel="noopener noreferrer" href="${h(m.url)}">${h(m.name)}</a>`).join("")}</div></div>`).join("");
      const nav = `<div class="server-bar" style="margin-top:8px">
        ${e.prev_id?`<a class="btn sm" href="#/${cfg.watch}/${encodeURIComponent(e.prev_id)}">&larr; Previous ep</a>`:""}
        <a class="btn sm" href="${seriesLink}">&#9776; All episodes</a>
        ${e.next_id?`<a class="btn sm" href="#/${cfg.watch}/${encodeURIComponent(e.next_id)}">Next ep &rarr;</a>`:""}</div>`;
      setView(
        `<div id="d">`+
        crumbs([{href:"#/",label:"Home"},{href:`#/browse/${cfg.browse}`,label:cfg.label},{label:`${e.series_title||cfg.label} - ${epLabel}`}])+
        `<div class="row-head"><h2><span class="dot"></span>${h(e.series_title||cfg.label)} - ${epLabel}</h2></div>`+
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
            const r = await api(`/${cfg.stream}?${qs({id: btn.dataset.stream})}`);
            frame.innerHTML = playerIframe(r.url, "player");
          }catch(err){
            frame.innerHTML = `<div class="empty">Failed to load server. Try another one.</div>`;
          }
        };
      });
      if(e.next_id) prefetch(`/${cfg.api}/episode/${encodeURIComponent(e.next_id)}`);
      if(e.prev_id) prefetch(`/${cfg.api}/episode/${encodeURIComponent(e.prev_id)}`);
      renderRecommendations(kind, e.series_id);
    }catch(err){
      if(isStaleId(err)){
        const fresh = await resolveById(id);
        if(fresh && fresh!==id){ try{ history.replaceState(null,"",`#/${cfg.watch}/${encodeURIComponent(fresh)}`);}catch(_){} return routeWatchAnime(fresh, kind); }
      }
      setView(`<div class="errbox">${h(err.message)}</div>`);
    }
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
      // Browsing-history record. Point at the series detail (series_id) so the
      // Library entry opens the series page, falling back to the chapter id.
      // Map the reader kind onto the detail kind (nhentai -> doujin) so the
      // recorded entry's kind matches its detail route (Req 3.1, 3.6, 3.8).
      void pstore.recordHistory({ opaqueId: c.series_id||id, title: c.series_title, kind: kind==="nhentai"?"doujin":"manga", thumbnail: c.thumbnail||c.cover||(c.pages&&c.pages[0]&&c.pages[0].url)||null, progress: { type: "chapter", number: c.chapter_number, id, label: (c.chapter_number!=null?("Ch. "+c.chapter_number):(c.chapter_title||null)) }, timestamp: Date.now() });
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
    }catch(err){
      if(isStaleId(err)){
        const fresh = await resolveById(id);
        if(fresh && fresh!==id){ try{ history.replaceState(null,"",`#/read/${kind}/${encodeURIComponent(fresh)}`);}catch(_){} return routeRead(kind, fresh); }
      }
      setView(`<div class="errbox">${h(err.message)}</div>`);
    }
  }

  async function renderNovelChapter(id){
    try{
    const c = await apiCached(`/novel/chapter/${encodeURIComponent(id)}`);
    // Browsing-history record pointing at the novel series detail (Req 3.2, 3.8).
    void pstore.recordHistory({ opaqueId: c.series_id||id, title: c.series_title, kind: "novel", thumbnail: c.thumbnail||c.cover||null, progress: { type: "chapter", number: c.chapter_number, id, label: (c.chapter_number!=null?("Ch. "+c.chapter_number):(c.chapter_title||null)) }, timestamp: Date.now() });
    const paras = (c.body||"").split(/\n{2,}/).map(s=>s.trim()).filter(Boolean).map(p=>`<p>${h(p)}</p>`).join("");
    const nav = `<div class="reader-nav">
      ${c.prev_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.prev_id)}">&larr; Previous</a>`:""}
      ${c.series_id?`<a class="btn sm" href="#/detail/novel/${encodeURIComponent(c.series_id)}">&#9776; Chapter list</a>`:""}
      ${c.next_id?`<a class="btn sm" href="#/read/novel/${encodeURIComponent(c.next_id)}">Next &rarr;</a>`:""}</div>`;
    setView(`<div class="row-head"><h2><span class="dot"></span>${h(c.series_title||"Novel")} &middot; Ch ${c.chapter_number}</h2></div>`+
      (c.chapter_title?`<p style="color:var(--muted);margin-top:-8px">${h(c.chapter_title)}</p>`:"")+
      `<div class="novel-body">${paras||"<p>(empty)</p>"}</div>`+nav);
    if(c.next_id) prefetch(`/novel/chapter/${encodeURIComponent(c.next_id)}`);
    }catch(err){
      if(isStaleId(err)){
        const fresh = await resolveById(id);
        if(fresh && fresh!==id){ try{ history.replaceState(null,"",`#/read/novel/${encodeURIComponent(fresh)}`);}catch(_){} return renderNovelChapter(fresh); }
      }
      setView(`<div class="errbox">${h(err.message)}</div>`);
    }
  }

  // route dispatch is defined in part 2 (appended)
  window.__apiku = { shell, setView, viewEl, h, qs, api, apiRaw, apiCached, prefetch, spinner, go, I,
    LITE, setLite, pstore,
    isStaleId, searchTitle, resolveSaved, fetchDetail,
    routeHome, routeBrowse, routeSearch, routeXref, routeSchedule, routeDetail, routeWatch, routeWatchAnime, routeRead, routeLibrary };
})();

// ===========================================================================
// Docs + Explorer + Router
// ===========================================================================
(function () {
  "use strict";
  const A = window.__apiku;
  const { shell, setView, h, apiRaw, I, go,
    routeHome, routeBrowse, routeSearch, routeXref, routeSchedule, routeDetail, routeWatch, routeWatchAnime, routeRead, routeLibrary } = A;

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
      case "xref":     return routeXref(parts[1], parts[2]);
      case "schedule": return routeSchedule();
      case "library":  return routeLibrary(parts[1]);
      case "detail":   return routeDetail(parts[1], parts[2]);
      case "watch":    return routeWatch(parts[1]);
      case "watchanime": return routeWatchAnime(parts[1]);
      case "watchlm":  return routeWatchAnime(parts[1], "lmanime");
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
