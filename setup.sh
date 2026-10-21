#!/usr/bin/env bash
# one-time setup. run it as your normal user, it asks for sudo where it needs it.
#   - uinput: the virtual mouse behind the aimbot, the ragebot's trigger and auto punch
#   - the input group: access to /dev/uinput, and to your mouse while the menu is open
#   - ptrace: only if your kernel stops goretux from reading the game

UDEV_RULE_FILE="/etc/udev/rules.d/99-goretux-uinput.rules"
INPUT_GROUP="input"
CURRENT_USER=$(id -un)

if [ "$(id -u)" -eq 0 ]; then
  echo "run this as your normal user, not with sudo (it asks for sudo itself)"
  exit 1
fi

if [ -e /etc/NIXOS ]; then
  echo "NixOS manages /etc itself, set this up in configuration.nix instead, see compatibility.md"
  exit 1
fi

sudo modprobe uinput
echo "uinput" | sudo tee /etc/modules-load.d/uinput.conf > /dev/null
echo "uinput loads at boot: /etc/modules-load.d/uinput.conf"

if ! getent group "$INPUT_GROUP" > /dev/null; then
  if grep -qE "^$INPUT_GROUP:" /usr/lib/group 2> /dev/null; then
    # Fedora Atomic and other image-based systems keep system groups in /usr/lib/group
    grep -E "^$INPUT_GROUP:" /usr/lib/group | sudo tee -a /etc/group > /dev/null
  else
    sudo groupadd --system "$INPUT_GROUP"
  fi
  echo "created group $INPUT_GROUP"
fi

echo "KERNEL==\"uinput\", MODE=\"0660\", GROUP=\"$INPUT_GROUP\", OPTIONS+=\"static_node=uinput\"" | sudo tee "$UDEV_RULE_FILE" > /dev/null
echo "created udev rule: $UDEV_RULE_FILE"

sudo usermod -aG "$INPUT_GROUP" "$CURRENT_USER"
echo "added user $CURRENT_USER to group $INPUT_GROUP"

sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --sysname-match=uinput
sudo udevadm settle
echo "reloaded udev rules"

SCOPE=$(cat /proc/sys/kernel/yama/ptrace_scope 2> /dev/null || echo 0)
case "$SCOPE" in
  0) ;;
  3)
    echo "kernel.yama.ptrace_scope is 3: no process can read another one's memory until you reboot with a lower value."
    echo "see compatibility.md (ptrace)."
    ;;
  *)
    echo "kernel.yama.ptrace_scope is $SCOPE, which can stop goretux from reading the game."
    echo "lowering it to 0 lets your programs read your other programs' memory (the default on many distros)."
    read -r -p "lower it to 0, now and at every boot? [y/N] " answer
    if [[ "$answer" =~ ^[Yy]$ ]]; then
      echo "kernel.yama.ptrace_scope = 0" | sudo tee /etc/sysctl.d/10-goretux-ptrace.conf > /dev/null
      sudo sysctl -q -w kernel.yama.ptrace_scope=0
      echo "set kernel.yama.ptrace_scope = 0 (/etc/sysctl.d/10-goretux-ptrace.conf)"
    else
      echo "left as is, see compatibility.md (ptrace) for the other way"
    fi
    ;;
esac

if [ "${XDG_CURRENT_DESKTOP:-}" = "Hyprland" ]; then
  echo "detected Hyprland as window manager"

  RULE='
-- goretux overlay no_blur windowrule
hl.window_rule({
    match = {
        title = "^(goretux)$",
    },
    no_blur = true,
})'

  LUA_FILE="$HOME/.config/hypr/hyprland.lua"

  if [ ! -f "$LUA_FILE" ]; then
    echo "no $LUA_FILE, add this to hyprland.conf yourself:"
    echo '  windowrule = no_blur 1, match:title ^(goretux)$'
  elif grep -Fq 'title = "^(goretux)$"' "$LUA_FILE"; then
    echo "goretux no_blur windowrule has already been added, skipping"
  else
    echo "$RULE" >> "$LUA_FILE"
    echo "added no_blur windowrule to Hyprland"
  fi
fi

echo
echo "done. log out and back in (or restart) so the group change takes effect."
