# Schnipsle

A local-first note editor for macOS and iOS that keeps every note as a plain Markdown file and organizes notes by **hashtags** and **saved searches** instead of folders.

> **Status:** early development (milestone M0, foundations). Not usable for real notes yet.

## Goals

- **Plain files.** A vault is an ordinary folder of `.md` files with images next to them. Your notes stay readable without the app, and any sync service (iCloud Drive, Nextcloud, Syncthing) can sync them.
- **Virtual folders.** `#radio` in a note puts it in the _radio_ folder; a note with `#radio #cw` also appears in _radio › cw_. Hierarchical tags (`#projects/tuner`) and saved searches work the same way.
- **Nothing gets lost.** Every note keeps a version history, and deleted notes stay searchable in the trash.
- **Import and export.** Import from Apple Notes, Simplenote, HTML and Markdown folders; export to HTML, PDF and OpenDocument.
- **Music.** Scores in ABC notation and song sheets with chords (ChordPro), rendered, playable and searchable.

Platforms: macOS and iOS first; Linux and Android planned; Windows possible.

## Documentation

| Document                                     | Contents                                                   |
| -------------------------------------------- | ---------------------------------------------------------- |
| [High-level design](docs/design/README.md)   | Requirements, architecture, roadmap                        |
| [Architecture decisions](docs/adr/README.md) | Why the project is built the way it is                     |
| [Vault format](docs/format.md)               | The on-disk format, including tags, music and chord syntax |
| [Development setup](docs/dev-setup.md)       | Tools and commands for macOS (and Linux)                   |
| [Contributing](CONTRIBUTING.md)              | Workflow, conventions, sign-off                            |

## Quick start for developers

```sh
git clone https://github.com/oe1wkl/schnipsle.git ~/Developer/schnipsle
cd ~/Developer/schnipsle
scripts/bootstrap-macos.sh   # installs the toolchain and runs all checks
pnpm tauri dev               # desktop app
pnpm tauri ios dev           # iOS simulator (after `pnpm tauri ios init`)
```

## Repository layout

```text
crates/core/     vault, Markdown parsing, tags, index, queries (no UI dependency)
crates/import/   importer adapters (Apple Notes, Simplenote, HTML, …)
crates/export/   exporters (HTML, PDF, ODT; optional Pandoc)
src-tauri/       Tauri 2 app shell: commands, platform code, app config
ui/              TypeScript front end
docs/            design, ADRs, format specification, setup guide
scripts/         bootstrap and check scripts
tests/fixtures/  golden files for format, import and export tests
```

## License

[Apache-2.0](LICENSE). Contributions are accepted under the same license with a [DCO sign-off](CONTRIBUTING.md#sign-off).
