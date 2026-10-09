# Owned native path dialogs

`PathChooser<T>` adapts GPUI's existing `App::prompt_for_paths` and
`App::prompt_for_new_path`. It selects one path; it does not read/write files,
import projects, create sessions, persist state, or install menu actions.

## Ownership

Construct a chooser from a window root's `Window` and `Context<T>`. Store it with
that root or its host service. Call `select` from the same root context. Completion
receives the original root/window, not whichever window is now active. With Base
Root as the window root, use `PathChooser<Root>` and weak handles to child views;
this API deliberately rejects a child context presented as the window root.

```rust,ignore
self.paths.select(PathOperation::ProjectFolder, window, cx,
    |root, outcome, window, cx| match outcome {
        PathOutcome::Selected(path) => root.import_project(path, window, cx),
        PathOutcome::Cancelled => {},
        PathOutcome::Failed(_) => root.show_folder_picker_failure(window, cx),
    });
```

The example methods belong to the host. Callbacks own reviewed failure copy,
path validation, import/persistence and subsequent asynchronous operations. Do
not capture strong roots: use the supplied root or weak handles to other views.
The adapter records no paths, raw errors or telemetry.

`PathStart::Busy` rejects all concurrent operations on the same chooser, without
launching another panel or calling the rejected callback. `WrongContext` also
has no effects. Completion clears busy before calling the host, allowing retry.
Independent windows may own separate choosers.

Call `invalidate()` before replacing workflow context on the same root. Reuse the
chooser rather than replacing it to escape busy. Selected, cancelled and failed
results are discarded after invalidation, including results already sent but
not yet dispatched. Root replacement (even with the same Rust type), root/service
removal, and window closure also discard late delivery. The service and pending
task hold only weak root references and non-owning window handles. A vetoed close
does not invalidate the workflow unless the host explicitly does so.

## Operations

- `ProjectFolder`: directories only, one selection, with the prompt label
  “Choose a Project Folder”. Choosing a folder does not import it: the host still
  owns canonicalization, access checks, project creation and persistence.
- `OpenFile`: one local file, without replacing project-scoped preview/security
  rules or adding remote-file acquisition.
- `SaveDestination`: an initial directory and optional suggested name. This
  chooses a destination only; it does not create an export feature, overwrite
  policy, file or remote-path model.

The macOS Save backend preserves the native selected file URL. The inherited
macOS >=15 extension-stripping workaround could turn legitimate `a.sql.s` into
`a.sql`, bypassing overwrite consent for that different destination. It is
removed rather than replaced by another guess. OS-added extensions are preserved
as well; the backend cannot distinguish them safely from user-chosen extensions.

Outcomes distinguish `Selected(PathBuf)`, OS `Cancelled`, and content-free
`Failed(Platform | Disconnected | InvalidSelection)`. Zero/multiple paths from a
single-path Open request are invalid, not cancellation. Stale results are not
delivered as cancellation to a replacement workflow.

## Native panel boundary

Waiting for a person has no arbitrary short deadline. Logical invalidation does
**not** dismiss the OS panel: the existing API returns a receiver, not a panel
cancellation handle. The chooser stays busy until that receiver settles. A
closed/replaced root's panel may remain visible; its result is ignored. Host
close/quit policy must account for this independently.

GPUI's APIs are App-scoped, with no owning-window argument. The macOS backend
uses `beginWithCompletionHandler`, not an owner-attached sheet. This adapter
provides logical delivery ownership, not sheet attachment, positioning,
activation or dismissal. It does not activate another window or add a second
picker backend. Selection is not filesystem validation or write authorization.

## Verification

```sh
cargo test --locked --manifest-path gpui/Cargo.toml --lib native::paths
```

Tests use actual window roots and the existing TestPlatform chooser queues, with
synthetic paths and no file I/O or OS panels. They cover options, success/cancel/
failure/disconnection, invalid responses, duplicate rejection, retry, activation
changes, explicit invalidation, root replacement and closure with externally
retained roots, and weak ownership while a panel is pending. The Core test patch
adds failure/disconnection delivery only; it does not change production choosers.
The Save fake checks the initial directory but does not retain the suggested
name; that forwarding is source-reviewed rather than observed by the fake.

Separate macOS tests exercise production NSURL conversion with compound,
repeated, Unicode and escaped filenames, plus rejection of non-file URLs. No
panel is opened or file accessed. Real OS presentation, overwrite and alias
behavior remain outside these deterministic checks.
