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
 * `vscode:`-prefixed channel names preserved (Phase 2 of ROADMAP.md extracts
 * the full contract from the ipc-calls.jsonl log this produces).
 *
 * Main-process -> renderer events: Rust can call
 * `window.__VSTAURI_DISPATCH__(channel, ...args)` (via WebView2 script
 * evaluation) to deliver events to ipcRenderer.on listeners, mirroring
 * Electron's webContents.send.
 *
 * Notes on intentionally missing globals:
 *   - `Buffer` and `global` are NOT provided: Electron sandboxed renderers do
 *     not expose them either, and the renderer code paths never rely on them.
 *   - `setImmediate` is provided (harmless setTimeout(0) polyfill).
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
  // ipcRenderer
  // ---------------------------------------------------------------------------
  var listeners = Object.create(null); // channel -> Set<Function>

  // Entry point used by the Rust backend to push events into the renderer
  // (equivalent of Electron's webContents.send + ipcRenderer listeners).
  window.__VSTAURI_DISPATCH__ = function (channel) {
    var args = Array.prototype.slice.call(arguments, 1);
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
      var args = jsonSafe(Array.prototype.slice.call(arguments, 1));
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
  var env = {};

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

  coreInvoke('vscode_log', { level: 'info', message: 'vstauri preload shim installed (Phase 1)' }).catch(function () { });
}());
