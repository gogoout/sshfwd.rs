# Clipboard Paste

`Ctrl+V` in the TUI reads the local clipboard image, uploads it to the remote, and puts the remote path back in the local clipboard. Lives in `crates/sshfwd/src/paste.rs`.

## Flow

1. Key handler in `app::handle_normal_key` → `handle_paste_image` — rejects with transient error if `connection_state != Connected`.
2. `paste::spawn_paste(fwd_tx, bg_tx)` spawns a `std::thread` (clipboard read is sync, must not block the main loop).
3. Thread: `arboard::get_image()` (RGBA) → `png::Encoder` (PNG) → builds path `/tmp/sshfwd-<unix-ms>.png` → sends `ForwardCommand::UploadImage { path, bytes }` via `fwd_tx`.
4. `ForwardManager::handle_upload_image` spawns a tokio task: `Session::exec_with_stdin("cat > '<path>' && chmod 600 '<path>'", bytes)`. Emits `ForwardEvent::ImageUploaded { path }` or `ImageUploadFailed { error }`.
5. `app::update` for `ImageUploaded`: writes path to clipboard via `paste::set_clipboard_text` (own thread; arboard set is sync), records path in `model.paste_uploads`, sets transient status.

Failures at any step (no image, encode error, channel closed, remote command nonzero) are surfaced as `ImageUploadFailed` so the user sees an error in the footer.

## Model channel handles

`Model` carries `fwd_cmd_tx: Option<...>` and `bg_tx: Option<...>` — clones of the sidecar command channel and the main background channel. They are wired up in `main.rs` after the channels are created and before the main loop starts.

This is the pattern to use when a UI handler needs to **dispatch async work AND receive results back** — different from the standard `update() -> Vec<ForwardCommand>` return path because:
- The clipboard read is sync I/O that must not block update().
- The result event needs to come back through the existing `bg_tx → bg_rx → update()` path so it goes through TEA normally.

`Option<...>` is intentional — channels are only available after main wires them up, but in practice the binding is impossible to trigger before that point.

## Transient status

`paste::TransientStatus { text, expires_at, is_error }` with TTL `TRANSIENT_STATUS_TTL` (currently 1s). Lives in `model.transient_status`.

- Set on `ImageUploaded` (ok) / `ImageUploadFailed` (err) / pre-flight checks in `handle_paste_image` (err).
- Expired in the existing `Tick` handler (which fires every 1s — actual on-screen time is 1–2s depending on tick alignment).
- Rendered by `ui::hotkey_bar::render`: when present and not expired, the status text **replaces** the hotkey row (green for ok, red for err). When None/expired, normal hotkeys render.

If you need to show feedback for some other transient action, reuse `TransientStatus` rather than inventing a new mechanism.

## Cleanup on exit

`model.paste_uploads: HashSet<String>` tracks all paths uploaded in the current process lifetime. In `main.rs`, **after** terminal restore and **before** `process::exit(0)`:

```rust
if !model.paste_uploads.is_empty() {
    let paths: Vec<String> = model.paste_uploads.iter().cloned().collect();
    let (done_tx, done_rx) = crossbeam_channel::bounded::<()>(1);
    let _ = fwd_cmd_tx.send(ForwardCommand::CleanupPasteUploads { paths, done: done_tx });
    let _ = done_rx.recv_timeout(Duration::from_secs(3));
}
```

`ForwardManager::handle_cleanup_paste_uploads` runs a single `rm -f '<p1>' '<p2>' ...` over the live session, then sends `()` on the done channel.

**Pattern for any future "do X before exit" feature**: pass a `crossbeam_channel::Sender<()>` inside the `ForwardCommand` variant, block main on the matching receiver with a short timeout. Do NOT try to do cleanup via destructors — the `process::exit(0)` gotcha from [TUI Architecture](/.claude/rules/tui-architecture.md#exit-gotcha) skips them. The 3s timeout is essential: if the SSH session is mid-reconnect, the cleanup command queues but won't run, and we must not block shutdown indefinitely.

## Shell safety

Paths are constructed from a fixed prefix + Unix-ms digits only, so single-quote wrapping (`'<path>'`) is sufficient. If you ever derive a path from user/clipboard input, swap to proper escaping.

## X11 caveat

On X11, the local clipboard only persists while sshfwd is running (standard X11 selection limitation — arboard does not fork to persist). Acceptable for the primary use case (user pastes path into another terminal while sshfwd is still up). Not an issue on macOS, Wayland, or Windows.

## Dependencies

- `arboard` — cross-platform clipboard, image + text
- `png` — RGBA → PNG encoder

Both in `[workspace.dependencies]` per [Workspace Dependencies](/.claude/rules/workspace-dependencies.md).
