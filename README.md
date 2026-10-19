# Tau

A minimal, focused native interface for [Pi](https://pi.dev), built with GPUI.

## Features

- Projects with persistent sessions, including remote projects over SSH
- Pi extension commands and inline prompts
- Archived sessions, steering and follow-up queues
- Markdown, code and Mermaid rendering
- File previews and a keyboard-first command palette

## Build and develop

Development targets macOS with Bun, Rust and Xcode/Metal tools. Install Pi separately;
Tau does not bundle Pi, Node, SSH, provider credentials or extensions.

```sh
bun install --frozen-lockfile
bun run prepare
bun run dev -- --isolated /absolute/path/to/profile --project /absolute/path/to/project
```

Development requires an explicit isolated profile. It persists across launches and
starts without synthetic projects. This separates Tau metadata and local Pi sessions,
**not** tools, credentials, extensions or access to real files. See
[development storage](docs/development-storage.md).

```sh
bun run check
bun run test
bun run build
bun run package
```

`build` produces the optimized native binary; `package` creates a fresh ad-hoc macOS
preview. **Public native distribution and updates are not implemented.** The old
v0.2.1 GitHub DMG is a historical Vue/Tauri build, not this native application.
See [packaging](gpui/PACKAGING.md) before building or launching a preview.

The canonical app remains in `gpui/` to preserve workspace/cache paths: Cargo package
`tau-gpui`, binary `tau`. Shared crates provide storage/process services, Pi RPC
orchestration and observability. There is no Vue/Tauri app or Playwright suite.
Direct Cargo commands from root work after prepare with `--manifest-path gpui/Cargo.toml`.

A normal product launch without `--isolated` uses existing production storage, not
a development sandbox. The cutover is cold-turkey: close the old app first and never
run both against that profile. Pi owns transcripts; migration of old Vue drafts is
not guaranteed. See [application contracts](gpui/APP.md), [Pi compatibility](docs/pi.md),
[keyboard commands](docs/keyboard.md) and [testing](docs/ci-testing.md).

## Non-goals

- Support for other harnesses
- Integrated terminal and browser
- Debugging tools
- File editor

## Philosophy

Pi is lean, stable and extensible. Tau follows that approach at the UI level:
focus on projects, sessions, models and transcripts, and leave extra capability to
extensions. Something used ten hours a day should be fast, reliable and feel owned
by its user, without a growing set of soon-redundant orchestration features.

Also, no tabs.

## Inspirations

- [Zed](https://zed.dev)
- [Linear](https://linear.app)
- [iA Writer](https://ia.net/writer)
