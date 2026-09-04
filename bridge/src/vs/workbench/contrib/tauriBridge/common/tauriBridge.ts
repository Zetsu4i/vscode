/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

/**
 * VSTauri bridge transport.
 *
 * The Tauri backbone serves the workbench over a local HTTP server and exposes
 * native services (file system, pty, dialogs, ...) via JSON-RPC (`/bridge/rpc/...`)
 * and a multiplexed WebSocket (`/bridge/ws`) for events. Authentication is a
 * per-session random token injected into the served workbench HTML.
 *
 * When the workbench runs outside the Tauri shell (plain browser build of
 * vscode-web) `globalThis.__VSTAURI__` is absent and `TauriBridge.get()`
 * returns undefined, keeping upstream behavior 100% intact.
 */

import { Emitter, Event } from '../../../../base/common/event.js';
import { Disposable, IDisposable, toDisposable } from '../../../../base/common/lifecycle.js';

export interface IVSTauriConfig {
	readonly token: string;
	readonly version: string;
	readonly upstreamVersion: string;
	readonly upstreamCommit: string;
	readonly platform: 'windows' | 'linux' | 'macos';
}

declare global {
	// injected by the backbone into the served workbench.html
	// eslint-disable-next-line no-var
	var __VSTAURI__: IVSTauriConfig | undefined;
}

export class BridgeUnavailableError extends Error {
	constructor() {
		super('VSTauri bridge unavailable (not running inside the Tauri shell)');
		this.name = 'BridgeUnavailableError';
	}
}

export class TauriBridge extends Disposable {

	private static _instance: TauriBridge | undefined;

	static get(): TauriBridge | undefined {
		if (!TauriBridge._instance && globalThis.__VSTAURI__) {
			TauriBridge._instance = new TauriBridge(globalThis.__VSTAURI__);
		}
		return TauriBridge._instance;
	}

	private readonly _token: string;

	private _ws: WebSocket | undefined;

	private readonly _eventEmitters = new Map<string, Set<Emitter<unknown>>>();

	private constructor(config: IVSTauriConfig) {
		super();
		this._token = config.token;
		this._connectWebSocket();
	}

	get config(): IVSTauriConfig {
		return globalThis.__VSTAURI__!;
	}

	/**
	 * JSON-RPC style call to the backbone. `args` are passed as positional
	 * JSON arguments; binary payloads use base64 strings.
	 */
	async call<T>(method: string, ...args: unknown[]): Promise<T> {
		const res = await fetch(`/bridge/rpc/${method}`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'x-vstauri-token': this._token },
			body: JSON.stringify(args)
		});
		const body = await res.json();
		if (!res.ok || body.err) {
			throw new Error(typeof body.err === 'string' ? body.err : `bridge call ${method} failed (${res.status})`);
		}
		return body.ok as T;
	}

	/**
	 * Listen to a backbone event channel. Returns an upstream `Event`.
	 */
	listen<T>(event: string): Event<T> {
		let set = this._eventEmitters.get(event);
		if (!set) {
			set = new Set<Emitter<unknown>>();
			this._eventEmitters.set(event, set);
		}
		const emitter = new Emitter<T>({
			onDidRemoveLastListener: () => {
				set!.delete(emitter);
				if (set!.size === 0) {
					this._eventEmitters.delete(event);
				}
			}
		});
		set.add(emitter as Emitter<unknown>);
		return emitter.event;
	}

	/** Adapts an upstream Disposable to detach from an event when disposed. */
	protected registerListener(event: string, emitter: Emitter<unknown>): IDisposable {
		let set = this._eventEmitters.get(event);
		if (!set) {
			set = new Set<Emitter<unknown>>();
			this._eventEmitters.set(event, set);
		}
		set.add(emitter);
		return toDisposable(() => {
			set!.delete(emitter);
			if (set!.size === 0) {
				this._eventEmitters.delete(event);
			}
		});
	}

	private _connectWebSocket(): void {
		const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
		const ws = new WebSocket(`${protocol}//${window.location.host}/bridge/ws?token=${encodeURIComponent(this._token)}`);
		ws.binaryType = 'arraybuffer';
		this._ws = ws;

		ws.onmessage = ev => {
			let msg: { event: string; payload: unknown };
			try {
				msg = JSON.parse(typeof ev.data === 'string' ? ev.data : new TextDecoder().decode(ev.data as ArrayBuffer));
			} catch {
				return;
			}
			const emitters = this._eventEmitters.get(msg.event);
			if (emitters) {
				for (const emitter of [...emitters]) {
					emitter.fire(msg.payload);
				}
			}
		};
		ws.onclose = () => {
			this._ws = undefined;
			// reconnect with backoff; the backbone lifetime matches the app's
			setTimeout(() => this._connectWebSocket(), 1000 + Math.floor(Math.random() * 2000));
		};
	}
}
