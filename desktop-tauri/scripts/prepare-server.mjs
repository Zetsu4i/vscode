/*---------------------------------------------------------------------------------------------
 *  Copyright (c) Microsoft Corporation. All rights reserved.
 *  Licensed under the MIT License. See License.txt in the project root for license information.
 *--------------------------------------------------------------------------------------------*/

// Builds the VS Code server (vscode-reh-web) for the current platform and stages it
// into desktop-tauri/server-dist so `tauri build` can bundle it as a resource.
//
// Usage:
//   node scripts/prepare-server.mjs            # minified production build
//   node scripts/prepare-server.mjs --no-min   # unminified (faster, for debugging)
//   node scripts/prepare-server.mjs --skip-gulp  # only restage an existing build

import { execSync } from 'node:child_process';
import { cpSync, existsSync, rmSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, '..', '..');
const stagingDir = resolve(__dirname, '..', 'server-dist');

const platformMap = { linux: 'linux', darwin: 'darwin', win32: 'win32' };
const archMap = { x64: 'x64', arm64: 'arm64' };

const platform = platformMap[process.platform];
const arch = archMap[process.arch];
if (!platform || !arch) {
	console.error(`Unsupported platform/arch: ${process.platform}/${process.arch}`);
	process.exit(1);
}

const min = !process.argv.includes('--no-min');
const skipGulp = process.argv.includes('--skip-gulp');
const gulpTask = `vscode-reh-web-${platform}-${arch}${min ? '-min' : ''}`;
// gulpfile.reh.ts writes the package next to the repository checkout.
const buildOutput = resolve(repoRoot, '..', `vscode-reh-web-${platform}-${arch}`);

if (!skipGulp) {
	console.log(`> Building VS Code server: npm run gulp ${gulpTask}`);
	execSync(`npm run gulp ${gulpTask}`, {
		cwd: repoRoot,
		stdio: 'inherit',
		env: { ...process.env, NODE_OPTIONS: process.env.NODE_OPTIONS ?? '--max-old-space-size=8192' }
	});
}

if (!existsSync(buildOutput)) {
	console.error(`Expected server build not found at ${buildOutput}`);
	process.exit(1);
}

console.log(`> Staging ${buildOutput} -> ${stagingDir}`);
rmSync(stagingDir, { recursive: true, force: true });
mkdirSync(stagingDir, { recursive: true });
cpSync(buildOutput, stagingDir, { recursive: true, verbatimSymlinks: true });

console.log('> Server staged. Next: npm run build (or npm run dev) in desktop-tauri/.');
