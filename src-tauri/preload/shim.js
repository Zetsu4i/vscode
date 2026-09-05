/*---------------------------------------------------------------------------------------------
 *  VS Code Tauri shell — renderer compatibility shim.
 *
 *  This is the Tauri counterpart of
 *  `src/vs/base/parts/sandbox/electron-browser/preload.ts`.
 *
 *  It runs before any workbench code and installs `globalThis.vscode` with the
 *  exact shape `globals.ts` destructures:
 *
 *      vscode.ipcRenderer     send/invoke/on/once/removeListener
 *      vscode.ipcMessagePort  acquire(responseChannel, nonce)
 *      vscode.webFrame        setZoomLevel
 *      vscode.webUtils        getPathForFile
 *      vscode.process         platform/arch/env/versions/type/execPath/cwd/
 *                             shellEnv/getProcessMemoryInfo/on
 *      vscode.context         configuration/resolveConfiguration
 *
 *  Per AGENTS.md constraint 2 all Electron/Tauri divergence is absorbed HERE.
 *  The workbench sources are never edited.
 *--------------------------------------------------------------------------------------------*/

(function () {
	'use strict';

	if (globalThis.vscode && globalThis.vscode.__tauriShim) {
		return; // already installed (auxiliary window re-entry)
	}

	const invokeTauri = (globalThis.__TAURI_INTERNALS__ && globalThis.__TAURI_INTERNALS__.invoke)
		? globalThis.__TAURI_INTERNALS__.invoke
		: () => Promise.reject(new Error('Tauri IPC unavailable'));

	//#region Node-ish globals the workbench assumes exist

	if (typeof globalThis.global === 'undefined') {
		globalThis.global = globalThis;
	}

	if (typeof globalThis.setImmediate !== 'function') {
		const pending = new Map();
		let nextId = 1;
		const channel = new MessageChannel();
		channel.port1.onmessage = (event) => {
			const entry = pending.get(event.data);
			if (entry) {
				pending.delete(event.data);
				try {
					entry.fn(...entry.args);
				} catch (err) {
					console.error(err);
				}
			}
		};
		globalThis.setImmediate = function (fn, ...args) {
			const id = nextId++;
			pending.set(id, { fn, args });
			channel.port2.postMessage(id);
			return id;
		};
		globalThis.clearImmediate = function (id) { pending.delete(id); };
	}

	//#endregion

	//#region Channel validation (identical rule to preload.ts)

	function validateIPC(channel) {
		if (!channel || typeof channel !== 'string' || !channel.startsWith('vscode:')) {
			throw new Error(`Unsupported event IPC channel '${channel}'`);
		}
		return true;
	}

	//#endregion

	//#region ipcRenderer over Tauri IPC + a Tauri event stream

	/** channel -> Set<listener> */
	const listeners = new Map();

	function emit(channel, args) {
		const set = listeners.get(channel);
		if (!set || set.size === 0) {
			return;
		}
		// Electron hands listeners an `IpcRendererEvent` first; the workbench
		// only ever reads `.ports` and `.sender`, so a minimal object suffices.
		const event = { sender: ipcRenderer, ports: args && args.__ports ? args.__ports : [] };
		for (const listener of Array.from(set)) {
			try {
				listener(event, ...(args && args.payload ? args.payload : []));
			} catch (err) {
				console.error(`[shim] listener for ${channel} threw`, err);
			}
		}
	}

	const ipcRenderer = {
		send(channel, ...args) {
			validateIPC(channel);
			invokeTauri('ipc_send', { channel, args: serializable(args) })
				.catch((err) => console.error(`[shim] send ${channel} failed`, err));
		},

		invoke(channel, ...args) {
			validateIPC(channel);
			return invokeTauri('ipc_invoke', { channel, args: serializable(args) });
		},

		on(channel, listener) {
			validateIPC(channel);
			let set = listeners.get(channel);
			if (!set) {
				set = new Set();
				listeners.set(channel, set);
				invokeTauri('ipc_subscribe', { channel }).catch(() => { /* best effort */ });
			}
			set.add(listener);
			return this;
		},

		once(channel, listener) {
			validateIPC(channel);
			const wrapped = (...args) => {
				this.removeListener(channel, wrapped);
				listener(...args);
			};
			return this.on(channel, wrapped);
		},

		removeListener(channel, listener) {
			const set = listeners.get(channel);
			if (set) {
				set.delete(listener);
			}
			return this;
		},

		off(channel, listener) {
			return this.removeListener(channel, listener);
		}
	};

	// structuredClone-safe conversion: Tauri IPC is JSON, Electron IPC is
	// structured-clone. VSBuffer instances travel as Uint8Array -> array.
	function serializable(value) {
		if (value instanceof Uint8Array) {
			return { __u8: Array.from(value) };
		}
		if (Array.isArray(value)) {
			return value.map(serializable);
		}
		if (value && typeof value === 'object') {
			if (value.buffer instanceof Uint8Array) {
				return { __u8: Array.from(value.buffer) };
			}
			const out = {};
			for (const key of Object.keys(value)) {
				out[key] = serializable(value[key]);
			}
			return out;
		}
		return value;
	}

	function deserialize(value) {
		if (value && typeof value === 'object') {
			if (Array.isArray(value.__u8)) {
				return new Uint8Array(value.__u8);
			}
			if (Array.isArray(value)) {
				return value.map(deserialize);
			}
			const out = {};
			for (const key of Object.keys(value)) {
				out[key] = deserialize(value[key]);
			}
			return out;
		}
		return value;
	}

	// The Rust side pushes main->renderer traffic on one Tauri event.
	if (globalThis.__TAURI_INTERNALS__ && typeof globalThis.__TAURI_INTERNALS__.transformCallback === 'function') {
		const callback = globalThis.__TAURI_INTERNALS__.transformCallback((message) => {
			if (!message || typeof message.channel !== 'string') {
				return;
			}
			emit(message.channel, { payload: (message.args || []).map(deserialize) });
		});
		invokeTauri('ipc_listen', { handler: callback }).catch((err) => {
			console.error('[shim] could not attach main->renderer channel', err);
		});
	}

	//#endregion

	//#region MessagePort acquisition
	//
	// Electron transfers a real MessagePort from the main process. Tauri has no
	// port transfer, so the shim creates the channel in the renderer, keeps one
	// end, and asks Rust to bind the other end to the named endpoint (pty host,
	// extension host, shared process). Rust pumps bytes between that endpoint
	// and this port. The renderer-visible contract — a `message` event on
	// `window` carrying the nonce and `e.ports[0]` — is preserved exactly.

	const ipcMessagePort = {
		acquire(responseChannel, nonce) {
			validateIPC(responseChannel);
			const channel = new MessageChannel();
			invokeTauri('message_port_acquire', { responseChannel, nonce })
				.then(() => {
					channel.port1.start();
					// Rust pumps this port; hand port2 to the requester exactly
					// like Electron's `window.postMessage(response, '*', ports)`.
					bindPortToBackend(channel.port1, responseChannel, nonce);
					window.postMessage(nonce, '*', [channel.port2]);
				})
				.catch((err) => {
					console.error(`[shim] message port ${responseChannel} failed`, err);
					window.postMessage({ nonce, error: String(err), fatal: true }, '*');
				});
		}
	};

	function bindPortToBackend(port, responseChannel, nonce) {
		port.onmessage = (event) => {
			invokeTauri('message_port_send', {
				responseChannel,
				nonce,
				data: serializable(event.data)
			}).catch((err) => console.error('[shim] port send failed', err));
		};
		if (globalThis.__TAURI_INTERNALS__ && typeof globalThis.__TAURI_INTERNALS__.transformCallback === 'function') {
			const cb = globalThis.__TAURI_INTERNALS__.transformCallback((data) => {
				port.postMessage(deserialize(data));
			});
			invokeTauri('message_port_listen', { responseChannel, nonce, handler: cb })
				.catch((err) => console.error('[shim] port listen failed', err));
		}
	}

	//#endregion

	//#region webFrame / webUtils

	const webFrame = {
		setZoomLevel(level) {
			if (typeof level !== 'number') {
				return;
			}
			// WebView2/WKWebView expose no per-frame zoom to JS, so the shim
			// scales through the document and mirrors the value to the native
			// side, which applies real window zoom where supported.
			const factor = Math.pow(1.2, level);
			document.documentElement.style.setProperty('zoom', String(factor));
			invokeTauri('set_zoom_level', { level }).catch(() => { /* optional */ });
		}
	};

	const filePaths = new WeakMap();
	const webUtils = {
		getPathForFile(file) {
			// Electron exposes the real path of a dropped File. In a webview we
			// only get it if the drop handler recorded it; the native drop
			// handler in Rust supplies paths through `vscode:drop-paths`.
			return filePaths.get(file) || '';
		}
	};

	//#endregion

	//#region process

	let resolvedConfiguration;
	let configurationValue;

	const resolveConfiguration = (async () => {
		const config = await invokeTauri('resolve_window_configuration', {});
		configurationValue = config;
		Object.assign(processShim.env, config.userEnv || {});
		webFrame.setZoomLevel(config.zoomLevel || 0);
		return config;
	})();
	resolvedConfiguration = resolveConfiguration;

	const bootstrapEnv = {};
	const processShim = {
		platform: (globalThis.__VSCODE_SHELL_PLATFORM__ || detectPlatform()),
		arch: (globalThis.__VSCODE_SHELL_ARCH__ || 'x64'),
		type: 'renderer',
		versions: { node: '20.0.0', tauri: '2', chrome: detectChromeVersion() },
		env: bootstrapEnv,
		execPath: '',

		cwd() {
			return (configurationValue && configurationValue.appRoot) || '';
		},

		async shellEnv() {
			const config = await resolvedConfiguration;
			const shellEnv = await invokeTauri('fetch_shell_env', {}).catch(() => ({}));
			return Object.assign({}, processShim.env, shellEnv, config.userEnv || {});
		},

		getProcessMemoryInfo() {
			return invokeTauri('get_process_memory_info', {})
				.catch(() => ({ residentSet: 0, private: 0, shared: 0 }));
		},

		on(type, callback) {
			// Electron forwards a small subset (`exit`, `uncaughtException`).
			if (type === 'uncaughtException') {
				window.addEventListener('error', (e) => callback(e.error || e.message));
				window.addEventListener('unhandledrejection', (e) => callback(e.reason));
			}
		},

		nextTick(fn, ...args) {
			queueMicrotask(() => fn(...args));
		}
	};

	function detectPlatform() {
		const ua = navigator.userAgent;
		if (ua.indexOf('Windows') >= 0) { return 'win32'; }
		if (ua.indexOf('Mac') >= 0) { return 'darwin'; }
		return 'linux';
	}

	function detectChromeVersion() {
		const match = /Chrome\/(\d+\.\d+\.\d+\.\d+)/.exec(navigator.userAgent);
		return match ? match[1] : '0.0.0.0';
	}

	//#endregion

	const context = {
		configuration() {
			return configurationValue;
		},
		async resolveConfiguration() {
			return resolvedConfiguration;
		}
	};

	globalThis.vscode = {
		__tauriShim: true,
		ipcRenderer,
		ipcMessagePort,
		webFrame,
		webUtils,
		process: processShim,
		context
	};

	// Some workbench code paths (and node-compat shims inside bundled deps)
	// look for a bare `process`. Electron's sandbox provides one; mirror it.
	if (typeof globalThis.process === 'undefined') {
		globalThis.process = processShim;
	}

	console.log('[vscode-tauri] renderer shim installed');
}());
