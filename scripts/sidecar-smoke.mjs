#!/usr/bin/env node
/*
 * VSTauri sidecar boot smoke test.
 *
 * Boots the real sidecar wrapper (vstauri-sidecar.mjs) with the entry
 * modules the Rust shell spawns (file watcher, extension host, pty host)
 * and asserts each one:
 *   1. does not exit within the observation window, and
 *   2. never emits the "[vstauri-sidecar] entry failed" stderr frame.
 *
 * This runs in CI right after the client bundle is assembled, so missing
 * runtime pieces (package.json at the client root, node_modules the bundle
 * keeps external, native ABI mismatches) fail the build instead of the
 * user's first launch.
 *
 * Usage:
 *   node scripts/sidecar-smoke.mjs <clientRoot> [watch|exthost|ptyhost|all]
 *
 * The wrapper stays alive waiting for the MessagePort handshake that the
 * Rust parent normally drives, so "still running after N seconds" is the
 * pass condition.
 */

import { spawn } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const clientRoot = path.resolve(process.argv[2] || '.');
const which = process.argv[3] || 'all';
const OBSERVE_MS = 15000;

const wrapper = path.join(clientRoot, 'vstauri-sidecar.mjs');
if (!fs.existsSync(wrapper)) {
	console.error(`sidecar-smoke: wrapper not found: ${wrapper}`);
	process.exit(1);
}
if (!fs.existsSync(path.join(clientRoot, 'package.json'))) {
	console.error('sidecar-smoke: client bundle is missing package.json (bootstrap-meta.ts requires ../package.json)');
	process.exit(1);
}
if (!fs.existsSync(path.join(clientRoot, 'out', 'bootstrap-fork.js'))) {
	console.error('sidecar-smoke: out/bootstrap-fork.js missing from client bundle');
	process.exit(1);
}

const CASES = {
	watch: {
		entry: 'vs/platform/files/node/watcher/watcherMain',
		env: {},
	},
	exthost: {
		entry: 'vs/workbench/api/node/extensionHostProcess',
		// Parity with what the renderer's IStartParams carry (see
		// sidecar_channel.rs start(): the env mixin forwards VSCODE_* keys).
		env: {
			VSCODE_WILL_SEND_MESSAGE_PORT: '1',
			VSCODE_HANDLES_UNCAUGHT_ERRORS: '1',
		},
	},
	ptyhost: {
		entry: 'vs/platform/terminal/node/ptyHostMain',
		env: {
			VSCODE_HANDLES_UNCAUGHT_ERRORS: '1',
		},
	},
};

/** Parse the length-prefixed ctrl frames the wrapper writes to stdout. */
function startCase(name, spec) {
	return new Promise((resolve) => {
		const child = spawn(process.execPath, [wrapper], {
			cwd: clientRoot,
			env: {
				...process.env,
				VSCODE_ESM_ENTRYPOINT: spec.entry,
				VSCODE_SIDECAR_TRANSPORT: 'stdio',
				VSCODE_PIPE_LOGGING: 'false',
				...spec.env,
			},
			stdio: ['pipe', 'pipe', 'pipe'],
		});

		let failed = false;
		let buffer = Buffer.alloc(0);
		const notes = [];

		const verdict = (ok, why) => {
			if (failed) {
				return;
			}
			failed = true;
			clearTimeout(timer);
			try {
				child.kill();
			} catch {
				/* already gone */
			}
			resolve({ name, ok, why, notes });
		};

		const timer = setTimeout(() => {
			// Still alive after the full window with no entry failure: pass.
			verdict(true, 'alive after observation window');
		}, OBSERVE_MS);

		child.stdout.on('data', (chunk) => {
			buffer = Buffer.concat([buffer, chunk]);
			for (;;) {
				if (buffer.length < 5) {
					return;
				}
				const bodyLen = buffer.readUInt32LE(0);
				if (buffer.length < 4 + bodyLen) {
					return;
				}
				const type = buffer.readUInt8(4);
				const payload = buffer.subarray(5, 4 + bodyLen);
				buffer = buffer.subarray(4 + bodyLen);
				if (type === 2) {
					try {
						const ctrl = JSON.parse(payload.toString('utf8'));
						notes.push(ctrl);
						if (ctrl.t === 'stderr' && /entry failed|Cannot find module|SyntaxError/i.test(String(ctrl.data))) {
							verdict(false, String(ctrl.data).trim().split('\n')[0]);
						}
					} catch {
						/* ignore malformed */
					}
				}
			}
		});

		child.stderr.on('data', (chunk) => {
			const text = String(chunk);
			notes.push({ t: 'raw-stderr', data: text });
			if (/entry failed|Cannot find module/i.test(text)) {
				verdict(false, text.trim().split('\n')[0]);
			}
		});

		child.on('exit', (code, signal) => {
			verdict(false, `exited early (code ${code}, signal ${signal}) — the entry crashed during import`);
		});
		child.on('error', (err) => {
			verdict(false, `spawn error: ${err}`);
		});
	});
}

const names = which === 'all' ? Object.keys(CASES) : [which];
if (!names.every((n) => CASES[n])) {
	console.error(`sidecar-smoke: unknown case(s): ${names.join(', ')}`);
	process.exit(1);
}

console.log(`sidecar-smoke: clientRoot=${clientRoot} node=${process.version} cases=${names.join(',')}`);

let allOk = true;
for (const name of names) {
	const result = await startCase(name, CASES[name]);
	const head = result.ok ? 'PASS' : 'FAIL';
	console.log(`sidecar-smoke: ${head} ${name}: ${result.why}`);
	if (!result.ok) {
		allOk = false;
		for (const note of result.notes.slice(0, 8)) {
			console.log(`  [${note.t}] ${String(note.data).trim().slice(0, 500)}`);
		}
	}
}
process.exit(allOk ? 0 : 1);
