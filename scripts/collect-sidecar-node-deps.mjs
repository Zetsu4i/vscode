#!/usr/bin/env node
/*
 * VSTauri sidecar Node dependency scanner.
 *
 * The Tauri shell runs the Electron "utility process" roles (extension host,
 * file watcher, pty host, shared process, agent host, ...) as plain Node.js
 * child processes through out/bootstrap-fork.js. Unlike Electron, plain Node
 * does NOT understand node_modules.asar, so every bare module specifier those
 * entries import (or require) at runtime must exist as a REAL directory under
 * <clientRoot>/node_modules/.
 *
 * The product bundle (build/next, esbuild `packages: 'external'`) keeps those
 * imports external — they appear as string literals in the emitted bundles.
 * This script scans the bundled entry files for bare specifiers, resolves
 * them against the repo's node_modules, then walks each package's own files
 * to collect the transitive closure. Output: one package path (relative to
 * node_modules/) per line on stdout, for the CI staging copy loop.
 *
 * Usage:
 *   node scripts/collect-sidecar-node-deps.mjs [clientRoot]
 *
 * clientRoot defaults to the repo root (expects node_modules/ + out-client/).
 */

import { createRequire } from 'node:module';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const clientRoot = path.resolve(process.argv[2] || repoRoot);
const nodeModulesRoot = path.join(clientRoot, 'node_modules');
const outRoot = path.join(clientRoot, process.env.VSTAURI_SCAN_OUT || 'out-client');

if (!fs.existsSync(nodeModulesRoot)) {
        console.error(`collect-sidecar-node-deps: ${nodeModulesRoot} not found (run after npm ci)`);
        process.exit(1);
}

// Every entry the Rust sidecar manager can spawn (sidecar_channel.rs) plus
// bootstrap-fork.js itself. Paths are inside the bundled out tree.
const SIDECAR_ENTRIES = [
        'bootstrap-fork.js',
        'vs/workbench/api/node/extensionHostProcess.js',
        'vs/platform/files/node/watcher/watcherMain.js',
        'vs/platform/terminal/node/ptyHostMain.js',
        'vs/code/electron-utility/sharedProcess/sharedProcessMain.js',
        'vs/platform/agentHost/node/agentHostMain.js',
        'vs/platform/agentHost/node/diffWorkerMain.js',
        'vs/platform/localTranscription/node/localTranscriptionMain.js',
        'vs/workbench/contrib/debug/node/telemetryApp.js',
];

// Hard safety net: packages the bundles reach through dynamic/computed
// require() (try/catch guarded imports the scanner cannot see statically)
// plus native modules whose absence only shows up at runtime. Unioned with
// the scan results; never removed.
const ALWAYS_INCLUDE = [
        '@parcel/watcher',
        '@vscode/native-watchdog',
        '@vscode/proxy-agent',
        '@vscode/spdlog',
        '@vscode/sqlite3',
        '@vscode/windows-registry',
        '@vscode/windows-process-tree',
        '@vscode/policy-watcher',
        '@vscode/ripgrep-universal',
        '@vscode/fs-copyfile',
        'native-keymap',
        'native-is-elevated',
        'node-pty',
        'minimist',
];

// Heavy optional packages that are dynamically imported inside guarded
// paths (agent/remote features with graceful degradation when missing).
// Shipping them would balloon the installer for features the sidecars can
// report as unavailable instead. The copilot EXTENSION ships its own copy
// under extensions/copilot (with its pruned node_modules), so the root
// @github/copilot prebuilds are never needed by the sidecar set.
const EXCLUDE = new Set([
        '@devcontainers/cli',
        '@github/copilot',
        'playwright-core',
]);

function isExcluded(spec) {
        for (const name of EXCLUDE) {
                if (spec === name || spec.startsWith(`${name}/`)) {
                        return true;
                }
        }
        return spec.startsWith('@types/');
}

const MAX_FILES_PER_PACKAGE = 4000;

/** Extract bare module specifiers from a JS-ish file's text. */
function extractSpecifiers(text) {
        const found = new Set();

        // 1) Explicit call positions: require('x') / import('x')
        const callRe = /(?:\brequire\s*\(\s*|\bimport\s*\(\s*)['"]([^'"]+)['"]\s*\)/g;
        // 2) Import statements: from'x' / from "x" / import 'x'
        const fromRe = /(?:\bfrom\s*|\bimport\s+)['"]([^'"]+)['"]/g;

        for (const re of [callRe, fromRe]) {
                let m;
                while ((m = re.exec(text)) !== null) {
                        found.add(m[1]);
                }
        }

        // 3) Safety net: ANY quoted literal that looks like a bare specifier and
        //    actually resolves. esbuild's minified output writes
        //    `import{a}from"x"` with no spaces, which tier 1/2 can miss. False
        //    positives only cost installer size, false negatives break sidecars
        //    on the user's machine.
        const anyStringRe = /['"](@[a-z0-9-~]+\/[a-z0-9-~][a-z0-9-._~]*|[a-z0-9-~][a-z0-9-._~]*)['"]/g;
        let m;
        while ((m = anyStringRe.exec(text)) !== null) {
                found.add(m[1]);
        }

        return [...found].filter((spec) => {
                if (!spec || spec.startsWith('.') || spec.startsWith('/')) {
                        return false;
                }
                if (spec.startsWith('node:') || spec === 'electron' || spec.startsWith('electron/')) {
                        return false;
                }
                if (isExcluded(spec)) {
                        return false;
                }
                try {
                        // URL-canonical specifiers (data:, http:, file:) are not packages.
                        new URL(spec);
                        return false;
                } catch {
                        /* not a URL — fine */
                }
                return true;
        });
}

/** Map a resolved file path to its owning package root under node_modules. */
function packageRootOf(resolvedFile) {
        let dir = path.dirname(resolvedFile);
        for (;;) {
                const pkgJson = path.join(dir, 'package.json');
                if (fs.existsSync(pkgJson)) {
                        try {
                                const pkg = JSON.parse(fs.readFileSync(pkgJson, 'utf8'));
                                if (typeof pkg.name === 'string' && pkg.name !== '') {
                                        return dir;
                                }
                        } catch {
                                /* malformed — keep walking */
                        }
                }
                const parent = path.dirname(dir);
                if (parent === dir) {
                        return undefined;
                }
                if (dir === nodeModulesRoot) {
                        // A file directly under node_modules/<pkg> with no package.json?
                        // Not a real package; skip.
                        return undefined;
                }
                dir = parent;
        }
}

const seenPackages = new Map(); // packageRoot -> { name, entrySpecifiers:Set }
const queue = []; // { file, spec }

function enqueueEntryFile(file) {
        if (!fs.existsSync(file)) {
                console.error(`collect-sidecar-node-deps: entry missing (skipping): ${path.relative(clientRoot, file)}`);
                return;
        }
        queue.push({ file, spec: null });
}

for (const entry of SIDECAR_ENTRIES) {
        enqueueEntryFile(path.join(outRoot, entry));
}

function scanPackage(packageRoot, name, firstSpecifier) {
        if (seenPackages.has(packageRoot)) {
                seenPackages.get(packageRoot).specifiers.add(firstSpecifier);
                return;
        }
        seenPackages.set(packageRoot, { name, specifiers: new Set([firstSpecifier]) });

        // Walk the package's own files (skip nested node_modules — those are
        // separate packages discovered via resolution below).
        const files = [];
        const walk = (dir, budget) => {
                let entries;
                try {
                        entries = fs.readdirSync(dir, { withFileTypes: true });
                } catch {
                        return budget;
                }
                for (const entry of entries) {
                        if (budget <= 0) {
                                return budget;
                        }
                        if (entry.name === 'node_modules' || entry.name.startsWith('.')) {
                                continue;
                        }
                        const full = path.join(dir, entry.name);
                        if (entry.isDirectory()) {
                                budget = walk(full, budget);
                        } else if (/\.(?:js|cjs|mjs)$/.test(entry.name)) {
                                files.push(full);
                                budget -= 1;
                        }
                }
                return budget;
        };
        walk(packageRoot, MAX_FILES_PER_PACKAGE);

        for (const file of files) {
                let text;
                try {
                        text = fs.readFileSync(file, 'utf8');
                } catch {
                        continue;
                }
                for (const spec of extractSpecifiers(text)) {
                        queue.push({ file, spec });
                }
        }
}

// BFS over entry files and package files.
let guard = 0;
while (queue.length > 0 && guard < 50000) {
        guard += 1;
        const { file, spec } = queue.shift();

        if (spec === null) {
                // An out/ entry file: scan its specifiers, no package registration.
                let text;
                try {
                        text = fs.readFileSync(file, 'utf8');
                } catch {
                        continue;
                }
                for (const s of extractSpecifiers(text)) {
                        queue.push({ file, spec: s });
                }
                continue;
        }

        // Resolve the specifier from the file that referenced it, then register
        // the owning package and scan it.
        const require = createRequire(pathToFileURL(file).href);
        let resolved;
        try {
                resolved = require.resolve(spec);
        } catch {
                continue; // not installed / builtin-ish — not shippable anyway
        }

        // Only ship packages from the install we are staging.
        const resolvedNorm = path.resolve(resolved);
        if (!resolvedNorm.startsWith(nodeModulesRoot + path.sep)) {
                continue;
        }

        const packageRoot = packageRootOf(resolvedNorm);
        if (!packageRoot) {
                continue;
        }
        const rel = path.relative(nodeModulesRoot, packageRoot).replace(/\\/g, '/');
        let name = rel;
        try {
                name = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8')).name || rel;
        } catch {
                /* keep dir-based name */
        }
        scanPackage(packageRoot, name, spec);
}

// Union with the hard safety list.
for (const name of ALWAYS_INCLUDE) {
        const packageRoot = path.join(nodeModulesRoot, ...name.split('/'));
        if (!fs.existsSync(path.join(packageRoot, 'package.json'))) {
                console.error(`collect-sidecar-node-deps: WARN: safety package not installed: ${name}`);
                continue;
        }
        if (!seenPackages.has(packageRoot)) {
                scanPackage(packageRoot, name, '<always>');
        }
}

// Emit: package paths relative to node_modules (posix form), sorted.
const out = [...seenPackages.keys()]
        .map((root) => path.relative(nodeModulesRoot, root).replace(/\\/g, '/'))
        .filter((rel) => rel && !rel.startsWith('..'))
        .sort();
for (const rel of out) {
        console.log(rel);
}
console.error(`collect-sidecar-node-deps: ${out.length} packages (${guard} scan steps)`);
