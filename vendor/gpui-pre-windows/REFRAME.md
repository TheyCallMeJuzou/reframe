# gpui-pre-windows, with reframe's changes

GPUI's Windows backend (`gpui-pre-windows` 0.3.12 from crates.io, Apache-2.0, a snapshot of Zed's
`gpui_windows`), used instead of the published crate through `[patch.crates-io]` in the
workspace's `Cargo.toml`. The first commit that added it here has it unchanged; `git log` on this
folder shows every change since. Each change is marked with a comment that starts with
"reframe".

What is changed:

- **Frames only on demand** (`vsync.rs`: `FrameDemand`, `FrameClock`; `platform.rs`,
  `window.rs`, `events.rs`). Upstream's vsync thread wakes every window on every display refresh,
  whether anything changed or not (280 times a second on a 280 Hz display). Now a window is only
  ticked while it wants frames: GPUI wakes it through `frame_waker`/`schedule_frame` (which the
  macOS backend already uses) when a view changed, an animation runs or a frame was throttled,
  and it stops after three ticks that drew nothing. With nothing to draw the thread sleeps.
  reframe idle with three sessions: about 2 % of one core before, below 0.2 % after.
- **Notifications play the system's sound** (`system_notifications.rs`), as the Electron
  version's did; upstream makes them silent.
- **Drops** (`drop_extras.rs`, `WindowsDragDropHandler` in `window.rs`). Upstream takes only files
  on disk. Now files that exist only in the drag (a picture dragged out of a browser:
  "FileGroupDescriptorW" with "FileContents") are taken too, written when dropped into the folder
  set with `set_dropped_files_folder`. A drag of something the system has no format for (a tab
  torn out of Windows Terminal) is not taken, but `set_foreign_drag_handler` is told where it is,
  so reframe can show where the tab would go.
- **File types in the file dialog** (`platform.rs`: `set_next_open_file_types`). Upstream's
  dialog for opening a file has none; reframe's "Choose…" for claude offers "Programs" and "All
  files".
- **Prompts like Electron's message boxes** (`window.rs`: `set_next_prompt`, `PromptStyle`).
  Upstream's task dialog is titled "Info", "Warning" or "Critical" and shows every answer as a
  button. reframe's questions have their own title, their answers as command links above the
  system's Cancel button, a "Don't ask again" checkbox and a default answer, as in 0.x.

When GPUI is upgraded, the new version of the crate goes here the same way (unchanged first),
and these changes are made again on top of it.
