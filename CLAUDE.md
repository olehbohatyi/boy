# boy

A personal package manager for prebuilt binaries. The product spec is [docs/prd.md](docs/prd.md); read it before changing behaviour.

## Build and check

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test -- --ignored   # tests that hit the network
```

- Rust is pinned to 1.100.0 in `rust-toolchain.toml` (CI uses the same version), edition 2024.
- CI builds `x86_64-apple-darwin` with `MACOSX_DEPLOYMENT_TARGET=10.12` and `aarch64-apple-darwin` with `11.0`, and checks the minimum macOS recorded in the binary. v1 is macOS only.

## Layout

- `src/main.rs`: command dispatch (`i`, `r`, `l`, `s`, `u`).
- `src/home.rs`: the boy home (`BOY_HOME`, else found from the boy binary, else `~/.boy`) and its folders `bins/`, `pkgs/`, `defs/`, `temp/`, `lock.toml`.
- `src/defs.rs`: program definitions (`defs/<name>.toml`), tag patterns, platform keys.
- `src/github.rs`: the `github` source. Public release links only.
- `src/sums.rs`: SHA-256 checks against a release's checksum file.
- `src/archive.rs`: unpacking `.tar.gz` and `.zip`.
- `src/registry.rs`: the local registry copy, refreshed from this repository's archive by direct link.
- `src/install.rs`, `src/list.rs`, `src/remove.rs`, `src/search.rs`: commands.
- `defs/`: the registry, one TOML file per program.

## Rules

- **No GitHub API and no tokens.** Versions come from the `releases/latest` redirect, files from `releases/download/<tag>/<file>`.
- **Nothing outside the boy home.** Archive entries, `path` values and symlinked commands must stay inside the package folder; boy never edits shell files or anything else on the system.
- **Never leave half state.** Download into `temp/`, stage the release there, then move it into `pkgs/` in one rename; on any error `temp/`, `pkgs/`, `bins/` and `lock.toml` are left as they were.
- **Symlinks in `bins/` are relative** (`../pkgs/<name>/<version>/...`), so the whole home folder can be moved.
- **Four-letter names** for folders and definition keys (`bins`, `pkgs`, `defs`, `temp`, `name`, `info`, `from`, `repo`, `tags`, `sums`, `plat`, `file`, `path`). Unknown keys in a definition are an error.
- **Curated registry:** every definition names its release files explicitly with `{version}` templates; no guessing.
- Unit tests live next to the code (`#[cfg(test)]`); tests that need the network are `#[ignore]`. A bug fix comes with a test.
- Error messages are lowercase, start without `error:` (main adds `boy:`), and say what to check.

## Workflow

- One Linear issue (OLE-*) per change, its own branch named after the issue, a pull request into `main`. Nothing is pushed to `main` directly.
- Commit messages and PR titles follow Conventional Commits (`feat:`, `fix:`, `chore:`...).
