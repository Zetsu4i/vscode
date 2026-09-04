# Parity checklists

One file per ported module: `docs/tauri/parity/<module>.md`. A module may only enter **State C (cutover)** when its checklist is fully green (`AGENTS.md` §M-1), and the Deletion Ledger row in `ROADMAP.md` links to it as deletion evidence.

Use the template below.

~~~
# Parity: <module>

State: A | B | C
Upstream ref: <path in src/vs/...>
Rust/Tauri home: <path in src-tauri/...>

## Scenarios (steps → expected result, from real Electron VS Code behavior)
- [ ] <scenario 1>
- [ ] <scenario 2>

## Automated tests
- [ ] <test name / file> or "n/a — manual only (justify)"

## Perf vs Electron baseline
startup: <ms> vs <ms> · memory: <MB> vs <MB> · op latency: <ms> vs <ms>

## Screenshots / notes
<attach images or describe deviations; every deviation needs a ledger entry>
~~~
