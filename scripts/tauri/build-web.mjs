#!/usr/bin/env node
/**
 * tauri: Phase 1 harness — build the genuine VS Code web workbench and place it
 * where the Tauri shell serves it from (out/vscode-web, per tauri.conf.json).
 *
 * Steps:
 *  1. `npm run gulp vscode-web-ci` — upstream task (build/gulpfile.vscode.web.ts):
 *     codicons + built-in web extensions + esbuild bundle straight from src/
 *     + packaging into a deployable static site at <repo-parent>/vscode-web.
 *  2. Copy that site into <repo>/out/vscode-web for the Tauri shell to serve.
 *  3. Render the workbench bootstrap (out/vs/code/browser/workbench/workbench.html)
 *     exactly like the upstream node server does
 *     (src/vs/server/node/webClientServer.ts `renderWorkbenchTemplate`) so the
 *     site boots from any static server — including Tauri's asset protocol.
 *
 * The Electron app is untouched by any of this.
 * Usage: node scripts/tauri/build-web.mjs [--skip-gulp]
 */
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const PACKAGED_SITE = path.resolve(REPO_ROOT, '..', 'vscode-web'); // upstream packageTask destination (BUILD_ROOT)
const OUT_SITE = path.join(REPO_ROOT, 'out', 'vscode-web'); // what tauri.conf.json serves
const SKIP_GULP = process.argv.includes('--skip-gulp');

// --- workbench.html bootstrap rendering (mirrors upstream webClientServer.ts) ---

// Mirror of src/vs/base/common/strings.ts `htmlAttributeEncodeValue`.
function htmlAttributeEncodeValue(value) {
	return value.replace(/[<>"'&]/g, ch => {
		switch (ch) {
			case '<': return '&lt;';
			case '>': return '&gt;';
			case '"': return '&quot;';
			case '\'': return '&apos;';
			case '&': return '&amp;';
		}
		return ch;
	});
}

// Mirror of src/vs/server/node/webClientServer.ts `renderWorkbenchTemplate`.
function renderWorkbenchTemplate(template, values) {
	return template.replace(/\{\{([^}]+)\}\}/g, (_, key) => htmlAttributeEncodeValue(values[key] ?? 'undefined'));
}

// Mirror of src/vs/server/node/webClientServer.ts `createScriptNonce`.
function createScriptNonce() {
	return crypto.randomBytes(16).toString('base64url');
}

// Renders the bootstrapped workbench page for a *statically served* site
// (no remote connection): same fields the upstream server sends for a local,
// no-remote session, with `undefined` fields dropped by JSON.stringify exactly
// like upstream's asJSON().
function renderWorkbenchHtml(siteRoot) {
	const templatePath = path.join(siteRoot, 'out', 'vs', 'code', 'browser', 'workbench', 'workbench.html');
	const template = fs.readFileSync(templatePath, 'utf8');

	const product = JSON.parse(fs.readFileSync(path.join(REPO_ROOT, 'product.json'), 'utf8'));

	// Static site layout: base URL '' makes the boot script resolve
	// `_VSCODE_FILE_ROOT = <origin>/out/` — matching this site's layout.
	const baseUrl = '';
	const workbenchWebConfiguration = {
		serverBasePath: '',
		developmentOptions: {},
		enableWorkspaceTrust: true,
		productConfiguration: product,
		callbackRoute: `${baseUrl}/out/vs/code/browser/workbench/callback.html`,
	};

	const values = {
		WORKBENCH_WEB_CONFIGURATION: JSON.stringify(workbenchWebConfiguration),
		WORKBENCH_AUTH_SESSION: '', // no auth session (Phase 1)
		WORKBENCH_WEB_BASE_URL: baseUrl,
		WORKBENCH_NLS_URL: '', // english fallback will apply
		WORKBENCH_NLS_FALLBACK_URL: `${baseUrl}/out/nls.messages.js`,
		WORKBENCH_SCRIPT_NONCE: createScriptNonce(),
	};

	fs.writeFileSync(templatePath, renderWorkbenchTemplate(template, values));
}

// Bundles the browser-shell loader (`vs/code/browser/workbench/workbench.ts`)
// which the `web` build target deliberately omits ("web workbench only (no
// browser shell)" — see getEntryPointsForTarget in build/next/index.ts):
// the shell loader is only included by the `server-web` target. A statically
// served site needs it, so we bundle this single upstream entry point with the
// exact options used by the upstream bundler (build/next/index.ts `bundle()`).
async function bundleBrowserShellLoader(siteRoot) {
	const require = createRequire(path.join(REPO_ROOT, 'build', 'package.json'));
	const esbuild = require('esbuild');

	const entry = path.join(REPO_ROOT, 'src', 'vs', 'code', 'browser', 'workbench', 'workbench.ts');
	const outfile = path.join(siteRoot, 'out', 'vs', 'code', 'browser', 'workbench', 'workbench.js');

	// Same banner as upstream bundle()
	const tslib = fs.readFileSync(path.join(REPO_ROOT, 'build', 'node_modules', 'tslib', 'tslib.es6.js'), 'utf-8');
	const banner = {
		js: `/*!--------------------------------------------------------
 * Copyright (C) Microsoft Corporation. All rights reserved.
 *--------------------------------------------------------*/
${tslib}`,
	};

	await esbuild.build({
		entryPoints: [entry],
		outfile,
		bundle: true,
		format: 'esm',
		platform: 'neutral',
		target: ['es2024'],
		packages: 'external',
		sourcemap: 'linked',
		sourcesContent: true,
		minify: false,
		treeShaking: true,
		banner,
		loader: {
			'.ttf': 'file',
			'.svg': 'file',
			'.png': 'file',
			'.sh': 'file',
		},
		assetNames: 'media/[name]',
		logLevel: 'warning',
		logOverride: { 'unsupported-require-call': 'silent' },
		tsconfigRaw: JSON.stringify({
			compilerOptions: {
				experimentalDecorators: true,
				useDefineForClassFields: false,
			},
		}),
	});
}

function fail(msg) {
	console.error(`\n[tauri:web] ERROR: ${msg}`);
	process.exit(1);
}

function copyDir(src, dest) {
	fs.rmSync(dest, { recursive: true, force: true });
	fs.mkdirSync(path.dirname(dest), { recursive: true });
	fs.cpSync(src, dest, { recursive: true });
}

function dirStats(dir) {
	let files = 0;
	let bytes = 0;
	const walk = d => {
		for (const e of fs.readdirSync(d, { withFileTypes: true })) {
			const p = path.join(d, e.name);
			if (e.isDirectory()) walk(p);
			else {
				files++;
				bytes += fs.statSync(p).size;
			}
		}
	};
	walk(dir);
	return { files, mb: (bytes / (1024 * 1024)).toFixed(1) };
}

if (!fs.existsSync(path.join(REPO_ROOT, 'node_modules', 'gulp'))) {
	fail('dependencies are not installed — run `npm ci` at the repo root first.');
}

if (!SKIP_GULP) {
	console.log('[tauri:web] running upstream web bundle task: gulp vscode-web-ci (this bundles the workbench from src/ with esbuild and packages the static site)...');
	const res = spawnSync('npm', ['run', '--silent', 'gulp', 'vscode-web-ci'], {
		cwd: REPO_ROOT,
		stdio: 'inherit',
		shell: process.platform === 'win32',
	});
	if (res.status !== 0) {
		fail('gulp vscode-web-ci failed — see output above.');
	}
}

if (!fs.existsSync(path.join(PACKAGED_SITE, 'out'))) {
	fail(`packaged site not found at ${PACKAGED_SITE} — did the gulp task run? (use --skip-gulp only when it already exists)`);
}

console.log(`[tauri:web] copying packaged site ${PACKAGED_SITE} -> ${OUT_SITE} ...`);
copyDir(PACKAGED_SITE, OUT_SITE);

// Copy server-brand resources so the template's `{{BASE}}/resources/server/*`
// links resolve on a static server (upstream maps these server-side).
const serverResources = path.join(REPO_ROOT, 'resources', 'server');
if (fs.existsSync(serverResources)) {
	copyDir(serverResources, path.join(OUT_SITE, 'resources', 'server'));
}

// Render the workbench bootstrap for static serving (mirrors upstream
// webClientServer.ts template substitution — see function above).
console.log('[tauri:web] rendering workbench bootstrap for static serving...');
renderWorkbenchHtml(OUT_SITE);

// Bundle the upstream browser-shell loader that the static site needs
// (see bundleBrowserShellLoader above).
console.log('[tauri:web] bundling browser-shell loader (vs/code/browser/workbench/workbench)...');
await bundleBrowserShellLoader(OUT_SITE);

// Convenience root page for plain-browser previews (the Tauri window opens the
// workbench html directly; this only helps humans poking at the folder).
fs.writeFileSync(
	path.join(OUT_SITE, 'index.html'),
	'<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0;url=out/vs/code/browser/workbench/workbench.html"><title>Code - Tauri</title>'
);

const stats = dirStats(OUT_SITE);
console.log(`[tauri:web] done: ${stats.files} files, ${stats.mb} MB in out/vscode-web`);
console.log('[tauri:web] the Tauri shell (src-tauri) serves this directory as the workbench frontend.');
console.log('[tauri:web] next: `npm run tauri:dev` (requires the Rust toolchain + webview deps).');
