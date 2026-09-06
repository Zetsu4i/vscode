#!/usr/bin/env node
/*---------------------------------------------------------------------------------------------
 *  VSTauri IPC contract extractor (ROADMAP.md Phase 2, "Wind & Mountain").
 *
 *  Scans the pristine upstream VS Code source tree and catalogs every IPC
 *  surface the Wind shim / Mountain backend must answer:
 *
 *    1. Plain `vscode:` ipcRenderer channels
 *       (validatedIpcMain / ipcMain / ipcRenderer / webContents.send)
 *
 *    2. Main-process message protocol channels
 *       (registerChannel(...) server side, getChannel(...) client side)
 *       including the ProxyChannel services whose command surface equals
 *       the public methods of the wrapped service interface.
 *
 *  Outputs:
 *    compat/ipc-contract.json  machine-readable contract (checked in)
 *    compat/ipc-contract.md    human-readable summary + Mountain coverage
 *
 *  Modes:
 *    (default)  extract + write both files
 *    --check    re-scan and diff against the checked-in JSON; exit 1 on
 *               drift (the CI tripwire from the Minutes-to-Update section
 *               of ROADMAP.md)
 *    --summary  print the summary to stdout without writing files
 *
 *  Only stdlib is used on purpose: the extractor must run before
 *  `npm install` in CI and inside plain `node` checkouts.
 *--------------------------------------------------------------------------------------------*/

import fs from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(new URL('.', import.meta.url).pathname, '../..');
const SRC = path.join(ROOT, 'src', 'vs');
const OUT_JSON = path.join(ROOT, 'compat', 'ipc-contract.json');
const OUT_MD = path.join(ROOT, 'compat', 'ipc-contract.md');

// Directories that are not part of the shipping IPC surface (test scaffolding,
// fixtures, examples). They are scanned but entries get tagged so the contract
// can separate product surface from test-only surface.
const TEST_DIR_RE = /(^|\/)(test|tests|fixtures|fixture|sandbox\.|samples?)(\/|$)/i;

function walk(dir, files = [], filter = /\.(ts|js)$/) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith('.') || entry.name === 'node_modules') {
      continue;
    }
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, files, filter);
    } else if (filter.test(entry.name) && !/\.d\.ts$/.test(entry.name)) {
      files.push(full);
    }
  }
  return files;
}

const rel = (file) => path.relative(ROOT, file).replaceAll('\\', '/');

function sideOf(file) {
  const f = rel(file);
  if (/\/electron-main\//.test(f)) return 'electron-main';
  if (/\/sharedProcess\//.test(f)) return 'shared-process';
  if (/\/electron-browser\//.test(f)) return 'renderer';
  if (/\/node\//.test(f)) return 'node-main';
  if (/\/browser\//.test(f)) return 'renderer';
  if (/\/common\//.test(f)) return 'common';
  return 'other';
}

const isTestFile = (file) => TEST_DIR_RE.test(rel(file));

// ---------------------------------------------------------------------------
// Channel-name constants used in registerChannel/getChannel calls, resolved
// statically (name -> literal). Only the handful that upstream defines.
// ---------------------------------------------------------------------------

function collectChannelConstants() {
  const constants = new Map();
  const defs = [
    ['src/vs/platform/files/common/diskFileSystemProviderClient.ts', 'LOCAL_FILE_SYSTEM_CHANNEL_NAME'],
    ['src/vs/platform/meteredConnection/common/meteredConnectionIpc.ts', 'METERED_CONNECTION_CHANNEL'],
    ['src/vs/platform/browserView/common/browserView.ts', 'ipcBrowserViewChannelName'],
    ['src/vs/platform/browserView/common/browserViewGroup.ts', 'ipcBrowserViewGroupChannelName'],
  ];
  for (const [file, name] of defs) {
    try {
      const text = fs.readFileSync(path.join(ROOT, file), 'utf8');
      const m = text.match(new RegExp(`export\\s+const\\s+${name}\\s*=\\s*['"]([^'"]+)['"]`));
      if (m) {
        constants.set(name, { value: m[1], file: rel(path.join(ROOT, file)) });
      }
    } catch { /* optional file */ }
  }
  return constants;
}

const CHANNEL_CONSTANTS = collectChannelConstants();

function resolveChannelName(rawName) {
  if (!rawName) {
    return { name: null, constant: null };
  }
  if (/^['"].*['"]$/.test(rawName)) {
    return { name: rawName.slice(1, -1), constant: null };
  }
  const hit = CHANNEL_CONSTANTS.get(rawName);
  if (hit) {
    return { name: hit.value, constant: rawName };
  }
  return { name: null, constant: null };
}

// ---------------------------------------------------------------------------
// Regex scans. Kept intentionally conservative: only calls with a literal
// (or known constant) first argument are captured — dynamic channel names are
// reported as `dynamic` so nothing silently disappears from the contract.
// ---------------------------------------------------------------------------

const RE_VALIDATED = /(?:^|[^\w$.])(validatedIpcMain|ipcMain)\s*\.\s*(handle|on|once|removeHandler|removeListener)\s*\(\s*([^,)]+)/g;
const RE_RENDERER = /(?:^|[^\w$.])ipcRenderer\s*\.\s*(send|sendSync|invoke|on|once|removeListener|postMessage)\s*\(\s*([^,)]+)/g;
const RE_WC_SEND = /(?:webContents|win\.win|window\.win|this\._win|win)\s*\.\s*send\s*\(\s*([^,)]+)/g; // heuristic: WebContents.send pushes
const RE_REGISTER = /\bregisterChannel\s*\(\s*([^,)]+)/g;
const RE_GET = /\bgetChannel\s*\(\s*([^,)]+)/g;

function scanFile(file, out) {
  const text = fs.readFileSync(file, 'utf8');

  const record = (bucket, name, kind, extra = {}) => {
    if (!name) {
      out.dynamic.push({ file: rel(file), kind, ...extra });
      return;
    }
    const entry = out[bucket].get(name) ?? { name, kinds: new Set(), refs: [] };
    entry.kinds.add(kind);
    entry.refs.push({ file: rel(file), kind, side: sideOf(file), test: isTestFile(file), ...extra });
    out[bucket].set(name, entry);
  };

  for (const m of text.matchAll(RE_VALIDATED)) {
    const { name } = resolveChannelName(m[3]?.trim());
    record('plain', name, `main:${m[2]}`);
  }
  for (const m of text.matchAll(RE_RENDERER)) {
    const { name } = resolveChannelName(m[2]?.trim());
    record('plain', name, `renderer:${m[1]}`);
  }
  for (const m of text.matchAll(RE_WC_SEND)) {
    const raw = m[1].trim();
    const { name } = resolveChannelName(raw);
    record('plain', name, 'main:push');
  }
  for (const m of text.matchAll(RE_REGISTER)) {
    const { name, constant } = resolveChannelName(m[1]?.trim());
    record('protocol', name, 'server:register', constant ? { constant } : {});
  }
  for (const m of text.matchAll(RE_GET)) {
    const { name, constant } = resolveChannelName(m[1]?.trim());
    record('protocol', name, 'client:get', constant ? { constant } : {});
  }
}

// ---------------------------------------------------------------------------
// ProxyChannel service surfaces: the command names of channels created with
// `ProxyChannel.fromService(service)` equal the public methods of the service
// interface. Extract those method lists from the interface definitions.
// ---------------------------------------------------------------------------

const PROXY_SERVICE_FILES = [
  // [channel, interface file, interface name, notes]
  ['nativeHost', 'src/vs/platform/native/common/native.ts', 'INativeHostService', 'window/dialog/clipboard/process surface'],
  ['keyboardLayout', 'src/vs/platform/keyboardLayout/common/keyboardLayoutService.ts', 'INativeKeyboardLayoutService', 'keyboard layout'],
  ['userDataProfiles', 'src/vs/platform/userDataProfile/common/userDataProfile.ts', 'IUserDataProfilesService', 'profile CRUD'],
  ['sign', 'src/vs/platform/sign/common/sign.ts', 'ISignService', 'signing'],
  ['workspaces', 'src/vs/platform/workspaces/common/workspaces.ts', 'IWorkspacesService', 'workspace dialogs'],
  ['menubar', 'src/vs/platform/menubar/common/menubar.ts', 'IMenubarService', 'native menu bar'],
  ['url', 'src/vs/platform/url/common/url.ts', 'IURLService', 'deep links'],
  ['webview', 'src/vs/platform/webview/common/webviewManagerService.ts', 'IWebviewManagerService', 'webview lifecycle'],
  ['update', 'src/vs/platform/update/common/update.ts', 'IUpdateService', 'updates'],
  ['encryption', 'src/vs/platform/encryption/common/encryption.ts', 'IEncryptionService', 'secret storage'],
  ['process', 'src/vs/platform/process/common/process.ts', 'IProcessService', 'process info'],
];

/**
 * Parse the public surface of a TS interface: methods `name(...)`, readonly
 * props (data, not IPC), events `on...: Event<...>` and getters. Follows
 * single-file `extends` chains (e.g. INativeHostService extends
 * ICommonNativeHostService).
 */
function extractInterfaceSurface(file, interfaceName, seen = new Set()) {
  if (seen.has(interfaceName)) {
    return { methods: [], events: [] };
  }
  seen.add(interfaceName);
  const text = fs.readFileSync(path.join(ROOT, file), 'utf8');
  const start = text.indexOf(`interface ${interfaceName}`);
  if (start === -1) {
    return { methods: [], events: [] };
  }
  // find opening brace, then brace-count to the interface end
  let i = text.indexOf('{', start);
  let depth = 0;
  let end = i;
  for (; i < text.length; i++) {
    if (text[i] === '{') depth++;
    else if (text[i] === '}') { depth--; if (depth === 0) { end = i; break; } }
  }
  const header = text.slice(start, i);
  const body = text.slice(start, end);

  const methods = [];
  const events = [];
  const lines = body.split(/\r?\n/);
  for (const line of lines) {
    const t = line.trim();
    if (!t || t.startsWith('//') || t.startsWith('*') || t.startsWith('|')) continue;
    const ev = t.match(/^readonly\s+(on\w+?)\s*:/);
    if (ev) { events.push(ev[1]); continue; }
    const gm = t.match(/^(?:async\s+)?(?:get\s+)?([A-Za-z_][\w$]*)\s*\(/);
    if (gm && !['if', 'for', 'while', 'switch', 'catch', 'constructor', 'function', 'interface', 'readonly', 'return', 'typeof', 'get', 'set'].includes(gm[1])) {
      const name = gm[1];
      if (!methods.includes(name) && !events.includes(name)) {
        methods.push(name);
      }
    }
  }

  // extends chain (same file)
  const ext = header.match(/extends\s+([A-Za-z_][\w$]*)/);
  if (ext) {
    const parent = extractInterfaceSurface(file, ext[1], seen);
    for (const m of parent.methods) {
      if (!methods.includes(m)) methods.push(m);
    }
    for (const e of parent.events) {
      if (!events.includes(e)) events.push(e);
    }
  }
  return { methods, events };
}

// Explicit IServerChannel classes: harvest the `call()` / `listen()` switch
// cases of known channel implementations for their command lists.
const EXPLICIT_CHANNEL_SERVERS = [
  // [channel, server file]
  ['storage', 'src/vs/platform/storage/electron-main/storageIpc.ts'],
  ['logger', 'src/vs/platform/log/common/logIpc.ts'],
  ['userDataProfiles', 'src/vs/platform/userDataProfile/common/userDataProfileIpc.ts'],
  ['policy', 'src/vs/platform/policy/common/policyIpc.ts'],
  ['extensions', 'src/vs/platform/extensionManagement/common/extensionManagementIpc.ts'],
];

function extractSwitchCommands(file) {
  try {
    const text = fs.readFileSync(path.join(ROOT, file), 'utf8');
    const commands = [];
    // case 'command': / case "command":
    for (const m of text.matchAll(/case\s+['"]([A-Za-z_][\w$]*)['"]\s*:/g)) {
      commands.push(m[1]);
    }
    return [...new Set(commands)];
  } catch {
    return [];
  }
}

// ---------------------------------------------------------------------------
// Mountain coverage: which (channel, command) pairs does the Rust backend
// already answer? Parsed from the match arms in src-tauri/src/*.rs and the
// channel modules it routes to.
// ---------------------------------------------------------------------------

function collectMountainCoverage() {
  const implemented = new Map(); // "channel" -> Set<command> (command "*" = whole channel)
  const tauriSrc = path.join(ROOT, 'src-tauri', 'src');
  if (!fs.existsSync(tauriSrc)) {
    return { implemented, implementedChannels: [] };
  }
  const files = walk(tauriSrc, [], /\.rs$/);
  for (const file of files) {
    const text = fs.readFileSync(file, 'utf8');
    // match arms: ("nativeHost", "windowId") => ...  |  ("channel", cmd) => ...
    for (const m of text.matchAll(/\(\s*"([^"]+)"\s*,\s*(?:"([^"]*)"|([a-zA-Z_][\w]*))\s*\)\s*=>/g)) {
      const channel = m[1];
      const command = m[2] ?? m[3] ?? null;
      if (!implemented.has(channel)) {
        implemented.set(channel, new Set());
      }
      if (command && m[2] !== undefined) {
        implemented.get(channel).add(command);
      } else if (command) {
        // variable (e.g. `cmd`) — whole-channel handler in a submodule
        implemented.get(channel).add('*');
      } else {
        implemented.get(channel).add('*');
      }
    }
    // whole-channel delegation: "storage" => storage_channel::handle
    for (const m of text.matchAll(/"([A-Za-z][\w]*)"\s*=>\s*[a-z_]+::/g)) {
      const channel = m[1];
      if (!implemented.has(channel)) {
        implemented.set(channel, new Set(['*']));
      }
    }
  }
  return { implemented, implementedChannels: [...implemented.keys()].sort() };
}

// ---------------------------------------------------------------------------
// Build the contract
// ---------------------------------------------------------------------------

function buildContract() {
  const files = walk(SRC);
  const out = { plain: new Map(), protocol: new Map(), dynamic: [] };
  for (const file of files) {
    try {
      scanFile(file, out);
    } catch (err) {
      console.error(`warn: cannot scan ${rel(file)}: ${err.message}`);
    }
  }

  const productText = fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8');
  const version = JSON.parse(productText).version ?? 'unknown';

  const plain = {};
  for (const [name, entry] of [...out.plain.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    const productRefs = entry.refs.filter((r) => !r.test);
    const isTestOnly = productRefs.length === 0;
    plain[name] = {
      kinds: [...entry.kinds].sort(),
      testOnly: isTestOnly,
      refs: entry.refs.map((r) => ({ file: r.file, kind: r.kind, side: r.side, test: r.test })),
    };
  }

  const { implemented, implementedChannels } = collectMountainCoverage();

  const proxySurfaces = {};
  for (const [channel, file, iface, note] of PROXY_SERVICE_FILES) {
    let surface = { methods: [], events: [], note };
    try {
      surface = { ...extractInterfaceSurface(file, iface), note };
    } catch { /* file missing in this upstream snapshot */ }
    proxySurfaces[channel] = surface;
  }

  const explicitCommands = {};
  for (const [channel, file] of EXPLICIT_CHANNEL_SERVERS) {
    const commands = extractSwitchCommands(file);
    if (commands.length) {
      explicitCommands[channel] = { file: rel(path.join(ROOT, file)), commands };
    }
  }

  const protocol = {};
  for (const [name, entry] of [...out.protocol.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    const productRefs = entry.refs.filter((r) => !r.test);
    const isTestOnly = productRefs.length === 0;
    const servers = entry.refs.filter((r) => r.kind === 'server:register' && !r.test);
    const clients = entry.refs.filter((r) => r.kind === 'client:get' && !r.test);
    const impl = implemented.get(name);
    protocol[name] = {
      registered: servers.length > 0,
      consumed: clients.length > 0,
      testOnly: isTestOnly,
      mountain: impl ? (impl.has('*') ? 'implemented(*)' : `implemented(${impl.size} commands)`) : 'not-implemented',
      refs: entry.refs.map((r) => ({ file: r.file, kind: r.kind, side: r.side, test: r.test, constant: r.constant ?? undefined })),
      ...(proxySurfaces[name] ? { proxyService: proxySurfaces[name] } : {}),
      ...(explicitCommands[name] ? { serverCommands: explicitCommands[name].commands } : {}),
    };
  }

  const dynamic = [...new Map(out.dynamic.map((d) => [`${d.file}:${d.kind}`, d])).values()];

  const productPlain = Object.fromEntries(Object.entries(plain).filter(([, v]) => !v.testOnly));
  const productProtocol = Object.fromEntries(Object.entries(protocol).filter(([, v]) => !v.testOnly && (v.registered || v.consumed)));

  const contract = {
    generatedAt: new Date().toISOString(),
    upstreamVersion: version,
    summary: {
      filesScanned: files.length,
      plainChannelsProduct: Object.keys(productPlain).length,
      plainChannelsTestOnly: Object.keys(plain).length - Object.keys(productPlain).length,
      protocolChannelsProduct: Object.keys(productProtocol).length,
      protocolChannelsTestOnly: Object.keys(protocol).length - Object.keys(productProtocol).length,
      dynamicChannelCalls: dynamic.length,
      mountainChannelsImplemented: implementedChannels.length,
    },
    plainChannels: plain,
    protocolChannels: protocol,
    mountainCoverage: {
      channels: implementedChannels,
      note: 'Parsed from match arms in src-tauri/src/*.rs; "*" = whole-channel handler.',
    },
    dynamicCalls: dynamic,
  };

  return contract;
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

function renderMarkdown(contract) {
  const L = [];
  L.push('# VSTauri IPC Contract');
  L.push('');
  L.push(`Generated from the pristine upstream tree (v${contract.upstreamVersion}) by \`build/ipc-contract/extract-ipc-contract.mjs\`.`);
  L.push('Regenerate with `node build/ipc-contract/extract-ipc-contract.mjs` and commit the result together with any upstream merge.');
  L.push('');
  L.push('## Summary');
  L.push('');
  L.push(`- Files scanned: **${contract.summary.filesScanned}**`);
  L.push(`- Plain \`vscode:\` channels (product surface): **${contract.summary.plainChannelsProduct}** (+${contract.summary.plainChannelsTestOnly} test-only)`);
  L.push(`- Protocol service channels (product surface): **${contract.summary.protocolChannelsProduct}** (+${contract.summary.protocolChannelsTestOnly} test-only)`);
  L.push(`- Mountain channels answered in Rust: **${contract.summary.mountainChannelsImplemented}**`);
  L.push(`- Dynamic channel names (need manual tracing): **${contract.summary.dynamicChannelCalls}**`);
  L.push('');
  L.push('## Plain channels (renderer <-> main, `ipcRenderer` surface bridged by the Wind shim)');
  L.push('');
  L.push('| Channel | Kinds | Test-only | Main handler | Renderer calls |');
  L.push('| --- | --- | --- | --- | --- |');
  for (const [name, entry] of Object.entries(contract.plainChannels)) {
    if (entry.testOnly) continue;
    const mainCount = entry.refs.filter((r) => r.side === 'electron-main' && !r.test).length;
    const rendererCount = entry.refs.filter((r) => r.side === 'renderer' && !r.test).length;
    L.push(`| \`${name}\` | ${entry.kinds.join(', ')} | ${entry.testOnly ? 'yes' : 'no'} | ${mainCount} | ${rendererCount} |`);
  }
  L.push('');
  L.push('## Protocol channels (`vscode:hello` / `vscode:message` binary frames -> Mountain router)');
  L.push('');
  L.push('| Channel | Registered (main) | Consumed (renderer) | Mountain status | Commands known |');
  L.push('| --- | --- | --- | --- | --- |');
  for (const [name, entry] of Object.entries(contract.protocolChannels)) {
    if (entry.testOnly || (!entry.registered && !entry.consumed)) continue;
    const commands = entry.serverCommands?.length
      ? `${entry.serverCommands.length} (server switch)`
      : entry.proxyService?.methods?.length
        ? `${entry.proxyService.methods.length} (ProxyChannel: ${entry.proxyService.methods.slice(0, 6).join(', ')}${entry.proxyService.methods.length > 6 ? ', ...' : ''})`
        : 'unknown (dynamic)';
    L.push(`| \`${name}\` | ${entry.registered ? 'yes' : 'no'} | ${entry.consumed ? 'yes' : 'no'} | ${entry.mountain} | ${commands} |`);
  }
  L.push('');
  L.push('## Mountain coverage detail');
  L.push('');
  L.push('Channels the Rust backend answers today (whole-channel `*` handlers or per-command match arms):');
  L.push('');
  for (const ch of contract.mountainCoverage.channels) {
    L.push(`- \`${ch}\``);
  }
  L.push('');
  L.push('## ProxyChannel service surfaces');
  L.push('');
  for (const [channel, surface] of Object.entries(contract.protocolChannels)) {
    if (!surface.proxyService) continue;
    L.push(`### \`${channel}\` (${surface.proxyService.note ?? ''})`);
    L.push('');
    L.push(`Methods (${surface.proxyService.methods.length}): ${surface.proxyService.methods.map((m) => `\`${m}\``).join(', ')}`);
    L.push('');
    if (surface.proxyService.events?.length) {
      L.push(`Events: ${surface.proxyService.events.map((m) => `\`${m}\``).join(', ')}`);
      L.push('');
    }
  }
  L.push('## Explicit server channel commands');
  L.push('');
  for (const [channel, entry] of Object.entries(contract.protocolChannels)) {
    if (!entry.serverCommands) continue;
    L.push(`### \`${channel}\``);
    L.push('');
    L.push(entry.serverCommands.map((c) => `- \`${c}\``).join('\n'));
    L.push('');
  }
  return L.join('\n');
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);
const mode = args.includes('--check') ? 'check' : args.includes('--summary') ? 'summary' : 'write';

const contract = buildContract();

if (mode === 'summary') {
  console.log(renderMarkdown(contract));
  process.exit(0);
}

if (mode === 'check') {
  if (!fs.existsSync(OUT_JSON)) {
    console.error('compat/ipc-contract.json is missing — run the extractor without --check and commit it.');
    process.exit(1);
  }
  const existing = JSON.parse(fs.readFileSync(OUT_JSON, 'utf8'));
  // Only the SCANNED upstream surface is tripwired. The Mountain coverage
  // (top-level section, per-channel `mountain` status and the summary count)
  // is derived from our own src-tauri code and legitimately changes with
  // every Rust commit — regenerating the contract file alongside Rust
  // changes is encouraged but must not fail CI on its own.
  const strip = (c) => {
    const clone = JSON.parse(JSON.stringify(c));
    delete clone.generatedAt;
    delete clone.mountainCoverage;
    if (clone.summary) {
      delete clone.summary.mountainChannelsImplemented;
    }
    if (clone.protocolChannels) {
      for (const entry of Object.values(clone.protocolChannels)) {
        delete entry.mountain;
      }
    }
    return JSON.stringify(clone);
  };
  if (strip(existing) !== strip(contract)) {
    console.error('IPC contract drift detected: the scanned surface no longer matches compat/ipc-contract.json.');
    console.error('Run `node build/ipc-contract/extract-ipc-contract.mjs`, review the diff, and commit the regenerated contract.');
    process.exit(1);
  }
  console.log(`IPC contract OK (${contract.summary.protocolChannelsProduct} protocol channels, ${contract.summary.plainChannelsProduct} plain channels, no drift).`);
  process.exit(0);
}

fs.mkdirSync(path.dirname(OUT_JSON), { recursive: true });
fs.writeFileSync(OUT_JSON, JSON.stringify(contract, null, 2) + '\n');
fs.writeFileSync(OUT_MD, renderMarkdown(contract) + '\n');
console.log(`wrote ${rel(OUT_JSON)} and ${rel(OUT_MD)}`);
console.log(JSON.stringify(contract.summary, null, 2));
