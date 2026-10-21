# Operating Systems

## NixOS

`setup.sh` can't write to `/etc` on NixOS. Do the same in `configuration.nix`:

```nix
hardware.uinput.enable = true;

users.users.yourname = {
  isNormalUser = true;
  extraGroups = [ "wheel" "input" "uinput" ];
};
```

Then rebuild and reboot:

```bash
sudo nixos-rebuild switch
sudo reboot
```

goretux loads the X11 and OpenGL libraries at runtime, which aren't on NixOS's default library path. Build and run it from a shell that provides them (not tested by us):

```nix
# shell.nix
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ cargo rustc gcc ];
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
    libGL
    libxkbcommon
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
  ]);
}
```

```bash
nix-shell
cargo run --release
```

If goretux can't read the game, see [ptrace](#ptrace); on NixOS that's `boot.kernel.sysctl."kernel.yama.ptrace_scope" = 0;`.

<br>

## Fedora Atomic

Silverblue, Kinoite, Bazzite and friends keep the `input` group in `/usr/lib/group`. `setup.sh` copies it to `/etc/group` for you, or do it by hand:

```bash
grep -E '^input:' /usr/lib/group | sudo tee -a /etc/group
sudo usermod -aG input "$USER"
```

> **Restart your machine (required)**

There's no C++ compiler on the base image. Build inside a toolbox, which shares your home directory, then run the binary on the host:

```bash
toolbox create
toolbox enter
sudo dnf install gcc-c++
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo build --release
exit
./target/release/goretux
```

<br>

## Steam Deck / SteamOS

Game Mode runs games in gamescope, which goretux can't draw over (see [gamescope](#gamescope)). Desktop Mode is KDE Plasma and should work like any KDE desktop (not tested by us).

<br>
<br>

# Kernel

## ptrace

goretux reads the game through `/proc/<pid>/mem`, which the kernel only allows for processes you may `ptrace`. The Yama setting decides that:

```bash
cat /proc/sys/kernel/yama/ptrace_scope
```

- `0`: your programs may read your other programs. Works.
- `1`: only a program's parents may read it. Ubuntu and some others default to this. Whether it stops goretux depends on how Steam starts the game, so try first.
- `2`: only root may read other programs.
- `3`: nobody may, until a reboot with a lower value.

For `1` and `2`, `setup.sh` offers to set it to `0` now and at boot (`/etc/sysctl.d/10-goretux-ptrace.conf`). By hand:

```bash
echo "kernel.yama.ptrace_scope = 0" | sudo tee /etc/sysctl.d/10-goretux-ptrace.conf
sudo sysctl -w kernel.yama.ptrace_scope=0
```

If you'd rather keep the setting and allow only goretux, give the binary the capability instead. This has to be redone after every build, and the binary then ignores `LD_LIBRARY_PATH`, so it's not an option on NixOS:

```bash
cargo build --release
sudo setcap cap_sys_ptrace=eip target/release/goretux
./target/release/goretux
```

Don't run goretux as root instead: your settings would end up in root's home, and the X server may refuse root's overlay.

<br>

## Writes to the game's code

Chams, third person and anti-aim patch a few bytes of the game's code, which `/proc/<pid>/mem` can only do by forcing writes to read-only memory. Since Linux 6.12 the kernel can forbid that (`proc_mem.force_override=never` or `ptrace` on the kernel command line, or a kernel built that way). Those three features then fail with an error in the status line and everything else keeps working. Check with:

```bash
cat /proc/cmdline
```

<br>

## uinput

The aimbot, the ragebot's trigger and auto punch use a virtual mouse made through `/dev/uinput`. goretux warns at startup if it can't open it. `setup.sh` loads the module at boot and gives the `input` group access. To check:

```bash
ls -l /dev/uinput
groups
```

`/dev/uinput` should belong to group `input`, and `input` should be in your groups (after logging out and back in).

<br>
<br>

# Window Managers

## picom

Tested: i3 with picom.

Proton marks fullscreen games as "don't composite me", and with `unredir-if-possible` picom then stops compositing altogether, so the overlay turns black. picom also blurs everything behind transparent windows, which blurs the whole game behind the overlay. In `picom.conf`:

```conf
unredir-if-possible = false;
```

and, with picom 12 or newer:

```conf
rules = (
  { match = "class_g = 'goretux'"; blur-background = false; shadow = false; },
);
```

or, with older picom:

```conf
blur-background-exclude = [ "class_g = 'goretux'" ];
shadow-exclude = [ "class_g = 'goretux'" ];
```

Reload with `pkill -USR1 picom`.

<br>

## Hyprland

`setup.sh` appends a `no_blur` rule for the overlay when your Hyprland config is the Lua kind (`hyprland.lua`):

```lua
hl.window_rule({
	match = {
		title = "^(goretux)$",
	},
	no_blur = true,
})
```

With the legacy .conf configuration, add the rule to `hyprland.conf` yourself:

```conf
windowrule = no_blur 1, match:title ^(goretux)$
```

Hyprland's support for X11 overlays is limited, so it may need more tweaks (not tested by us).

<br>

## KDE and GNOME

KDE on X11 needs compositing on (**System Settings → Display and Monitor → Compositor**). GNOME always composites. On Wayland both run the overlay through XWayland (not tested by us).

<br>

## Wayland

goretux draws an X11 window, so a Wayland session needs XWayland (`DISPLAY` set), and the game has to run on XWayland too. That's Proton's default. If you turned on Wine's Wayland driver for GoreBox (for example with `PROTON_ENABLE_WAYLAND=1`), turn it off.

<br>
<br>

# Steam

## gamescope

Not supported. Games in gamescope draw to gamescope's own nested X server, which goretux's overlay can't reach. goretux says so after waiting 15 seconds for the game's window. Run GoreBox without gamescope.

<br>

## Proton and Flatpak

Tested with proton-cachyos. Other Proton builds (Experimental, GE) run the game the same way and should work. Steam from Flatpak should work too as long as the game shows up in `/proc` (not tested by us).
