# Pushing VSTauri to GitHub (so CI can build your installer)

VSTauri is the **native workbench rebuild** (no vscode-web). It lives in this
repository's `main` branch. CI (`.github/workflows/build.yml`) builds the
**Windows NSIS installer automatically on every push** and uploads it as the
`vstauri-windows-nsis` artifact.

The sandbox that produces this code has **no GitHub credentials by design**
(tokens shared in chat must be revoked). Pushing from your machine is two
commands. Pick one of the two options below.

## Get the code

Download `vstauri-tauri-rebuild.bundle` and run:

```bash
git clone vstauri-tauri-rebuild.bundle vstauri
cd vstauri
git checkout main
```

## Option A — dedicated repo (recommended)

1. Create an **empty** repo on GitHub: `Zetsu4i/vstauri` (no README, no
   license — we already have both).
2. Then:

```bash
git remote add origin https://github.com/Zetsu4i/vstauri.git
git push -u origin main
```

## Option B — branch on the existing Zetsu4i/vscode repo

No new repo needed; CI ships with the push:

```bash
git remote add origin https://github.com/Zetsu4i/vscode.git
git push origin main:refs/heads/native-rebuild
```

## After pushing

1. Open the repo on GitHub → **Actions** → wait for
   **Build VSTauri / windows-nsis** (green in a few minutes thanks to
   rust-cache).
2. Download the `vstauri-windows-nsis` artifact → install → test:
   - Terminal: prompt no longer missing on spawn (pre-attach buffer), a
     `yes` flood stays smooth (flow control), profile dropdown next to "+".
   - Window: resize/move, close, reopen → geometry and sidebar/panel layout
     restored, no white flash at startup.
   - Open an image file in the explorer → preview served via `vstauri://`.

## Legacy note

The old `tauri-rebuild` branch on `Zetsu4i/vscode` (the vscode-web shell)
is **frozen**: VSTauri native is the main line now. If you ever want that
branch green again, from its clone: `git push origin tauri-rebuild` (it is
1 commit ahead with the `IPickAndOpenOptions.title` typecheck fix).
