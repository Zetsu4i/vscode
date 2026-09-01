# Server staging area

This directory is populated by `npm run prepare-server` with the platform's
`vscode-reh-web` build (the VS Code server: extension host, terminals, filesystem).
`tauri build` bundles its contents as the `server` resource of the desktop app.

Only this README is checked in; the staged server build is git-ignored.
