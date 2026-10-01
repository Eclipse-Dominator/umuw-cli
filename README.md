# umuw

A thin wrapper around [umu-run](https://github.com/Open-Wine-Components/umu-launcher) for running
non-Steam Windows software on Linux — launchers, their games, and the odd portable exe. umuw gives
each launcher cluster its own Wine prefix by convention, keeps performance tuning (gamescope, HDR,
wrappers, env) in **declarative TOML profiles** instead of copy-pasted `.desktop` lines, and makes
the result launchable from your app grid or a keybind.

umu owns the Wine prefix; umuw owns the *recipe* — which runner, which env, which gamescope mode,
which wrapper — and turns it into the correct `umu-run` invocation every launch.

## Requirements

- Linux, and [umu-run](https://github.com/Open-Wine-Components/umu-launcher) on `PATH`
- A Proton runner umu can find (system or user `compatibilitytools.d`)
- `gamescope` for the gamescope/prompt launch modes (optional — `native` mode doesn't need it)
- A dmenu-compatible picker for the `prompt` mode and `umuw menu`: `noctalia` (default) or `rofi`

## Install

From source (Rust 1.88+, edition 2024):

```bash
git clone <this repo> && cd umuw
cargo install --path .
```

First run seeds `~/.config/umuw/` with a sensible `default` group.

## Usage

Seven verbs. Everything else is a flag or an edit to a TOML file.

```bash
# install a launcher into its own prefix (group = prefix)
umuw install ~/Downloads/HoYoPlay_setup.exe --new-grp hoyoverse

# register a game against a group (shares the prefix + tuning)
umuw new exe "/mnt/ssd/Genshin_Impact/HoYoPlay/games/Genshin Impact game/GenshinImpact.exe" \
             --use-grp hoyoverse --name genshin

# launch — gamescope/HDR/wrapper resolved from the profile
umuw run genshin              # prompt mode: pick a display mode, or wait 10s for the default
umuw run genshin --dry-run    # print the exact command instead of launching
umuw run genshin --windowed --fps 60    # trailing args pass through to the game
                                         # (CLI args replace any profile-level `args`)

# ad-hoc: run any exe once in the default group, nothing written
umuw run ~/Downloads/some-app.exe
umuw run "/mnt/ssd/My Games/app.exe" --windowed    # shell quoting handles spaced paths

# discover and re-surface
umuw list                     # groups, games, prefixes, sizes
umuw sync                     # regenerate .desktop entries + icons into ~/.local/share/applications/umuw/
umuw menu                     # keyboard quick-launch picker over every registered game

# create/edit config files ($EDITOR)
umuw new group scratch
umuw config game genshin      # edit the profile
umuw config group hoyoverse   #   (--rename also migrates members + moves the prefix)
umuw config general           # edit umuw.toml
```

## Configuration

Everything lives in `~/.config/umuw/`, editable by hand — that's the intended way to tune anything:

```
umuw.toml            [setting] picker preference (noctalia|rofi)
                     [global]  fallback defaults for every launch
groups/<name>.toml   one per prefix group — the shared Wine prefix is structural:
                     ~/Games/umuw/<group>/ (one prefix per group, by layout)
games/<name>.toml    one per registered game
```

Example group:

```toml
runner = "DW-Proton Latest"                # anticheat-friendly runner for this cluster
store = "egs"                              # protonfixes db namespace (optional)
drives = ["s:/mnt/ssd"]                    # dosdevices symlinks, ensured every launch (c/z reserved)
gamescope_mode = "prompt"                  # native | gamescope | prompt
gamescope_opts = [
  "3840 2160 3840 2160 120 hdr vrr",       # <w_in> <h_in> <w_out> <h_out> <fps> [hdr] [vrr] [grab]
  "3840 2160 3840 2160 120",
]
wrapper = "~/.config/myscripts/fcitx5-gamescope"   # wraps the launch command (tilde-expanded)
```

Example game (only what differs from its group needs setting):

```toml
group = "hoyoverse"
app_entry = "/mnt/ssd/.../GenshinImpact.exe"
args = ["--windowed"]                       # passed to the game after app_entry
gamescope_opts = [ "3840 2160 3840 2160 60 hdr vrr" ]   # cap this game at 60
```

**Merge rules** — global → group → game:

- knobs (`runner`, `gamescope_mode`, `gamescope_opts`, `hdr`, `store`, `wrapper`, `gameid`,
  `prefix`): nearest wins (game > group > global)
- `env` and `keywords`: additive across all tiers (deduped); `umuw`, the group name, and the game
  name are auto-injected as keywords for `.desktop` search
- `args`: profile sets defaults; CLI trailing args **replace** them when given

Fixes identity is separate from prefix location: `GAMEID` defaults to `umu-<game name>` and selects
protonfixes; the prefix is always `~/Games/umuw/<group>/` regardless.

## How a launch resolves

1. merge profiles (above), resolve runner path
2. ensure declared `drives` (idempotent; replayed after a prefix rebuild)
3. resolve the display mode — `prompt` opens the picker (timeout → first option, Esc → exit 0),
   `gamescope` uses the first entry, `native` runs bare
4. exec `gamescope <flags> -- env [vars] [wrapper] umu-run <app> [args]` (or the native
   equivalent), with umu's env contract: `WINEPREFIX`, `GAMEID`, `PROTONPATH`, `STORE` when set

## Notes

- One prefix per **group**; the launcher installer and all its games share it. Per-game isolation
  is opt-in via `prefix = "fragment"` on a game.
- `sync` owns `~/.local/share/applications/umuw/` wholesale (regenerates it) and extracts exe icons
  into `~/.local/share/icons/custom-icons/umuw/`.
- winetricks stays imperative on purpose: the prefix's `winetricks.log` is the record; a
  declarative ensure is a documented upgrade path only.
- Personal tool, built for one Hyprland + noctalia + umu setup; the dmenu adapter and XDG paths
  keep it adaptable to rofi or a stock desktop.
