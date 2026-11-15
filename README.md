# ts — fast tmux session switcher

**Docs:** https://rohitsx.github.io/tmux-ts/

One tiny Rust binary (~400 KB, only dependency: `libc`), three commands:

| command | what it does |
|---------|--------------|
| `ts` | list sessions → pick one to switch/attach. `n` or the **+ new session** row creates one. |
| `tn` | create a session: asks for a name (empty = current folder name). Existing name → just switches to it. |
| `tk` | list sessions → mark several → confirm → kill them. |

`tn` / `tk` are symlinks to `ts` (argv[0] picks the mode). Subcommands work too: `ts new`, `ts kill`.

## Install

```sh
git clone https://github.com/rohitsx/tmux-ts.git && cd tmux-ts
./install.sh        # builds and puts ts/tn/tk in ~/.local/bin
```

System-wide (`/usr/local/bin`): build as yourself, then
`sudo install -m755 target/release/ts /usr/local/bin/ts && sudo ln -sf ts /usr/local/bin/tn && sudo ln -sf ts /usr/local/bin/tk`.

## Keys

- **All screens:** `↑↓` / `j k` / `ctrl-n ctrl-p` move · `g`/`G` top/bottom · `esc` / `q` quit
- **`ts`:** `⏎` open · `1-9` jump straight to that session · `n` new
- **`tn`:** type a name · `⏎` create · `ctrl-u` clear · `esc` cancel (`.` and `:` become `_`, like tmux)
- **`tk`:** `space` mark · `tab` mark + next · `a` all · `1-9` toggle that row · `⏎` kill · `y` confirms
  (`⏎` with nothing marked targets the highlighted row)

Works inside tmux (uses `switch-client`) and outside it (`attach-session` / `new-session`).
When you're inside tmux the current session is tagged `current`, and the cursor starts on the
most recently active *other* session (alt-tab style).

## tmux key bindings (popup)

Add to `~/.tmux.conf`, then `tmux source ~/.tmux.conf`:

```tmux
bind s display-popup -E -w 70 -h 60% "ts"   # prefix + s  → switch
bind N display-popup -E -w 70 -h 10  "tn"   # prefix + N  → new
bind X display-popup -E -w 70 -h 60% "tk"   # prefix + X  → kill
```

(`prefix + s` replaces tmux's built-in session chooser; pick other keys if you'd rather keep it.)

## Why it's fast

- No TUI framework: raw termios + hand-rolled ANSI, one `write()` per frame.
- A single `tmux` process per run (inside tmux, the current-session lookup is chained into the
  same call as `list-sessions`), then `exec` into `tmux switch-client` / `attach-session`,
  so `ts` isn't even resident while you're in the session.
- Start → first frame ≈ 3 ms of our own time on top of tmux's ~3 ms process spawn.

## License

[PolyForm Noncommercial 1.0.0](LICENSE) — free to use, copy, modify and share for any
**non-commercial** purpose (personal use, hobby projects, study, research, charities, schools,
public and government bodies). **Commercial use is not permitted**, including use as part of work
for a for-profit company. If you need a commercial license, open an issue.

This is a source-available license, not an OSI-approved "open source" one.
