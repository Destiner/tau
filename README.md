# Tau

A Tauri desktop UI for the [Pi coding agent](https://pi.dev), built with Vue and Rust.

Tau keeps the original app's project and session layout while replacing Pi's own frontend. Production projects are stored in `~/Library/Application Support/tau/projects.json`; development builds use an isolated `tau-dev` app profile and `.tau-dev.json` session metadata. Both profiles keep the underlying Pi session files in the same Pi project session directories.

## Development

Requirements:

- Bun
- Rust
- Pi available on `PATH`, or `TAU_PI_PATH` set to its executable

```sh
bun install
bun tauri dev
```

Tau talks to Pi over RPC: Rust starts `pi --mode rpc` directly and exchanges JSONL with it. Remote projects run the same mode over SSH.

## Workflows and extensions

Tau uses Pi's native extension runtime and supports workflow-critical non-modal prompts, custom tools and hooks, session replacement, notifications, and editor prefill. See the [extension support matrix and compatibility fixture](docs/extensions.md) for supported and deferred APIs.

What a workflow writes is read as text rather than as a heading: a prompt's title and message, and the notifications beside them, are rendered as markdown and wrap. Links in them open in the default browser, and file paths open in whichever application the system gives their type, resolved against the session's working directory. A remote project's files are on the other host, so its paths stay as text.

A turn the model refuses is shown where its reply would have been, rather than leaving the prompt to stand alone. Pi reports these as messages of its own, so the row carries the provider's sentence lifted out of the payload it arrives wrapped in, under the status it was refused with, and any address in it opens in the default browser. Pi keeps them in the session, so they are read back with everything else; a failed compaction is not among its messages and stays only until the next run begins. A retry says what it is retrying while it waits.

Session names are shared with Pi. Clicking the name in the session header edits it and applies the new name on blur or Enter, which renames the session inside Pi itself, and names that Pi or one of its extensions sets appear in Tau without a reload.

The model picker offers the models Pi has scoped through its `enabledModels` setting, which is the list `/scoped-models` edits and `/model` shows. Patterns that match nothing leave the full catalogue in place.

Right-clicking a session in the sidebar opens a context menu that marks it unread, reads it again, or archives it. Text fields carry their own menu with cut, copy, and paste, backed by the system clipboard. The webview's menu is suppressed everywhere, so nothing offers a reload or page navigation. A session marked while it is open keeps its dot until it is selected again, so it stays visible after switching away.

The interface is drawn in [ayu](https://github.com/ayu-theme/ayu-colors), whose light and dark schemes are the only ones Tau has, and it follows whichever one the system is set to. The colours are the palette's own as it is published in `ayu@9`, down to the greys its separators and selections are mixed from; the three that are not, and why, are named at the top of `src/styles.css`, where every colour in the app lives as a custom property. The window's own background colour, painted before the webview has anything to show, is kept in `src-tauri/src/lib.rs` and has to move with the canvas.

Tau aims to behave like a native application rather than a page in a window. The pointer is an arrow over every control and a caret only over text that can be selected, which is transcript messages, tool rows, and status lines. Escape closes whichever surface is open, innermost first. The menu bar carries the standard window and editing shortcuts alongside Tau's own New Session, so `⌘N` can be found rather than only guessed. The window remembers its size, position, and maximised state between launches, opens hidden until the interface is mounted so no empty frame is shown, and dims its selection while it sits in the background.

Each live session owns an isolated Pi runtime. Switching the visible session does not interrupt running work in other sessions, and a hidden session that falls idle keeps its runtime rather than losing it on the way out. Only the least recently active idle runtimes past a small limit are released, and those are restored from their session files when selected again. Runtimes are kept because a session file is not the whole session: an extension driving a workflow holds the context that opens its next phase in the Pi process alone, and stopping that process leaves the workflow to be resumed by hand. Session lists are ordered by the latest user message, so background agent events do not move rows. Removing a project stops all of its runtimes, and quitting Tau stops every child process.

Remote projects use the system OpenSSH client and start directory browsing from the remote account's default working directory. SSH config aliases and command-line options are supported. Authentication must work non-interactively, such as through keys or an SSH agent, and Pi must be available on the remote login shell's `PATH`.

## Checks

```sh
bun run format
bun run lint
bun run typecheck
bun run test
bun run test:e2e
bun run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
bun tauri build
```

The Playwright suite runs the UI against a development-only 5,000-message transcript fixture in Chromium and WebKit. Install its browsers once with `bun x playwright install chromium webkit`.
