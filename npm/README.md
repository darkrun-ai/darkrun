# darkrun npm distribution (per-arch native binaries)

darkrun ships as a **native Rust binary** with **no JavaScript runtime**. npm is used purely as a
distribution channel, following the same pattern as esbuild / biome / swc.

## How it works

- The main package [`darkrun`](../plugin/package.json) (published from `plugin/`) declares each
  platform binary as an **optionalDependency**: `@darkrun/darwin-arm64`, `@darkrun/darwin-x64`,
  `@darkrun/linux-x64`, `@darkrun/linux-arm64`, `@darkrun/win32-x64`.
- Each sub-package here pins `os` + `cpu`, so **npm installs only the one matching the host**.
- The main package's `bin/darkrun` is a tiny POSIX shim (`bin/darkrun.cmd` on Windows) that
  resolves the installed `@darkrun/<os>-<arch>/bin/darkrun` and `exec`s it. No download step, no
  postinstall, no JS execution.

## Filling the binaries

These directories contain only `package.json` templates. CI builds the per-target binaries with
`cargo build --release` and drops them into each `npm/<os>-<arch>/bin/` before `npm publish`.
See `.github/workflows/release.yml`. Locally:

```sh
# example: macOS arm64
cargo build --release -p darkrun-cli --target aarch64-apple-darwin
mkdir -p npm/darwin-arm64/bin
cp target/aarch64-apple-darwin/release/darkrun npm/darwin-arm64/bin/darkrun
```

## Targets → package map

| rust target | npm package | os / cpu |
|---|---|---|
| `aarch64-apple-darwin` | `@darkrun/darwin-arm64` | darwin / arm64 |
| `x86_64-apple-darwin` | `@darkrun/darwin-x64` | darwin / x64 |
| `x86_64-unknown-linux-gnu` | `@darkrun/linux-x64` | linux / x64 |
| `aarch64-unknown-linux-gnu` | `@darkrun/linux-arm64` | linux / arm64 |
| `x86_64-pc-windows-msvc` | `@darkrun/win32-x64` | win32 / x64 (`darkrun.exe`) |
