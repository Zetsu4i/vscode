/*---------------------------------------------------------------------------------------------
 *  VS Code ⇄ Tauri preload shim (Phase 1 prototype).
 *
 *  Runs as a WebView initialization script *before any page script*, reproducing the
 *  globals that Electron's preload would otherwise expose:
 *
 *    - `window.vscode`   → mirrors src/vs/base/parts/sandbox/electron-browser/preload.ts
 *                          (ipcRenderer, ipcMessagePort, webFrame, webUtils, process, context)
 *    - `window.process`  → platform/arch/env/argv/versions subset (sandboxed-renderer shape)
 *    - `setImmediate` / `clearImmediate`, `Buffer` (minimal), `global`
 *
 *  The values for __VSCODE_TAURI_*__ placeholders are injected by src-tauri/src/shim.rs.
 *  IPC is forwarded to the Rust shell through Tauri's bridge, which is itself injected
 *  before user initialization scripts; all access is lazy so ordering never matters.
 *
 *  Phase status is tracked in ROADMAP.md (Phase 1); unimplemented channels are logged
 *  by the Rust side and catalogued in Phase 2.
 *--------------------------------------------------------------------------------------------*/

(function () {
	'use strict';

	if (window.__vscodeTauriShimInstalled) {
		return;
	}
	try {
		Object.defineProperty(window, '__vscodeTauriShimInstalled', { value: true, configurable: false });
	} catch (e) {
		window.__vscodeTauriShimInstalled = true;
	}

	//#region Diagnostics ring buffer (inspectable from devtools: window.__VSCODE_TAURI_DIAGNOSTICS__)

	var DIAG = (window.__VSCODE_TAURI_DIAGNOSTICS__ = { boot: [], errors: [], ipc: [] });
	function diag(kind, entry) {
		var bucket = DIAG[kind] || (DIAG[kind] = []);
		bucket.push(entry);
		if (bucket.length > 200) {
			bucket.shift();
		}
	}

	//#endregion

	//#region Injected shell values

	var PLATFORM = __VSCODE_TAURI_PLATFORM__;
	var ARCH = __VSCODE_TAURI_ARCH__;
	var EXEC_PATH = __VSCODE_TAURI_EXEC_PATH__;
	var APP_ROOT = __VSCODE_TAURI_APP_ROOT__;
	var WINDOW_ID = __VSCODE_TAURI_WINDOW_ID__;
	var SHELL_ENV = __VSCODE_TAURI_ENV__;

	// Electron main passes this channel to the renderer via argv; the preload parses it
	// out (see preload.ts `parseArgv`). The shim *is* the preload, so we synthesize argv
	// for fidelity and debugging, even though `context.resolveConfiguration` below does
	// not actually need to parse it.
	var WINDOW_CONFIG_CHANNEL = 'vscode:window-config:tauri-main';

	//#endregion

	//#region Tauri bridge (lazy)

	function tauriBridge() {
		if (window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {
			return window.__TAURI_INTERNALS__;
		}
		if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
			return window.__TAURI__.core;
		}
		return null;
	}

	function tauriInvoke(command, payload) {
		var bridge = tauriBridge();
		if (!bridge) {
			diag('errors', 'Tauri bridge unavailable for command `' + command + '`');
			return Promise.resolve(null);
		}
		try {
			return bridge.invoke(command, payload).catch(function (error) {
				diag('errors', 'command `' + command + '` failed: ' + error);
				return null;
			});
		} catch (error) {
			diag('errors', 'command `' + command + '` threw synchronously: ' + error);
			return Promise.resolve(null);
		}
	}

	function logToShell(level, message) {
		var bridge = tauriBridge();
		if (!bridge) {
			return;
		}
		try {
			bridge.invoke('renderer_log', { level: level, message: String(message) }).catch(function () { });
		} catch (error) { /* fire-and-forget */ }
	}

	window.addEventListener('error', function (event) {
		var message = 'Unhandled error: ' + event.message +
			' (' + (event.filename || 'unknown') + ':' + (event.lineno || 0) + ')';
		diag('errors', message);
		logToShell('error', message);
	});

	window.addEventListener('unhandledrejection', function (event) {
		var reason = event.reason;
		var text = reason && (reason.stack || reason.message) ? (reason.stack || reason.message) : String(reason);
		var message = 'Unhandled rejection: ' + text;
		diag('errors', message);
		logToShell('error', message);
	});

	//#endregion

	//#region Node-ish globals: global, setImmediate, Buffer

	if (typeof window.global === 'undefined') {
		try {
			Object.defineProperty(window, 'global', { value: window, configurable: false, writable: false });
		} catch (error) {
			window.global = window;
		}
	}

	if (typeof window.setImmediate !== 'function') {
		(function () {
			var channel = new MessageChannel();
			var queue = new Map();
			var sequence = 0;

			channel.port1.onmessage = function (event) {
				var entry = queue.get(event.data);
				queue.delete(event.data);
				if (entry) {
					entry.fn.apply(null, entry.args);
				}
			};

			window.setImmediate = function (fn) {
				var args = Array.prototype.slice.call(arguments, 1);
				var id = ++sequence;
				queue.set(id, { fn: fn, args: args });
				channel.port2.postMessage(id);
				return id;
			};

			window.clearImmediate = function (id) {
				queue.delete(id);
			};
		})();
	}

	// Minimal Buffer for code paths written against the Electron/Node environment.
	// Covers the shapes the workbench uses during boot; anything exotic is recorded
	// in the diagnostics ring so gaps surface during Phase 2 contract extraction.
	var textEncoder = new TextEncoder();
	var textDecoder = new TextDecoder('utf-8');

	class BufferLite extends Uint8Array {
		constructor(arg) {
			if (typeof arg === 'number') {
				super(arg);
			} else if (arg instanceof Uint8Array) {
				super(arg.length);
				this.set(arg);
			} else if (arg instanceof ArrayBuffer) {
				super(arg);
			} else {
				super(0);
			}
		}

		toString(encoding) {
			if (!encoding || encoding === 'utf8' || encoding === 'utf-8') {
				return textDecoder.decode(this);
			}
			if (encoding === 'base64') {
				var binary = '';
				var CHUNK = 0x8000;
				for (var i = 0; i < this.length; i += CHUNK) {
					binary += String.fromCharCode.apply(null, this.subarray(i, i + CHUNK));
				}
				return btoa(binary);
			}
			if (encoding === 'latin1' || encoding === 'binary') {
				var out = '';
				for (var j = 0; j < this.length; j++) {
					out += String.fromCharCode(this[j]);
				}
				return out;
			}
			diag('errors', 'Buffer.toString: unsupported encoding `' + encoding + '`');
			return textDecoder.decode(this);
		}

		static from(input) {
			if (typeof input === 'string') {
				return new BufferLite(textEncoder.encode(input));
			}
			if (input instanceof Uint8Array) {
				return new BufferLite(input);
			}
			if (Array.isArray(input) || ArrayBuffer.isView(input)) {
				return new BufferLite(new Uint8Array(input.buffer || input));
			}
			diag('errors', 'Buffer.from: unsupported input type ' + Object.prototype.toString.call(input));
			return new BufferLite(0);
		}

		static alloc(size, fill) {
			var out = new BufferLite(size);
			if (arguments.length > 1 && fill !== 0) {
				out.fill(fill);
			}
			return out;
		}

		static allocUnsafe(size) {
			return new BufferLite(size);
		}

		static byteLength(value, encoding) {
			if (typeof value !== 'string') {
				return value.length || 0;
			}
			if (encoding === 'base64') {
				return Math.floor(value.length * 3 / 4);
			}
			return textEncoder.encode(value).length;
		}

		static isBuffer(value) {
			return value instanceof BufferLite;
		}

		static concat(list) {
			var total = 0;
			for (var i = 0; i < list.length; i++) {
				total += list[i].length;
			}
			var out = new BufferLite(total);
			var offset = 0;
			for (var j = 0; j < list.length; j++) {
				out.set(list[j], offset);
				offset += list[j].length;
			}
			return out;
		}
	}

	if (typeof window.Buffer === 'undefined') {
		try {
			Object.defineProperty(window, 'Buffer', { value: BufferLite, configurable: false, writable: false });
		} catch (error) {
			window.Buffer = BufferLite;
		}
	}

	//#endregion

	//#region process (sandboxed-renderer subset — see ISandboxNodeProcess)

	function chromeVersion() {
		var match = /Chrome\/([0-9.]+)/.exec(navigator.userAgent);
		return match ? match[1] : '0.0.0.0';
	}

	var processEnv = Object.assign(Object.create(null), SHELL_ENV || {});

	var processObject = {
		platform: PLATFORM,
		arch: ARCH,
		type: 'renderer',
		execPath: EXEC_PATH,
		// Electron main injects this so the preload can find its window-config channel.
		argv: [EXEC_PATH, '--vscode-window-config=' + WINDOW_CONFIG_CHANNEL],
		// TODO(Phase 2): keep node/electron versions aligned with the WebView2 we ship.
		versions: {
			chrome: chromeVersion(),
			node: '22.14.0',
			electron: '33.3.1',
			v8: '13.0.100.0'
		},
		env: processEnv,
		cwd: function () {
			return APP_ROOT;
		},
		getProcessMemoryInfo: function () {
			// WebView2 exposes no per-process memory API; report zeros until wired up.
			return Promise.resolve({ private: 0, residentSet: 0, shared: 0 });
		},
		shellEnv: function () {
			return Promise.resolve(tauriInvoke('ipc_invoke', { channel: 'vscode:fetchShellEnv', args: null }));
		},
		on: function (type, callback) {
			// TODO(Phase 3): forward process events from the Rust shell.
			diag('ipc', 'process.on("' + type + '") registered (not yet forwarded)');
		}
	};

	if (typeof window.process === 'undefined') {
		try {
			Object.defineProperty(window, 'process', { value: processObject, configurable: false, writable: false });
		} catch (error) {
			window.process = processObject;
		}
	}

	//#endregion

	//#region window.vscode (mirrors preload.ts globals)

	function validateIPC(channel) {
		if (typeof channel !== 'string' || channel.indexOf('vscode:') !== 0) {
			throw new Error("Unsupported event IPC channel '" + channel + "'");
		}
		return true;
	}

	function noteChannel(channel, kind) {
		diag('ipc', kind + ' ' + channel);
	}

	// Electron invoke/send are variadic; Tauri IPC takes one JSON payload.
	function normalizeArgs(args) {
		if (args.length === 0) {
			return null;
		}
		if (args.length === 1) {
			return args[0] === undefined ? null : args[0];
		}
		return Array.prototype.slice.call(args);
	}

	var ipcListeners = new Map(); // channel -> Set<listener>

	function listenersFor(channel) {
		var set = ipcListeners.get(channel);
		if (!set) {
			set = new Set();
			ipcListeners.set(channel, set);
		}
		return set;
	}

	// TODO(Phase 3): Rust → JS event delivery. The shell will emit over the Tauri event
	// bus and `dispatchFromShell` will fan out to the listeners registered here.
	function dispatchFromShell(channel) {
		var args = Array.prototype.slice.call(arguments, 1);
		var set = ipcListeners.get(channel);
		if (!set) {
			return;
		}
		set.forEach(function (listener) {
			try {
				listener({ sender: null, frameId: 0, processId: 0 }, args.length === 1 ? args[0] : args);
			} catch (error) {
				diag('errors', 'ipc listener for `' + channel + '` threw: ' + error);
			}
		});
	}
	window.__vscodeTauriDispatchIpc = dispatchFromShell;

	var ipcRenderer = {
		send: function (channel) {
			validateIPC(channel);
			noteChannel(channel, 'send');
			var args = normalizeArgs(Array.prototype.slice.call(arguments, 1));
			tauriInvoke('ipc_send', { channel: channel, args: args });
		},
		invoke: function (channel) {
			validateIPC(channel);
			noteChannel(channel, 'invoke');
			var args = normalizeArgs(Array.prototype.slice.call(arguments, 1));
			return tauriInvoke('ipc_invoke', { channel: channel, args: args });
		},
		on: function (channel, listener) {
			validateIPC(channel);
			listenersFor(channel).add(listener);
			return this;
		},
		once: function (channel, listener) {
			validateIPC(channel);
			var self = this;
			var wrapped = function (event) {
				self.removeListener(channel, wrapped);
				listener.apply(null, arguments);
			};
			this.on(channel, wrapped);
			return this;
		},
		removeListener: function (channel, listener) {
			validateIPC(channel);
			var set = ipcListeners.get(channel);
			if (set) {
				set.delete(listener);
				if (set.size === 0) {
					ipcListeners.delete(channel);
				}
			}
			return this;
		}
	};

	var ipcMessagePort = {
		acquire: function (responseChannel, nonce) {
			validateIPC(responseChannel);
			// Mirrors preload.ts: wait for the main side to answer with a MessagePort.
			// The Rust shell does not host message ports yet (Phase 7 territory); the
			// listener simply stays pending rather than erroring.
			var responseListener = function (event, response) {
				var responseNonce = typeof response === 'string' ? response : (response && response.nonce);
				if (nonce === responseNonce) {
					ipcRenderer.removeListener(responseChannel, responseListener);
					window.postMessage(response, '*', []);
				}
			};
			ipcRenderer.on(responseChannel, responseListener);
		}
	};

	var webFrame = {
		setZoomLevel: function (level) {
			if (typeof level === 'number') {
				tauriInvoke('native_set_zoom_level', { level: level });
			}
		}
	};

	var webUtils = {
		getPathForFile: function (file) {
			// TODO(Phase 4): Electron resolves dragged/dropped File objects to real
			// filesystem paths; the Rust file service will own this mapping.
			diag('errors', 'webUtils.getPathForFile stub called for `' + (file && file.name) + '`');
			return (file && file.name) || '';
		}
	};

	var configuration; // resolved once `resolveConfiguration` settles (matches preload.ts)

	var resolveConfigurationPromise = (async function () {
		try {
			var config = await tauriInvoke('ipc_invoke', { channel: WINDOW_CONFIG_CHANNEL, args: null });
			if (!config || typeof config !== 'object') {
				throw new Error('window configuration unavailable (is the Rust ipc handler running?)');
			}
			configuration = config;
			try {
				Object.assign(processEnv, config.userEnv || {});
			} catch (error) {
				diag('errors', 'applying userEnv failed: ' + error);
			}
			diag('boot', 'window configuration resolved');
			logToShell('info', 'window configuration resolved');
			return config;
		} catch (error) {
			var message = 'vscode-tauri: resolving window configuration failed: ' + error;
			diag('errors', message);
			logToShell('error', message);
			throw error;
		}
	})();

	var context = {
		configuration: function () {
			return configuration;
		},
		resolveConfiguration: function () {
			return resolveConfigurationPromise;
		}
	};

	var vscodeGlobals = {
		ipcRenderer: ipcRenderer,
		ipcMessagePort: ipcMessagePort,
		webFrame: webFrame,
		webUtils: webUtils,
		process: processObject,
		context: context
	};

	try {
		Object.defineProperty(window, 'vscode', { value: vscodeGlobals, configurable: false, writable: false });
	} catch (error) {
		diag('errors', 'failed to install window.vscode: ' + error);
		window.vscode = vscodeGlobals;
	}

	//#endregion

	//#region Workbench module loading over the shim's HTTP origin

	// The stock workbench computes module URLs from `appRoot` using the vscode-file:
	// protocol (Electron-privileged). Under the shim's http origin that URL cannot
	// load, so we take the same branch `build/vite/setup-dev.ts` enables in dev:
	// relative ESM imports guarded by VSCODE_DEV + _VSCODE_USE_RELATIVE_IMPORTS.
	processEnv['VSCODE_DEV'] = '1';
	try {
		Object.defineProperty(globalThis, '_VSCODE_USE_RELATIVE_IMPORTS', { value: true, configurable: false });
	} catch (error) {
		globalThis._VSCODE_USE_RELATIVE_IMPORTS = true;
	}

	//#endregion

	diag('boot', 'shim installed (platform=' + PLATFORM + ', arch=' + ARCH + ')');
	logToShell('info', 'shim installed (platform=' + PLATFORM + ', arch=' + ARCH + ')');
}());
