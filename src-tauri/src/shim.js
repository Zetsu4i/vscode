/*
 * VSTauri preload compatibility shim (Phase 1).
 *
 * This initialization script replaces Electron's preload script
 * (src/vs/base/parts/sandbox/electron-browser/preload.ts in the VS Code tree).
 * It runs at document-start, before the workbench's <script type="module">
 * boots, and exposes the exact `window.vscode` surface the desktop renderer
 * expects (see IMainWindowSandboxGlobals in globals.ts):
 *
 *   window.vscode.ipcRenderer     send / invoke / on / once / removeListener
 *   window.vscode.ipcMessagePort  acquire (stub until the Rust message-channel
 *                                 service lands in a later phase)
 *   window.vscode.webFrame        setZoomLevel (mapped to WebView2 zoom via a
 *                                 Tauri command; scale = 1.2^level, same
 *                                 formula as window.ts zoomLevelToZoomFactor)
 *   window.vscode.process         platform/arch/type/versions/env/execPath/
 *                                 cwd/getProcessMemoryInfo/shellEnv/on
 *   window.vscode.context         configuration() / resolveConfiguration()
 *   window.vscode.webUtils        getPathForFile (limited: WebView2 has no
 *                                 File.path, same as any Chromium browser)
 *
 * Transport: every ipcRenderer call is forwarded to the Rust backend through
 * Tauri's core IPC (window.__TAURI_INTERNALS__.invoke) with the original
 * `vscode:`-prefixed channel names preserved. The `vscode:message` main-
 * process protocol frames (ArrayBuffers) are base64-bridged through the same
 * command; Rust answers by evaluating
 * `window.__VSTAURI_DISPATCH__('vscode:message', '<base64>')`.
 *
 * Document-origin bridge (Windows/WebView2 specifics):
 *   - wry serves `vscode-file://` through `http://vscode-file.localhost`, so
 *     the document origin is http and any literal `vscode-file://...` URL the
 *     boot code constructs (loader base, FileAccess) would be unresolvable.
 *     Two guards make the renderer's own dev-mode paths work instead:
 *       1. `process.env.VSCODE_DEV` + `_VSCODE_USE_RELATIVE_IMPORTS` turn the
 *          workbench entry import into a document-relative URL (the supported
 *          development boot path of workbench.ts).
 *       2. A `_VSCODE_FILE_ROOT` defineProperty trap keeps every
 *          FileAccess.asBrowserUri(...) URL inside this origin (the value
 *          workbench.ts assigns is a vscode-file:// URL that would 404 here).
 *   - `import './x.css'` members of the ESM graph are answered by the Rust
 *     protocol with a module that calls `globalThis._VSCODE_CSS_LOAD(url)`;
 *     that global is defined here (same implementation workbench.ts installs
 *     for Electron dev-mode css modules).
 *
 * Window chrome parity (custom titlebar):
 *   - VS Code renders its custom titlebar with a `.titlebar-drag-region`
 *     layer (Electron: -webkit-app-region: drag). We mirror that exactly:
 *     mousedown on the drag region calls Tauri's startDragging(), double
 *     click toggles maximize.
 *   - On Windows Electron the min/max/close buttons are NATIVE titleBarOverlay
 *     controls; `.window-controls-container` only reserves their space. With
 *     window decorations removed we inject the three `.window-icon` buttons
 *     into that container (the same DOM shape the CSS already styles) and
 *     wire them to the Tauri window APIs.
 *   - The system context menu is suppressed globally so the workbench's own
 *     context menus render without the WebView2 fallback menu on top.
 */
(function () {
  'use strict';

  // Only install in the main frame; auxiliary/webview frames get their own
  // environments in later phases.
  if (window !== window.top) {
    return;
  }

  var CHANNEL_PREFIX = 'vscode:';

  // ---------------------------------------------------------------------------
  // Transport: lazy access to Tauri's core IPC. Tolerates initialization
  // ordering differences between WebView2 and wry script injection.
  // ---------------------------------------------------------------------------
  function coreInvoke(cmd, payload) {
    return new Promise(function (resolve, reject) {
      var attempts = 0;
      function attempt() {
        var internals = window.__TAURI_INTERNALS__;
        if (internals && typeof internals.invoke === 'function') {
          try {
            Promise.resolve(internals.invoke(cmd, payload)).then(resolve, reject);
          } catch (err) {
            reject(err);
          }
        } else if (attempts++ > 500) {
          reject(new Error('VSTauri: Tauri IPC unavailable for command ' + cmd));
        } else {
          setTimeout(attempt, 10);
        }
      }
      attempt();
    });
  }

  // Deep JSON round-trip, mirroring how Electron IPC serializes arguments
  // (functions and undefined become null).
  function jsonSafe(value) {
    try {
      var s = JSON.stringify(value);
      return s === undefined ? null : JSON.parse(s);
    } catch (err) {
      return null;
    }
  }

  function validateIPC(channel) {
    if (typeof channel !== 'string' || channel.slice(0, CHANNEL_PREFIX.length) !== CHANNEL_PREFIX) {
      // Parity with the original preload: fail loudly on unsupported channels.
      throw new Error('Unsupported event IPC channel \'' + channel + '\'');
    }
    return true;
  }

  // ---------------------------------------------------------------------------
  // Binary helpers for the vscode:message main-process protocol frames
  // ---------------------------------------------------------------------------
  function arrayBufferToBase64(buf) {
    var bytes = buf instanceof ArrayBuffer
      ? new Uint8Array(buf)
      : (buf && buf.buffer instanceof ArrayBuffer ? new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength) : null);
    if (!bytes) {
      return null;
    }
    var binary = '';
    var CHUNK = 0x8000;
    for (var i = 0; i < bytes.length; i += CHUNK) {
      binary += String.fromCharCode.apply(null, bytes.subarray(i, Math.min(i + CHUNK, bytes.length)));
    }
    return btoa(binary);
  }

  function base64ToUint8Array(b64) {
    var binary = atob(b64);
    var bytes = new Uint8Array(binary.length);
    for (var i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }

  // ---------------------------------------------------------------------------
  // ipcRenderer
  // ---------------------------------------------------------------------------
  var listeners = Object.create(null); // channel -> Set<Function>

  // Entry point used by the Rust backend to push events into the renderer
  // (equivalent of Electron's webContents.send + ipcRenderer listeners).
  // `vscode:message` frames arrive base64-encoded and are decoded into a
  // Uint8Array, exactly what VSBuffer.wrap (used by the protocol's
  // Event.fromNodeEventEmitter mapping) accepts.
  window.__VSTAURI_DISPATCH__ = function (channel) {
    var args = Array.prototype.slice.call(arguments, 1);
    if (channel === 'vscode:message' && typeof args[0] === 'string') {
      try {
        args[0] = base64ToUint8Array(args[0]);
      } catch (err) {
        console.error('[vstauri] cannot decode protocol frame', err);
        return;
      }
    }
    var set = listeners[channel];
    if (!set) {
      return;
    }
    var fakeEvent = {
      sender: null,
      frame: null,
      processId: 0,
      frameId: 0,
      ports: []
    };
    var snapshot = Array.from(set);
    for (var i = 0; i < snapshot.length; i++) {
      try {
        snapshot[i].apply(null, [fakeEvent].concat(args));
      } catch (err) {
        console.error('[vstauri] listener error on ' + channel, err);
      }
    }
  };

  var ipcRenderer = {
    send: function (channel) {
      validateIPC(channel);
      var rawArgs = Array.prototype.slice.call(arguments, 1);

      // The main-process protocol transports binary frames over this channel
      // (`Protocol.send` -> ipcRenderer.send('vscode:message', ArrayBuffer)).
      // JSON would destroy the ArrayBuffer, so base64-bridge it explicitly.
      if (channel === 'vscode:message') {
        var b64 = arrayBufferToBase64(rawArgs[0]);
        if (b64 !== null) {
          coreInvoke('vscode_ipc', { channel: channel, args: [b64], kind: 'send' }).catch(function (err) {
            console.error('[vstauri] protocol frame send failed', err);
          });
          return;
        }
      }

      var args = jsonSafe(rawArgs);
      coreInvoke('vscode_ipc', { channel: channel, args: args, kind: 'send' }).catch(function (err) {
        console.error('[vstauri] ipc send failed for ' + channel, err);
      });
    },
    invoke: function (channel) {
      validateIPC(channel);
      var args = jsonSafe(Array.prototype.slice.call(arguments, 1));
      return coreInvoke('vscode_ipc', { channel: channel, args: args, kind: 'invoke' }).then(function (result) {
        return result === undefined ? null : result;
      });
    },
    on: function (channel, listener) {
      validateIPC(channel);
      var set = listeners[channel];
      if (!set) {
        set = new Set();
        listeners[channel] = set;
      }
      set.add(listener);
      return this;
    },
    once: function (channel, listener) {
      validateIPC(channel);
      var wrapped = function () {
        ipcRenderer.removeListener(channel, wrapped);
        listener.apply(this, arguments);
      };
      return ipcRenderer.on(channel, wrapped);
    },
    removeListener: function (channel, listener) {
      validateIPC(channel);
      var set = listeners[channel];
      if (set) {
        set.delete(listener);
      }
      return this;
    }
  };

  // ---------------------------------------------------------------------------
  // process (subset per ISandboxNodeProcess)
  // ---------------------------------------------------------------------------
  // Meta defaults; enriched from the window configuration `__vstauri` section
  // as soon as it resolves (see context.resolveConfiguration below).
  var meta = {
    arch: 'x64',
    execPath: '',
    cwd: '/',
    versions: {
      node: '22.14.0',
      v8: '13.0.0',
      electron: '37.2.0',
      chrome: '138.0.7204.100'
    }
  };

  // `process.env` starts as a copy of the OS environment the way Electron
  // sandboxed renderers see it; userEnv is merged in when the configuration
  // resolves (parity with preload.ts `Object.assign(process.env, ...)`).
  //
  // VSCODE_DEV is injected HERE (not into userEnv, so it never leaks into
  // terminals/extension hosts): it selects the renderer's documented dev boot
  // path — `_VSCODE_USE_RELATIVE_IMPORTS` + relative workbench import — which
  // is the only import form that resolves through this document's origin.
  // See workbench.ts line ~517 and shim header notes.
  var env = { VSCODE_DEV: '1' };

  var processObj = {
    get platform() { return 'win32'; },
    get arch() { return meta.arch; },
    get type() { return 'renderer'; },
    get versions() { return meta.versions; },
    get env() { return env; },
    get execPath() { return meta.execPath; },
    cwd: function () {
      return meta.cwd || '/';
    },
    getProcessMemoryInfo: function () {
      return Promise.resolve({ private: 0, residentSet: 0, shared: 0 });
    },
    shellEnv: function () {
      return ipcRenderer.invoke('vscode:fetchShellEnv');
    },
    on: function (type, callback) {
      // Electron emits process events (e.g. 'unresponsive'). No native
      // equivalent yet; registering is a no-op so code paths stay intact.
    }
  };

  // ---------------------------------------------------------------------------
  // webFrame / webUtils / ipcMessagePort
  // ---------------------------------------------------------------------------
  var webFrame = {
    setZoomLevel: function (level) {
      if (typeof level === 'number') {
        coreInvoke('vscode_set_zoom_level', { level: level }).catch(function () { /* zoom failures must not break boot */ });
      }
    }
  };

  var webUtils = {
    getPathForFile: function (file) {
      // Electron patches File objects with a real path; Chromium/WebView2
      // does not expose one. Later phases map Tauri drag-drop events to
      // paths instead.
      return (file && typeof file.path === 'string') ? file.path : '';
    }
  };

  var ipcMessagePort = {
    acquire: function (responseChannel, nonce) {
      validateIPC(responseChannel);
      // Phase 7 (extension host / utility process transport) will implement
      // the message-channel handshake. Registering nothing today matches a
      // main process that never answers: callers time out gracefully.
    }
  };

  // ---------------------------------------------------------------------------
  // context: window configuration (normally delivered by electron-main via
  // the `--vscode-window-config=vscode:window/<id>` IPC handshake).
  // ---------------------------------------------------------------------------
  var configuration;
  var configurationPromise;

  var context = {
    configuration: function () {
      return configuration;
    },
    resolveConfiguration: function () {
      if (!configurationPromise) {
        configurationPromise = coreInvoke('vscode_window_config', {}).then(function (cfg) {
          configuration = cfg;
          if (cfg && cfg.userEnv) {
            Object.assign(env, cfg.userEnv);
          }
          if (cfg && cfg.__vstauri) {
            Object.assign(meta, cfg.__vstauri);
          }
          // Parity with preload.ts: apply the persisted zoom level early.
          if (cfg && typeof cfg.zoomLevel === 'number') {
            webFrame.setZoomLevel(cfg.zoomLevel);
          }
          return cfg;
        });
      }
      return configurationPromise;
    }
  };

  // ---------------------------------------------------------------------------
  // Expose globals exactly as contextBridge.exposeInMainWorld('vscode', ...) did
  // ---------------------------------------------------------------------------
  window.vscode = {
    ipcRenderer: ipcRenderer,
    ipcMessagePort: ipcMessagePort,
    webFrame: webFrame,
    process: processObj,
    context: context,
    webUtils: webUtils
  };

  // ---------------------------------------------------------------------------
  // Misc Node-ish globals that Electron sandboxed renderers still provide
  // ---------------------------------------------------------------------------
  if (typeof window.setImmediate !== 'function') {
    window.setImmediate = function (fn) {
      return setTimeout(fn, 0);
    };
  }

  // ---------------------------------------------------------------------------
  // Dev-boot globals: force the workbench's document-relative import path
  // (see shim header). workbench.ts and sessions.ts both read this flag.
  // ---------------------------------------------------------------------------
  globalThis._VSCODE_USE_RELATIVE_IMPORTS = true;

  // `_VSCODE_FILE_ROOT` is assigned three different ways at boot
  // (bootstrap-esm.ts: import.meta.dirname; workbench.ts: a
  // `vscode-file://vscode-app/<appRoot>/out/` URL). In this origin those
  // values are unresolvable, so install a trap: every assignment is accepted
  // and ignored, and every read returns the document-origin out-root that
  // FileAccess.asBrowserUri / worker bootstraps resolve against.
  (function () {
    var stored = '';
    var outRoot = window.location.origin + '/out/';
    try {
      Object.defineProperty(globalThis, '_VSCODE_FILE_ROOT', {
        configurable: true,
        get: function () { return stored || outRoot; },
        set: function (value) {
          stored = (typeof value === 'string' && /^\w[\w\d+.-]*:\/\//.test(value)) ? outRoot : (typeof value === 'string' && value.length > 0 ? outRoot : '');
        }
      });
    } catch (err) { /* frozen global in some contexts; reads then use the raw value */ }
  })();

  // CSS module bridge: called from the wrapper modules protocol.rs serves for
  // every `import './x.css'` in the ESM graph. Same implementation as
  // workbench.ts setupCSSImportMaps installs in Electron dev mode.
  globalThis._VSCODE_CSS_LOAD = function (url) {
    var link = document.createElement('link');
    link.setAttribute('rel', 'stylesheet');
    link.setAttribute('type', 'text/css');
    link.setAttribute('href', url);
    document.head.appendChild(link);
  };

  // ---------------------------------------------------------------------------
  // Desktop window behavior parity (custom titlebar, frameless shell)
  // ---------------------------------------------------------------------------

  // Suppress the WebView2 default context menu globally. The workbench builds
  // its own context menus from mousedown/contextmenu events, so only the
  // default action is cancelled — propagation (and VS Code's menus) are
  // untouched.
  document.addEventListener('contextmenu', function (e) {
    e.preventDefault();
  }, { capture: true });

  function tauriWindow() {
    try {
      return window.__TAURI__ && window.__TAURI__.window && window.__TAURI__.window.getCurrentWindow
        ? window.__TAURI__.window.getCurrentWindow()
        : null;
    } catch (err) {
      return null;
    }
  }

  function wireTitlebar(root) {
    var dragRegion = root.querySelector('.titlebar-drag-region');
    if (dragRegion && !dragRegion.__vstauriDrag) {
      dragRegion.__vstauriDrag = true;
      dragRegion.addEventListener('mousedown', function (e) {
        if (e.button !== 0) {
          return;
        }
        var w = tauriWindow();
        if (w) {
          w.startDragging().catch(function () { });
        }
      });
      // Windows convention: double click on the titlebar toggles maximize.
      dragRegion.addEventListener('dblclick', function (e) {
        if (e.button !== 0) {
          return;
        }
        var w = tauriWindow();
        if (w) {
          w.toggleMaximize().catch(function () { });
        }
      });
    }

    var controls = root.querySelector('.window-controls-container:not(.web)');
    if (controls && !controls.__vstauriControls && !controls.childElementCount) {
      controls.__vstauriControls = true;
      var defs = [
        { icon: 'codicon-chrome-minimize', label: 'Minimize', action: function (w) { w.minimize().catch(function () { }); } },
        { icon: 'codicon-chrome-maximize', label: 'Maximize', action: function (w, btn) { w.toggleMaximize().then(function () { syncMaximizeIcon(w, btn); }).catch(function () { }); } },
        { icon: 'codicon-chrome-close', label: 'Close', cls: ' window-close', action: function (w) { w.close().catch(function () { }); } }
      ];
      var maximizeBtn = null;
      for (var i = 0; i < defs.length; i++) {
        (function (def) {
          var btn = document.createElement('button');
          btn.className = 'window-icon codicon ' + def.icon + (def.cls || '');
          btn.setAttribute('aria-label', def.label);
          btn.setAttribute('title', def.label);
          btn.addEventListener('click', function (e) {
            e.stopPropagation();
            var w = tauriWindow();
            if (w) {
              def.action(w, btn);
            }
          });
          if (def.icon === 'codicon-chrome-maximize') {
            maximizeBtn = btn;
          }
          controls.appendChild(btn);
        })(defs[i]);
      }
      if (maximizeBtn) {
        var w = tauriWindow();
        if (w) {
          syncMaximizeIcon(w, maximizeBtn);
        }
      }
    }
  }

  function syncMaximizeIcon(w, btn) {
    if (!w || !btn) {
      return;
    }
    w.isMaximized().then(function (maximized) {
      btn.classList.toggle('codicon-chrome-maximize', !maximized);
      btn.classList.toggle('codicon-chrome-restore', !!maximized);
      btn.setAttribute('aria-label', maximized ? 'Restore' : 'Maximize');
    }).catch(function () { });
  }

  // The titlebar part is created asynchronously during workbench layout
  // creation; watch for it and wire drag + window controls as soon as it
  // exists. Observed on the document node itself: at document-created time
  // `document.documentElement` may not exist yet.
  var titlebarObserver = new MutationObserver(function () {
    if (window.__VSTAURI_TITLEBAR_READY__) {
      return;
    }
    var root = document.querySelector('.monaco-workbench .part.titlebar .titlebar-container')
      || document.querySelector('.titlebar-container');
    if (root) {
      window.__VSTAURI_TITLEBAR_READY__ = true;
      wireTitlebar(root);
      titlebarObserver.disconnect();
    }
  });
  titlebarObserver.observe(document, { childList: true, subtree: true });

  // ---------------------------------------------------------------------------
  // Diagnostics: forward renderer errors to the Rust log so headless
  // iterations (CI + user machines without devtools) can see them.
  // ---------------------------------------------------------------------------
  var originalConsoleError = console.error ? console.error.bind(console) : null;

  function stringifyErrorArg(arg) {
    if (typeof arg === 'string') {
      return arg;
    }
    if (arg && arg.stack && typeof arg.stack === 'string') {
      return arg.stack;
    }
    try {
      return JSON.stringify(arg);
    } catch (err) {
      return String(arg);
    }
  }

  console.error = function () {
    if (originalConsoleError) {
      originalConsoleError.apply(console, arguments);
    }
    try {
      var parts = [];
      for (var i = 0; i < arguments.length; i++) {
        parts.push(stringifyErrorArg(arguments[i]));
      }
      var message = parts.join(' ').slice(0, 4000);
      coreInvoke('vscode_log', { level: 'error', message: message }).catch(function () { /* never break console */ });
    } catch (err) { /* never break console */ }
  };

  window.addEventListener('error', function (ev) {
    console.error('[vstauri][window.onerror] ' + (ev.message || 'unknown error') +
      ' @ ' + (ev.filename || '?') + ':' + (ev.lineno || 0) + ':' + (ev.colno || 0));
  });

  window.addEventListener('unhandledrejection', function (ev) {
    var reason = ev && ev.reason;
    console.error('[vstauri][unhandledrejection] ' + (reason && reason.stack ? reason.stack : String(reason)));
  });

  coreInvoke('vscode_log', {
    level: 'info',
    message: 'vstauri preload shim installed (Phase 1: origin ' + window.location.origin + ', dev-relative imports, css bridge, custom titlebar)'
  }).catch(function () { });
}());
