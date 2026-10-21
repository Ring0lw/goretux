<div align="center">

# goretux

<br>

simple external gorebox cheat, for linux only.

<br>

https://github.com/Ring0lw/goretux/raw/main/demo.mp4

<br>

Runs next to the game under Proton. Nothing is injected: it reads and writes the game's memory from outside and draws its own overlay.
There are no prebuilt binaries, you build it yourself. It takes about a minute.

</div>

<br>

## Quick Start

> [!NOTE]
> Running NixOS, Fedora Atomic, Hyprland, a Wayland session or a hardened kernel?
>
> See [compatibility.md](compatibility.md).

**Get it:**

```bash
git clone https://github.com/Ring0lw/goretux
cd goretux
```

Clone it with git, don't download a zip: `run.sh` updates the checkout with `git pull`.

**Setup (one-time only):**

```bash
./setup.sh
```

> **Log out and back in, or restart (required)**

This loads the `uinput` kernel module, installs a udev rule for it and adds your user to the `input` group, so goretux can move the mouse (aimbot) and read it while the menu is open.
If your kernel restricts `ptrace`, it offers to lift that too, see [compatibility.md](compatibility.md#ptrace).
Once per machine, updates don't need it again.

**Run:**

```bash
./run.sh
```

This updates the checkout, builds a release binary and starts it. It tells you if Rust or a C++ compiler is missing.
Start GoreBox before or after, goretux waits for it.

**Insert** opens the menu, **Home** toggles stream mode, **End** quits.

<br>

## Build from Source

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone https://github.com/Ring0lw/goretux
cd goretux
cargo run --release
```

Needs Rust 1.88 or newer and a C++ compiler, which Dear ImGui is built with:

| distribution | C++ compiler |
|---|---|
| Arch, CachyOS, EndeavourOS, Manjaro | `sudo pacman -S --needed base-devel` |
| Debian, Ubuntu, Mint, Pop!_OS | `sudo apt install build-essential` |
| Fedora, Nobara | `sudo dnf install gcc-c++` |
| openSUSE | `sudo zypper install gcc-c++` |
| Void | `sudo xbps-install -S gcc` |

At runtime it loads the usual X11 and OpenGL libraries (`libX11`, `libX11-xcb`, `libXcursor`, `libXrandr`, `libXi`, `libxkbcommon-x11`, `libGL`/`libEGL`), which every desktop already has.

`goretux --help` lists the options, including the read-only tools for finding things again after a game update.

<br>

## Features

### ESP

- Box
- Skeleton
- Head circle
- Name
- Distance
- Snaplines
- NPCs and dead bodies
- Visibility check (the map's own collision, built into a BVH)
- Teammates in their own color, or hidden
- Off-screen arrows
- Max range
- Delay (to line up with what's on screen)

### Aimbot

- Hotkey
- FOV
- Smooth
- Bone
- Range
- Visible only
- Players/NPCs
- Team check
- FOV circle
- Moves a virtual mouse, the game sees normal mouse input

### Hits

- Hitmarker
- Hitsound (your own `hitsound.wav`)
- Hit log with the body part
- Bullet tracers, yours and others'

### Misc

- Stream mode: nothing drawn or played that a recording would pick up
- Status line

### Unsafe

> [!WARNING]
> These features write to game memory, some patch the game's code, and might get you banned.

- Silent aim
  - Camera method, or magic bullet
  - Hit chance per body part
  - Head whenever it's visible
  - Prediction
- Magic bullet: through walls as far as the server's fire origin limit allows, through anything when you host
- Ragebot: silent aim at everyone in sight, fires by itself, no spread or recoil
- Anti-aim: pitch (down, up, jitter) and upper-body yaw (backwards, spin, jitter) for everyone else, your own view stays
- Melee aura: your fists hit the nearest head, with auto punch
- Chams on players, NPCs and dead bodies, glowing chrome on your own weapon and arms, rainbow mode
- No recoil, no spread, no sway, zoom while aiming
- Third person, also where the server turned it off
- World: time of day, sky and ground light, fog, weather, color grading, bloom, eight presets

<br>

## FAQ

<details>
<summary>Where are my settings saved?</summary>

`$XDG_CONFIG_HOME/goretux`, or `~/.config/goretux` when that isn't set: `settings.txt`, the aimbot calibration `aim.txt`, and your `hitsound.wav`.
With no home directory at all, next to the binary.

</details>

<br>

<details>
<summary>Which desktop environments and window managers are supported?</summary>

**Tested:**

- i3 with picom, on X11

**Should work** (the overlay is a transparent, click-through, override-redirect X11 window, the technique other external overlays use there too):

- KDE (KWin), X11 or Wayland
- GNOME (Mutter), X11 or Wayland
- Other X11 window managers with a compositor
- Hyprland, with a window rule (see [compatibility.md](compatibility.md#hyprland))

On Wayland the game has to run through XWayland, which is Proton's default.

</details>

<br>

<details>
<summary>How do I get a hitsound?</summary>

Put a `hitsound.wav` in the settings folder above. It's played with `pw-play`, `paplay` or `aplay`, whichever your system has.

</details>

<br>

<details>
<summary>It says it can't read the game</summary>

Your kernel restricts `ptrace`. Run `./setup.sh` again or see [compatibility.md](compatibility.md#ptrace).

</details>

<br>

<details>
<summary>My screen/overlay is black</summary>

The overlay is drawn with transparency, which needs a running compositor.

KDE on X11: turn compositing on under **System Settings → Display and Monitor → Compositor**.
picom: see [compatibility.md](compatibility.md#picom), fullscreen games need one setting changed.

</details>

<br>

<details>
<summary>The game looks blurry with the overlay on</summary>

Your compositor blurs whatever is behind transparent windows. Turn blur off for the overlay, see [compatibility.md](compatibility.md#picom) (picom) or [compatibility.md](compatibility.md#hyprland) (Hyprland).

</details>

<br>

<details>
<summary>The overlay doesn't show up</summary>

If it says the game's window isn't on this X server: the game runs where X can't see it, in gamescope or on Wine's Wayland driver. Run it without gamescope, on XWayland.

Otherwise the window manager is keeping it from being placed or raised over the game.

</details>

<br>

<details>
<summary>The aimbot, ragebot or auto punch does nothing</summary>

They move and click a virtual mouse through `/dev/uinput`. goretux prints a warning at startup when it can't open it: run `./setup.sh` and log in again.

</details>

<br>

<details>
<summary>A feature stopped working after a GoreBox update</summary>

Some features depend on the game's obfuscated names or code, which change with updates. Each one fails on its own and the rest keeps working; most say what's missing in the status line or the menu.

</details>
