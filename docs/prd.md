# boy: Product Requirements Document

Working name until now: gpm. New name: boy (the initials of the author's full name).

## 1. Problem

Installing command-line programs often turns into a fight with dependencies, compilation and documentation. I need a tool that simply installs prebuilt binaries.

## 2. Goal

Build a personal package manager, boy, that is convenient for me and works on everything I own. This is infrastructure built for myself: if a decision is convenient for me, it is the right decision. The project is open source, but the primary user is me.

The user types `boy i jq` and never needs to know where the program comes from or how it is packaged. That knowledge lives in boy's own curated registry.

## 3. Principles

- KISS: simple enough that I never need to open the documentation. Running `boy` with no arguments prints the five commands.
- Prebuilt binaries only: no compilation on the user's machine, no Xcode, no Command Line Tools.
- Curated, not guessed: every supported program has a hand-written definition that has been tested. No heuristics for picking files.
- Short commands: a single letter, no dashes and no flags wherever possible.
- Four-letter names: folders, files and definition keys use four-letter names (`bins`, `pkgs`, `defs`, `repo`, `tags`...), so everything looks the same.
- Portable: the whole boy folder can be moved and keeps working.
- One self-contained binary with no runtime and no dependencies beyond the OS itself.
- No accounts, no tokens, no API keys.

## 4. Platforms

| Version | OS | Targets | Testing |
|---|---|---|---|
| v1 | macOS 13+ Intel | `x86_64-apple-darwin` | By hand on my Mac (macOS 13.7, Intel) |
| v1 | macOS 13+ Apple Silicon | `aarch64-apple-darwin` | CI on an arm64 macOS runner |
| v1.1 | Linux | `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` | Docker containers |
| later | Windows | `x86_64-pc-windows-msvc` | To be decided |

boy itself is built with a low `MACOSX_DEPLOYMENT_TARGET` (10.12 for Intel, 11.0 for Apple Silicon). Binaries are built in CI and published in GitHub Releases.

## 5. Folder Layout

```
~/.boy/
  bins/       relative symlinks to executables; the only folder added to PATH
  pkgs/       full extracted releases: pkgs/<name>/<version>/
  defs/       local cache of the registry: defs/<name>.toml
  temp/       downloads in progress
  conf.toml   boy settings
  lock.toml   installed packages
```

- Every release is kept whole in `pkgs/<name>/<version>/`, so tools that need files next to the binary (for example a `runtime/` folder) work.
- `bins/` holds **relative** symlinks, for example `bins/jq -> ../pkgs/jq/1.8.2/jq`. Together with the next rule, this makes the whole folder portable.
- boy finds its home from the location of its own binary (`<home>/bins/boy`), not from a hard-coded path. `BOY_HOME` can override this.
- The default location on first install is `~/.boy`.
- `lock.toml` records, for each package: name, version, whether it is pinned, the release file it came from, and the symlinks it created.

## 6. Registry

### 6.1 Where it lives

- The registry is the `defs/` folder in the boy repository, one TOML file per program, named after the program: `defs/jq.toml`.
- boy keeps a local copy in `<home>/defs/`. It refreshes the copy during `boy u`, and during `boy i` when the requested program is not in the local copy.
- Adding a program to the registry is one commit. It does not need a new boy release.
- Every program in the registry is tested before it is added. That review also guarantees that no two programs provide the same command.

### 6.2 Definition format

All keys have four letters.

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | Program name, used in `boy i <name>`. Matches the file name. |
| `info` | no | One-line description, shown and searched by `boy s`. Every registry definition has one. |
| `from` | yes | Source type. Selects the implementation that fetches it: `github` or `oracle`. |
| `repo` | yes | Where the program lives in that source: for `github` the repository, for example `jqlang/jq`; for `oracle` the Java major version, for example `27`. |
| `tags` | no | Release tag pattern. Default `v{version}`. |
| `bins` | yes | Commands the package provides. Each gets a symlink in `bins/`. With several commands, each is found in the same folder as `path`. |
| `sums` | no | Name of the checksum file in the release, for example `SHA256SUMS`. May contain `{version}` and `{file}`, the release file name of the current platform. |
| `site` | no | `github` only. An `https://` address the release files and checksum files are downloaded from instead of the GitHub release; the version still comes from GitHub. May contain `{version}`. |
| `[plat]` | yes | One entry per supported platform: `file` is the release file name, `path` is the executable inside the archive (or the name to save a bare binary as). |

`{version}` is the version without the tag's prefix. Platform keys in v1 are `macos-x86_64` and `macos-aarch64`. If the current platform has no entry, boy says the program is not available for this system and stops.

Example: a bare binary with a tag prefix and a checksum file.

```toml
name = "jq"
info = "Command-line JSON processor"
from = "github"
repo = "jqlang/jq"
tags = "jq-{version}"
bins = ["jq"]
sums = "sha256sum.txt"

[plat]
macos-x86_64  = { file = "jq-macos-amd64", path = "jq" }
macos-aarch64 = { file = "jq-macos-arm64", path = "jq" }
```

Example: an archive with the binary in a subfolder and no checksum file.

```toml
name = "hyperfine"
from = "github"
repo = "sharkdp/hyperfine"
bins = ["hyperfine"]

[plat]
macos-x86_64  = { file = "hyperfine-v{version}-x86_64-apple-darwin.tar.gz", path = "hyperfine-v{version}-x86_64-apple-darwin/hyperfine" }
macos-aarch64 = { file = "hyperfine-v{version}-aarch64-apple-darwin.tar.gz", path = "hyperfine-v{version}-aarch64-apple-darwin/hyperfine" }
```

Supported file formats: `.tar.gz`, `.tgz`, `.tar.xz`, `.zip`, and bare binaries.

Checksums are SHA-256 (for the one exception see below). The `sums` file may use either line style, and lines for other files or other algorithms are ignored:

- `<hash>  <file>`, as written by `sha256sum`;
- `SHA256 (<file>) = <hash>`, the BSD style.

A release with one checksum file per download names it with `{file}`, for example `sums = "{file}.sha256"`. Only such a file may also hold the bare hash and nothing else.

The exception: when `sums` contains `{file}` and ends in `.sha1`, the file holds the bare SHA-1 hash. Maven Central publishes no stronger one.

boy 0.5.0 and older know neither `site` nor SHA-1: a definition with `site` is an unknown key to them.

boy 0.3.0 and older understand neither `{file}`, BSD lines nor a bare hash, and fail to install a program whose definition needs one of them.

### 6.3 Sources

`from` is the extension point. Each source is a separate implementation behind one interface: find the latest version, build the download URL, download. v1 implements `github` (section 7) and `oracle` (section 7.1). Later sources (for example my own server) are added as new implementations without changing definitions that use the existing ones. boy 0.4.0 and older do not support `oracle` and say so when asked to install such a program.

### 6.4 Per-program setup (planned)

Not built yet, and not part of version 1. Some programs need more than a command in `bins/`: a variable (`JAVA_HOME` for `jdk`), an extra folder on `PATH`, an alias, or a line for shell completion.

A definition declares this in an optional `[init]` table. There are no install scripts.

| Key | Meaning |
|---|---|
| `envs` | Environment variables to set. |
| `dirs` | Extra folders to add to `PATH`. |
| `abbr` | Shell aliases. |
| `eval` | Lines the shell runs at startup, for example shell completion. |
| `note` | Message printed after install. |

```toml
[init]
envs = { JAVA_HOME = "{pkgs}/jdk-{version}.jdk/Contents/Home" }
note = "open a new terminal"
```

`{pkgs}` is the folder of the installed version, `pkgs/<name>/<version>/`.

- boy collects `[init]` from every installed program into one generated file, `<home>/init.sh`, and rewrites it on install, update and remove. Removing a program therefore removes its setup.
- The user adds one line to their shell startup file, once: `source "$HOME/.boy/init.sh"` (or the path of their boy home). `install.sh` may offer it, as it does for `PATH`. boy itself still never edits shell startup files.
- boy 0.5.0 and older reject unknown keys, so the order is: code, then a release, then definitions that use `[init]`.

To decide before building:

- **Two shells:** zsh and bash must both be able to read the file, while `eval` lines often differ per shell (`starship init zsh`, `starship init bash`).
- **Moving the home:** the file must stay correct when the boy home is moved (section 3), so it cannot simply hold absolute paths.
- **`PATH` rule:** section 5 makes `bins/` the only folder added to `PATH`; `dirs` changes that.
- **Several versions of one tool:** for example two JDK majors both provide `java`, so there must be a way to pick the active one.
- **Trust:** `eval` lines run in the user's shell, so they are only as safe as the registry.

## 7. Network: the github Source

boy uses no GitHub API and no tokens. It only follows public links.

- **Latest version:** request `https://github.com/<repo>/releases/latest`. GitHub answers with a redirect to `/releases/tag/<tag>`. boy reads the tag from the redirect and extracts `{version}` using the `tags` pattern.
- **Download:** `https://github.com/<repo>/releases/download/<tag>/<file>`.
- **Checksum:** if `sums` is set, boy downloads that file from the same release and checks the SHA-256 of the downloaded file against it. On a mismatch boy refuses to install. If `sums` is not set, boy installs and prints a warning.

Because none of this goes through the API, there is no rate limit to worry about, however many packages are installed.

### 7.1 The oracle Source

For Oracle JDK, which is not published on GitHub. It needs no account and no token. `repo` is a Java major version, and `tags` is `{version}`.

- **Latest version:** `https://java.oraclecloud.com/javaVersions` lists each major version with its latest release. boy takes the entry for `repo`.
- **Download:** `https://download.oracle.com/java/<repo>/archive/<file>`. A pinned version exists if its file does.
- **Checksum:** as for `github`; Oracle publishes a bare hash beside each file, so `sums = "{file}.sha256"`.

A new major version is a change to `repo` in the definition.

## 8. Version 1 Commands

| Command | Behavior |
|---|---|
| `boy i <name>` | Install the latest version and record it in `lock.toml`. |
| `boy i <name>@<version>` | Install that exact version and mark it as pinned. If another version is installed, switch to this one and delete the old one (this is the rollback). |
| `boy r <name>` | Remove the package folder and its symlinks, and drop it from `lock.toml`. |
| `boy l` | List installed packages with their versions, marking pinned ones. |
| `boy s [query]` | Search the local registry copy by name, description and commands, ignoring case; without a query, list every program. No network, except that the very first search fetches the registry. |
| `boy u` | Refresh the registry, update every package that is not pinned, then clean up. |
| `boy u <name>` | Update one package. This also unpins it. |

`boy l` prints one line per package, sorted by name, in aligned columns; pinned packages are marked. With nothing installed it prints `nothing installed`.

```
hyperfine  2.0.0
jq         1.7.1   pinned
just       1.59.0
```

`boy s` prints one line per program, sorted by name, with its description; installed programs are marked with their version. With no match it prints ``nothing found for `<query>` ``.

```
hyperfine  Command-line benchmarking tool
jq         Command-line JSON processor      installed 1.8.2
just       Command runner for project tasks
```

`boy u` does three steps: refresh the registry, upgrade the packages, clear old versions. Upgrading extracts the new version next to the old one and switches the symlinks. At the end boy asks one question for everything it updated:

```
updated: jq 1.7.1 -> 1.8.2, just 1.58.0 -> 1.59.0
remove old versions (jq 1.7.1, just 1.58.0)? [Y/n]
```

Answering `n` keeps the old versions, and the next `boy u` asks again. An empty answer means yes; no answer at all (closed input, for example in a script) keeps them. With nothing newer, boy prints `everything is up to date`. If the registry cannot be fetched, boy warns and checks versions against its local copy. A package that fails to update is reported, the others still update, and `boy u` exits with an error.

## 9. Install and Self-Update

- **First install:** `curl -fsSL https://github.com/olehbohatyi/boy/raw/main/install.sh | sh`. The script is served as a raw file from `main` of the boy repository, so it always matches the latest code. It downloads the latest release for the Mac's architecture by direct link, checks it against `SHA256SUMS`, and runs `boy i boy` so boy creates `~/.boy` (or `$BOY_HOME`) and records itself in `lock.toml`. Then it offers to add `~/.boy/bins` to PATH: on yes it appends one `export PATH=...` line to `~/.zshrc` (zsh) or `~/.bash_profile` (bash); otherwise, or without a terminal, it prints the line. If boy is already installed, it stops and points to `boy u boy`. A manual download from the releases page works too.
- **PATH:** boy itself never edits shell startup files. If `bins/` is not on PATH, boy prints the one line to add.
- **Self-update:** boy is in its own registry (`defs/boy.toml`). Releases come from `.github/workflows/release.yml`: pushing a tag `v<version>` that matches `Cargo.toml` builds both macOS targets and publishes `boy-<version>-<target>.tar.gz` (binary at the archive root) and `SHA256SUMS`. Every utility in the family copies this workflow. `boy u` updates it like any other package: the new version is extracted into `pkgs/boy/<version>/` and the `bins/boy` symlink is switched. This is safe on macOS even while boy is running.

## 10. Language and Stack

Rust, written from scratch. gpm contributes lessons only (configurable install location, binary-only scope), no code.

- HTTP: a blocking client with `rustls` and bundled root certificates (`webpki-roots`). No OpenSSL, no dependency on the system root certificates. Redirects must be readable, not only followed.
- Archives: `flate2` + `tar`, `lzma-rs` (pure Rust, for `.tar.xz`), `zip`.
- Definitions and state: `serde` + `toml`. Oracle's version list: `serde_json`.
- Checksums: `sha2`, `sha1`.

## 11. Utility Family

On top of boy I will build my own console utilities: my own cd, my own grep, and others. Rules for every utility:

- Lives in its own repository.
- Publishes releases with prebuilt binaries for all supported platforms, built by the same CI workflow as boy.
- Gets a definition in the registry and installs through boy like any other package.

The utilities themselves are not part of boy version 1.

## 12. Out of Scope for Version 1

- Linux (v1.1) and Windows (later).
- Building packages from source.
- Dependency management between packages.
- Sources other than `github` and `oracle`.
- GitHub API access and tokens.
- Guessing release files without a definition.
- Checking whether a downloaded binary supports the running macOS version. Testing each program before adding it to the registry covers this for now.
- Rollback as a separate command (`boy i <name>@<version>` covers it).
- The utility family (cd, grep and so on).

## 13. Test Programs

None of these are installed on my Mac, and each one tests a different case.

| Program | Tag | macOS file (Intel) | Tests |
|---|---|---|---|
| jq | `jq-1.8.2` | `jq-macos-amd64` | Bare binary, tag prefix, no version in the file name, `sha256sum.txt` |
| just | `1.59.0` | `just-1.59.0-x86_64-apple-darwin.tar.gz` | Plain `tar.gz`, tag without `v`, `SHA256SUMS` |
| hyperfine | `v2.0.0` | `hyperfine-v2.0.0-x86_64-apple-darwin.tar.gz` | `v` in the tag and the file name, binary in a subfolder, no checksum file |
| fzf | `v0.75.0` | `fzf-0.75.0-darwin_amd64.tar.gz` | `v` in the tag only, different platform naming, `{version}` in the checksum file name |
| helix | `25.07.1` | `helix-25.07.1-x86_64-macos.tar.xz` | `tar.xz`, needs `runtime/` next to the binary (whole-release storage plus symlink) |
| ripgrep-all | `v0.10.10` | `ripgrep_all-v0.10.10-x86_64-apple-darwin.tar.gz` | Several commands in one package |
| delta | `0.22.0` | none (Apple Silicon only) | "Not available for this system" |

The archive layouts are confirmed when each definition is written.

## 14. Definition of Done for Version 1

- `boy i`, `boy u` and `boy r` work on macOS 13 Intel for jq, then just, then hyperfine, in that order.
- `boy l` and `boy s` work against those three.
- `boy u` updates boy itself.
- `install.sh` installs boy from a release on a clean macOS 13.
- CI builds and publishes boy's release binaries for `x86_64-apple-darwin` and `aarch64-apple-darwin`.

After v1, fzf, helix, ripgrep-all and delta are added to the registry one at a time. Each breaks one assumption of the first three.

## 15. Open Questions

None for version 1. The planned per-program setup has its own open points (section 6.4).

## 16. Workflow

This file (`docs/prd.md`) is the master copy of the PRD. A Linear project holds the PRD, and tasks are sliced inside that project by Claude Code through the Linear MCP connection.
