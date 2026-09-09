/*
 * VSTauri Node sidecar wrapper — the Electron "utility process" replacement.
 *
 * Spawned by the Rust shell as:
 *
 *   node.exe vstauri-sidecar.mjs
 *
 * with VSCODE_ESM_ENTRYPOINT pointing at the module Electron would have
 * loaded in a utility process (extensionHostProcess, ptyHostMain,
 * sharedProcessMain, textMate worker, ...).
 *
 * The wrapper installs a minimal Electron utility-process environment —
 * `process.parentPort` and a `MessagePortMain`-compatible port class —
 * bridged over a length-prefixed binary framing on stdin/stdout (see
 * src-tauri/src/sidecar_channel.rs for the Rust side):
 *
 *   [u32 body_len][u8 frame_type][payload]      body_len = 1 + payload.len()
 *
 *   frame_type 1 = PORT_MSG   payload = [u64 child_port_id LE][message bytes]
 *   frame_type 2 = CTRL (JSON):
 *       {"t":"port","portId":n,"data":...}   parent -> child: parentPort
 *                                           'message' event with ports:[port]
 *       {"t":"port-close","portId":n}        either: the port went away
 *       {"t":"ppm","msg":...}                child -> parent (written here)
 *       {"t":"stdout"/"stderr","data":...}   child -> parent
 *
 * After the shim it imports the ORIGINAL out/bootstrap-fork.js, which
 * imports the entry module. Nothing inside out/ knows it is not under
 * Electron.
 */

'use strict';

import { EventEmitter } from 'node:events';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

// The RAW stdout write — captured before the console-capture override
// installs, and used exclusively for frames so framing can never recurse.
const rawStdoutWrite = process.stdout.write.bind(process.stdout);

// ---------------------------------------------------------------------------
// Framing writer
// ---------------------------------------------------------------------------

/** @param {number} type @param {Buffer|Uint8Array} payload */
function writeFrame(type, payload) {
	const header = Buffer.alloc(5);
	header.writeUInt32LE(payload.length + 1, 0);
	header.writeUInt8(type, 4);
	rawStdoutWrite(Buffer.concat([header, Buffer.from(payload)]));
}

/** @param {unknown} value */
function writeCtrl(value) {
	writeFrame(2, Buffer.from(JSON.stringify(value), 'utf8'));
}

// ---------------------------------------------------------------------------
// Fake MessagePortMain (EventEmitter-based, like Electron's)
// ---------------------------------------------------------------------------

class MessagePortMainImpl extends EventEmitter {
	/**
	 * @param {number} portId
	 */
	constructor(portId) {
		super();
		this.portId = portId;
		this._closed = false;
		this._started = false;
		this._queue = [];
		this.on('newListener', (event) => {
			if (event === 'message') {
				this._started = true;
				this._flush();
			}
		});
	}

	/**
	 * @param {unknown} message
	 */
	postMessage(message) {
		if (this._closed) {
			return;
		}
		// The VS Code protocol posts raw Uint8Array/Buffer message bodies.
		const body = message instanceof Uint8Array ? message : Buffer.from(String(message));
		const payload = Buffer.alloc(8 + body.byteLength);
		payload.writeBigUInt64LE(BigInt(this.portId), 0);
		Buffer.from(body.buffer, body.byteOffset, body.byteLength).copy(payload, 8);
		writeFrame(1, payload);
	}

	start() {
		this._started = true;
		this._flush();
	}

	close() {
		if (this._closed) {
			return;
		}
		this._closed = true;
		writeCtrl({ t: 'port-close', portId: this.portId });
		this.emit('close');
	}

	/**
	 * Called by the stdin reader for incoming PORT_MSG frames.
	 * @param {Uint8Array} bytes
	 */
	_dispatch(bytes) {
		if (this._closed) {
			return;
		}
		// Copy into a plain Uint8Array — VSBuffer.wrap accepts Uint8Array and
		// several call sites do instanceof checks that must not see a Buffer.
		const copy = new Uint8Array(bytes.byteLength);
		copy.set(bytes);
		const event = { data: copy, ports: [] };
		if (!this._started) {
			this._queue.push(event);
		} else {
			this.emit('message', event);
		}
	}

	_flush() {
		if (this._queue.length > 0 && this._started) {
			const queued = this._queue;
			this._queue = [];
			for (const event of queued) {
				this.emit('message', event);
			}
		}
	}
}

/** portId -> MessagePortMainImpl */
const ports = new Map();

// ---------------------------------------------------------------------------
// process.parentPort (EventEmitter + postMessage)
// ---------------------------------------------------------------------------

const parentPort = new EventEmitter();

/**
 * Electron's ParentPort has postMessage; the VS Code shared process uses it
 * for lifecycle signaling. Mirror the API surface.
 */
parentPort.postMessage = function (message) {
	writeCtrl({ t: 'ppm', msg: message });
};

parentPort.start = function () { /* framing always flows */ };

Object.defineProperty(parentPort, 'close', {
	value: function () { /* parent disconnect */ },
	writable: false,
});

Object.defineProperty(process, 'parentPort', {
	value: parentPort,
	configurable: false,
	enumerable: true,
});

// `process.contextId` — Electron utility processes have this; not read by
// VS Code today, defined for safety.
if (!('contextId' in process)) {
	Object.defineProperty(process, 'contextId', {
		value: 'vstauri-sidecar',
		configurable: false,
	});
}

// ---------------------------------------------------------------------------
// Stdin reader (async stream — never blocks the Node event loop)
// ---------------------------------------------------------------------------

function handleCtrl(value) {
	const kind = value && value.t;
	if (kind === 'port') {
		const portId = Number(value.portId);
		let port = ports.get(portId);
		if (!port) {
			port = new MessagePortMainImpl(portId);
			ports.set(portId, port);
		}
		const event = {
			data: value.data === undefined ? null : value.data,
			ports: [port],
		};
		parentPort.emit('message', event);
	} else if (kind === 'port-close') {
		const portId = Number(value.portId);
		const port = ports.get(portId);
		if (port) {
			ports.delete(portId);
			port._closed = true;
			port.emit('close');
		}
	}
}

function dispatchPortMessage(portId, messageBytes) {
	let port = ports.get(portId);
	if (!port) {
		// A message can race port creation in theory; create on demand.
		port = new MessagePortMainImpl(portId);
		ports.set(portId, port);
	}
	port._dispatch(messageBytes);
}

let pending = Buffer.alloc(0);

function drainFrames() {
	for (;;) {
		if (pending.length < 5) {
			return;
		}
		const bodyLen = pending.readUInt32LE(0);
		if (pending.length < 4 + bodyLen) {
			return;
		}
		const type = pending.readUInt8(4);
		const payload = pending.subarray(5, 4 + bodyLen);
		pending = pending.subarray(4 + bodyLen);
		if (type === 1) {
			if (payload.length >= 8) {
				const portId = Number(payload.readBigUInt64LE(0));
				dispatchPortMessage(portId, payload.subarray(8));
			}
		} else if (type === 2) {
			try {
				handleCtrl(JSON.parse(payload.toString('utf8')));
			} catch (error) {
				writeCtrl({ t: 'stderr', data: '[vstauri-sidecar] bad ctrl frame: ' + String(error) + '\n' });
			}
		}
	}
}

process.stdin.on('data', (chunk) => {
	pending = Buffer.concat([pending, chunk]);
	drainFrames();
});
process.stdin.on('close', () => process.exit(0));
process.stdin.on('end', () => process.exit(0));
process.stdin.on('error', () => process.exit(0));
process.stdin.resume();

// ---------------------------------------------------------------------------
// stdout/stderr capture — the Rust shell reads structured frames, not raw
// text, so console output must be forwarded as CTRL frames instead of
// corrupting the framing channel.
// ---------------------------------------------------------------------------

function captureStream(streamName, severity) {
	const stream = process[streamName];
	/** @type {string} */
	let buffer = '';
	stream.write = function (chunk, encoding, callback) {
		const text = typeof chunk === 'string' ? chunk : Buffer.from(chunk).toString('utf8');
		buffer += text;
		let eol = buffer.length > 1024 * 1024 ? buffer.length : buffer.lastIndexOf('\n');
		if (eol !== -1) {
			const emit = buffer.slice(0, eol + 1);
			buffer = buffer.slice(eol + 1);
			writeCtrl({ t: severity, data: emit });
		}
		// Keep the raw bytes OFF the real pipe: stdout IS the frame channel.
		if (typeof callback === 'function') {
			process.nextTick(callback);
		}
		return true;
	};
}
captureStream('stdout', 'stdout');
captureStream('stderr', 'stderr');

// ---------------------------------------------------------------------------
// Terminate when the parent dies (bootstrap-fork parity; it also installs
// this via VSCODE_PARENT_PID, but we belt-and-brace it before import).
// ---------------------------------------------------------------------------

const parentPid = Number(process.env.VSCODE_PARENT_PID || 0);
if (parentPid > 0) {
	setInterval(() => {
		try {
			process.kill(parentPid, 0);
		} catch {
			process.exit(0);
		}
	}, 5000);
}

// ---------------------------------------------------------------------------
// Hand off to the original bootstrap-fork (the same file Electron's utility
// processes run), which imports the entry module from VSCODE_ESM_ENTRYPOINT.
// ---------------------------------------------------------------------------

const here = path.dirname(fileURLToPath(import.meta.url));
const bootstrapFork = path.join(here, 'out', 'bootstrap-fork.js');

writeCtrl({ t: 'stdout', data: '[vstauri-sidecar] starting ' + (process.env.VSCODE_ESM_ENTRYPOINT || '<unknown>') + '\n' });

try {
	await import('file://' + bootstrapFork.replace(/\\/g, '/'));
} catch (error) {
	writeCtrl({ t: 'stderr', data: '[vstauri-sidecar] entry failed: ' + (error && error.stack ? error.stack : String(error)) + '\n' });
	process.exit(1);
}
