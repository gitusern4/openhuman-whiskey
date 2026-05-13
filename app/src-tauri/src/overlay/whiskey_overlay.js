/**
 * Whiskey TV Overlay Panel — injected into TradingView Desktop via CDP Runtime.evaluate.
 *
 * Self-contained vanilla JS (no imports, no bundler). Runs in TV's page context.
 *
 * Architecture:
 *   - Panel is docked to right edge of chart at z-index 999999.
 *   - State pushed by Rust via window.__WHISKEY_OVERLAY_STATE (polled every 100ms).
 *   - Commands posted to window.__WHISKEY_OVERLAY_OUTBOX (array, drained by Rust every 200ms).
 *   - MutationObserver on document.body re-creates panel if detached.
 *   - Position persisted to localStorage under key "whiskey-overlay-pos".
 *   - Minimize state persisted to localStorage under key "whiskey-overlay-minimized".
 *
 * All injected DOM elements use whiskey- prefix in ids/classes.
 */
(function whiskeyOverlayMain() {
  'use strict';

  // ── Idempotency guard ───────────────────────────────────────────────────────
  if (document.getElementById('whiskey-tv-overlay')) {
    return { ok: true, panel_id: 'whiskey-tv-overlay', skipped: true };
  }

  // ── Constants ───────────────────────────────────────────────────────────────
  var PANEL_ID = 'whiskey-tv-overlay';
  var LOCKOUT_VEIL_ID = 'whiskey-lockout-veil';
  var STATE_KEY = '__WHISKEY_OVERLAY_STATE';
  var OUTBOX_KEY = '__WHISKEY_OVERLAY_OUTBOX';
  var POS_STORE = 'whiskey-overlay-pos';
  var MIN_STORE = 'whiskey-overlay-minimized';

  // Per-session nonce baked in at inject time. Substituted by the
  // Rust inject path BEFORE the JS runs. Lives in this closure ONLY
  // — never written to window — so TV-page scripts cannot forge it.
  // The Rust drain checks every outbox command for this exact value
  // and drops mismatches. Senior architect review 2026-05-12.
  var NONCE = '__WHISKEY_NONCE__';
  var PANEL_W = 320;
  var PANEL_H = 480;
  var MINI_W = 32;
  var Z = 999999;

  var TAG_CHIPS = [
    'absorbed',
    'delta_div',
    'single_print',
    'value_area_reject',
    'responsive_buyer',
    'responsive_seller',
  ];

  // ── Outbox helper ───────────────────────────────────────────────────────────
  // Every command gets stamped with the per-session NONCE. The Rust
  // drain enforces a strict equality check; commands without a
  // matching nonce are dropped silently. This prevents TV-page
  // scripts (which can also reach `window[OUTBOX_KEY]`) from forging
  // commands to escalate via our poll bridge.
  function postCommand(cmd) {
    if (!Array.isArray(window[OUTBOX_KEY])) {
      window[OUTBOX_KEY] = [];
    }
    // Defensive shallow copy so the nonce can't be mutated by a
    // later caller. Object spread is widely supported in TV's
    // Electron renderer (Chromium 100+).
    var stamped = Object.assign({}, cmd, { __nonce: NONCE });
    window[OUTBOX_KEY].push(stamped);
  }

  // ── localStorage helpers ────────────────────────────────────────────────────
  function lsGet(key) {
    try { return localStorage.getItem(key); } catch (_) { return null; }
  }
  function lsSet(key, val) {
    try { localStorage.setItem(key, val); } catch (_) {}
  }

  // ── Saved position ──────────────────────────────────────────────────────────
  function loadPos() {
    try {
      var raw = lsGet(POS_STORE);
      if (raw) return JSON.parse(raw);
    } catch (_) {}
    return null;
  }
  function savePos(x, y) {
    lsSet(POS_STORE, JSON.stringify({ x: x, y: y }));
  }

  // ── CSS ─────────────────────────────────────────────────────────────────────
  var COLORS = {
    bg: '#1a1a1a',
    border: '#333',
    text: '#e5e5e5',
    muted: '#888',
    accent: '#39ff14',
    red: '#ef4444',
    redDark: '#991b1b',
    inputBg: '#252525',
    chipBg: '#2a2a2a',
    chipActive: '#39ff14',
    chipActiveTxt: '#000',
    handle: '#111',
  };

  function px(n) { return n + 'px'; }

  // ── DOM builder helpers ─────────────────────────────────────────────────────
  function el(tag, attrs, styles) {
    var e = document.createElement(tag);
    if (attrs) {
      Object.keys(attrs).forEach(function(k) {
        if (k === 'textContent') { e.textContent = attrs[k]; }
        else if (k === 'className') { e.className = attrs[k]; }
        else { e.setAttribute(k, attrs[k]); }
      });
    }
    if (styles) applyStyle(e, styles);
    return e;
  }

  function applyStyle(e, styles) {
    Object.keys(styles).forEach(function(k) { e.style[k] = styles[k]; });
  }

  function sep() {
    return el('hr', {}, {
      all: 'initial',
      display: 'block',
      border: 'none',
      borderTop: '1px solid ' + COLORS.border,
      margin: '8px 0',
    });
  }

  function label(txt) {
    return el('div', { textContent: txt }, {
      all: 'initial',
      display: 'block',
      fontSize: '10px',
      color: COLORS.muted,
      marginBottom: '4px',
      fontFamily: 'sans-serif',
      textTransform: 'uppercase',
      letterSpacing: '0.05em',
    });
  }

  function numInput(placeholder, val) {
    var inp = el('input', { type: 'number', step: 'any', placeholder: placeholder }, {
      all: 'initial',
      display: 'block',
      width: '100%',
      boxSizing: 'border-box',
      background: COLORS.inputBg,
      border: '1px solid ' + COLORS.border,
      borderRadius: '4px',
      color: COLORS.text,
      fontSize: '12px',
      padding: '4px 6px',
      fontFamily: 'monospace',
      marginBottom: '6px',
      outline: 'none',
    });
    if (val !== undefined && val !== null) inp.value = val;
    return inp;
  }

  function btn(txt, accentColor) {
    return el('button', { textContent: txt }, {
      all: 'initial',
      display: 'inline-block',
      background: accentColor || COLORS.accent,
      color: accentColor === COLORS.red ? '#fff' : '#000',
      border: 'none',
      borderRadius: '4px',
      padding: '5px 10px',
      fontSize: '11px',
      fontFamily: 'sans-serif',
      fontWeight: '600',
      cursor: 'pointer',
      marginRight: '6px',
      marginTop: '4px',
    });
  }

  // ── Lockout veil ─────────────────────────────────────────────────────────────
  function buildLockoutVeil(untilStr) {
    var veil = el('div', { id: LOCKOUT_VEIL_ID }, {
      all: 'initial',
      position: 'fixed',
      top: '0',
      left: '0',
      right: '0',
      bottom: '0',
      background: 'rgba(153, 27, 27, 0.55)',
      zIndex: String(Z - 1),
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      pointerEvents: 'none',
    });
    var txt = el('div', { textContent: 'LOCKED OUT until ' + untilStr }, {
      all: 'initial',
      fontFamily: 'sans-serif',
      fontSize: '28px',
      fontWeight: '900',
      color: '#fff',
      textAlign: 'center',
      pointerEvents: 'none',
    });
    veil.appendChild(txt);
    return veil;
  }

  // ── Panel builder ────────────────────────────────────────────────────────────
  function buildPanel(state) {
    var savedPos = loadPos();
    var minimized = lsGet(MIN_STORE) === 'true';

    // Root — pointer-events none on root, auto on children
    var root = el('div', { id: PANEL_ID }, {
      all: 'initial',
      position: 'fixed',
      top: savedPos ? px(savedPos.y) : '60px',
      right: savedPos ? 'auto' : '0',
      left: savedPos ? px(savedPos.x) : 'auto',
      width: minimized ? px(MINI_W) : px(PANEL_W),
      maxHeight: minimized ? '100vh' : px(PANEL_H),
      zIndex: String(Z),
      pointerEvents: 'none',
      display: 'flex',
      flexDirection: 'column',
    });

    // Inner panel
    var panel = el('div', {}, {
      all: 'initial',
      display: 'flex',
      flexDirection: 'column',
      width: '100%',
      height: '100%',
      maxHeight: minimized ? '100vh' : px(PANEL_H),
      background: COLORS.bg,
      border: '1px solid ' + COLORS.border,
      borderRadius: '6px',
      overflow: 'hidden',
      boxShadow: '0 4px 24px rgba(0,0,0,0.7)',
      pointerEvents: 'auto',
      boxSizing: 'border-box',
    });
    root.appendChild(panel);

    // ── Drag handle ──────────────────────────────────────────────────────────
    var handle = el('div', {}, {
      all: 'initial',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'space-between',
      background: COLORS.handle,
      padding: minimized ? '6px 4px' : '6px 10px',
      cursor: 'grab',
      userSelect: 'none',
      flexShrink: '0',
    });

    var logoTxt = minimized ? 'W' : 'W Whiskey';
    var logo = el('span', { textContent: logoTxt }, {
      all: 'initial',
      fontFamily: 'monospace',
      fontWeight: '700',
      color: COLORS.accent,
      fontSize: minimized ? '11px' : '12px',
    });
    handle.appendChild(logo);

    if (!minimized) {
      var minBtn = el('button', { textContent: '—' }, {
        all: 'initial',
        background: 'none',
        border: 'none',
        color: COLORS.muted,
        cursor: 'pointer',
        fontSize: '14px',
        lineHeight: '1',
        padding: '0 2px',
      });
      minBtn.addEventListener('click', function(ev) {
        ev.stopPropagation();
        lsSet(MIN_STORE, 'true');
        rebuild(state);
      });
      handle.appendChild(minBtn);
    } else {
      var expBtn = el('button', { textContent: '+' }, {
        all: 'initial',
        background: 'none',
        border: 'none',
        color: COLORS.muted,
        cursor: 'pointer',
        fontSize: '14px',
        lineHeight: '1',
        padding: '0',
      });
      expBtn.addEventListener('click', function(ev) {
        ev.stopPropagation();
        lsSet(MIN_STORE, 'false');
        rebuild(state);
      });
      handle.appendChild(expBtn);
    }
    panel.appendChild(handle);

    // Drag logic
    var dragging = false, ox = 0, oy = 0;
    handle.addEventListener('mousedown', function(ev) {
      if (ev.button !== 0) return;
      dragging = true;
      var rect = root.getBoundingClientRect();
      ox = ev.clientX - rect.left;
      oy = ev.clientY - rect.top;
      handle.style.cursor = 'grabbing';
      ev.preventDefault();
    });
    document.addEventListener('mousemove', function(ev) {
      if (!dragging) return;
      var nx = ev.clientX - ox;
      var ny = ev.clientY - oy;
      root.style.left = px(nx);
      root.style.top = px(ny);
      root.style.right = 'auto';
      savePos(nx, ny);
    });
    document.addEventListener('mouseup', function() {
      if (dragging) {
        dragging = false;
        handle.style.cursor = 'grab';
      }
    });

    if (minimized) {
      // minimized strip: just show chip dots for active tag
      if (state && state.active_tag) {
        var dot = el('div', {}, {
          all: 'initial',
          display: 'block',
          width: '8px',
          height: '8px',
          borderRadius: '50%',
          background: COLORS.accent,
          margin: '6px auto',
        });
        panel.appendChild(dot);
      }
      return root;
    }

    // ── Scrollable content ────────────────────────────────────────────────────
    var content = el('div', {}, {
      all: 'initial',
      display: 'block',
      flex: '1',
      overflowY: 'auto',
      overflowX: 'hidden',
      padding: '10px',
      boxSizing: 'border-box',
    });
    panel.appendChild(content);

    // ── SECTION 1: Symbol favorites ──────────────────────────────────────────
    content.appendChild(label('Symbols'));

    var favorites = (state && Array.isArray(state.favorites)) ? state.favorites : [];
    var favList = el('div', {}, {
      all: 'initial',
      display: 'flex',
      flexDirection: 'column',
      gap: '4px',
      marginBottom: '6px',
    });
    favorites.forEach(function(sym) {
      var fb = el('button', { textContent: sym }, {
        all: 'initial',
        display: 'block',
        width: '100%',
        boxSizing: 'border-box',
        background: COLORS.chipBg,
        border: '1px solid ' + COLORS.border,
        borderRadius: '4px',
        color: COLORS.text,
        fontSize: '12px',
        fontFamily: 'monospace',
        padding: '5px 8px',
        cursor: 'pointer',
        textAlign: 'left',
      });
      fb.addEventListener('mouseenter', function() {
        fb.style.borderColor = COLORS.accent;
        fb.style.color = COLORS.accent;
      });
      fb.addEventListener('mouseleave', function() {
        fb.style.borderColor = COLORS.border;
        fb.style.color = COLORS.text;
      });
      fb.addEventListener('click', function() {
        postCommand({ type: 'set_symbol', symbol: sym });
      });
      favList.appendChild(fb);
    });
    if (favorites.length === 0) {
      favList.appendChild(el('div', { textContent: 'No favorites — add in TK\'s Mods' }, {
        all: 'initial',
        display: 'block',
        fontSize: '11px',
        color: COLORS.muted,
        fontFamily: 'sans-serif',
      }));
    }
    content.appendChild(favList);
    content.appendChild(sep());

    // ── SECTION 2: Quick SL/TP form ──────────────────────────────────────────
    content.appendChild(label('SL / TP'));

    var defaultSltp = (state && state.default_sltp) ? state.default_sltp : [0, 0, 0];
    var entryInp = numInput('Entry', defaultSltp[0] || '');
    var stopInp = numInput('Stop', defaultSltp[1] || '');
    var targetInp = numInput('Target', defaultSltp[2] || '');
    content.appendChild(entryInp);
    content.appendChild(stopInp);
    content.appendChild(targetInp);

    var sltpRow = el('div', {}, { all: 'initial', display: 'flex', flexWrap: 'wrap' });
    var drawBtn = btn('Draw', COLORS.accent);
    drawBtn.addEventListener('click', function() {
      var e = parseFloat(entryInp.value);
      var s = parseFloat(stopInp.value);
      var t = parseFloat(targetInp.value);
      if (isNaN(e) || isNaN(s) || isNaN(t)) return;
      postCommand({ type: 'draw_sltp', entry: e, stop: s, target: t });
    });
    var clearBtn = btn('Clear', '#555');
    clearBtn.style.color = COLORS.text;
    clearBtn.addEventListener('click', function() {
      postCommand({ type: 'clear_sltp' });
    });
    sltpRow.appendChild(drawBtn);
    sltpRow.appendChild(clearBtn);
    content.appendChild(sltpRow);
    content.appendChild(sep());

    // ── SECTION 3: Order-flow tag chips ──────────────────────────────────────
    content.appendChild(label('Order Flow'));

    var activeTag = (state && state.active_tag) ? state.active_tag : null;
    var chipGrid = el('div', {}, {
      all: 'initial',
      display: 'grid',
      gridTemplateColumns: '1fr 1fr',
      gap: '4px',
      marginBottom: '6px',
    });
    TAG_CHIPS.forEach(function(tag) {
      var isActive = tag === activeTag;
      var chip = el('button', { textContent: tag.replace(/_/g, ' ') }, {
        all: 'initial',
        display: 'block',
        background: isActive ? COLORS.chipActive : COLORS.chipBg,
        border: '1px solid ' + (isActive ? COLORS.chipActive : COLORS.border),
        borderRadius: '4px',
        color: isActive ? COLORS.chipActiveTxt : COLORS.text,
        fontSize: '10px',
        fontFamily: 'sans-serif',
        padding: '4px 6px',
        cursor: 'pointer',
        textAlign: 'center',
        transition: 'background 0.15s',
      });
      chip.dataset.whiskeyTag = tag;
      chip.addEventListener('click', function() {
        postCommand({ type: 'order_flow_tag', tag: tag });
        // Visual flash
        chip.style.background = COLORS.accent;
        chip.style.color = '#000';
        setTimeout(function() {
          chip.style.background = isActive ? COLORS.chipActive : COLORS.chipBg;
          chip.style.color = isActive ? COLORS.chipActiveTxt : COLORS.text;
        }, 600);
      });
      chipGrid.appendChild(chip);
    });
    content.appendChild(chipGrid);
    content.appendChild(sep());

    // ── SECTION 4: Lockout status ─────────────────────────────────────────────
    var lockout = (state && state.lockout) ? state.lockout : null;
    if (lockout && lockout.is_locked) {
      var until = formatLockoutUntil(lockout.locked_until_unix);
      var loDiv = el('div', { textContent: 'LOCKED OUT until ' + until }, {
        all: 'initial',
        display: 'block',
        background: COLORS.redDark,
        borderRadius: '4px',
        color: '#fff',
        fontSize: '11px',
        fontFamily: 'sans-serif',
        fontWeight: '700',
        padding: '6px 8px',
        textAlign: 'center',
      });
      content.appendChild(loDiv);
    }

    return root;
  }

  function formatLockoutUntil(unix) {
    if (!unix) return '?';
    try {
      var d = new Date(unix * 1000);
      return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    } catch (_) { return '?'; }
  }

  // ── Rebuild helper (re-creates panel from current state) ──────────────────
  var currentState = null;

  function rebuild(state) {
    if (state !== undefined) currentState = state;
    var existing = document.getElementById(PANEL_ID);
    if (existing) existing.remove();
    var veil = document.getElementById(LOCKOUT_VEIL_ID);
    if (veil) veil.remove();

    var panel = buildPanel(currentState);
    document.body.appendChild(panel);

    // Lockout veil
    var lockout = currentState && currentState.lockout;
    if (lockout && lockout.is_locked) {
      var until = formatLockoutUntil(lockout.locked_until_unix);
      document.body.appendChild(buildLockoutVeil(until));
    }
  }

  // ── State poll ────────────────────────────────────────────────────────────
  var lastStateJson = null;
  setInterval(function() {
    var s = window[STATE_KEY];
    if (!s) return;
    var raw;
    try { raw = JSON.stringify(s); } catch (_) { return; }
    if (raw === lastStateJson) return;
    lastStateJson = raw;
    rebuild(s);
  }, 100);

  // ── MutationObserver — re-create if removed ───────────────────────────────
  var mo = new MutationObserver(function() {
    if (!document.getElementById(PANEL_ID)) {
      rebuild(currentState);
    }
  });
  mo.observe(document.body, { childList: true });

  // ── Initial render ─────────────────────────────────────────────────────────
  rebuild(null);

  // ── Node.js export for unit tests ─────────────────────────────────────────
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = {
      postCommand: postCommand,
      formatLockoutUntil: formatLockoutUntil,
      PANEL_ID: PANEL_ID,
      TAG_CHIPS: TAG_CHIPS,
    };
  }

  return { ok: true, panel_id: PANEL_ID };
})();
