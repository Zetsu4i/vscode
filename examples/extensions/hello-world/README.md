# hello-world (sample VSTauri extension)

The canonical sample for the VSTauri Rust-native extension format.

## Layout

```
acme.hello/
└── extension.json   manifest: id, publisher, commands, keybindings
```

`main: "extension.wasm"` points at the module the Phase-3 wasmtime runtime
will execute. Until then, the manifest is discovered and listed in the
Extensions view; contributes are parsed and validated by
`src-tauri/src/ext/manifest.rs`.

## Install

Copy the folder (renamed to the extension id) into either location:

- `~/.vstauri/extensions/acme.hello/`
- `<workspace>/.vstauri/extensions/acme.hello/`

Reload the Extensions view in VSTauri.
