#!/usr/bin/env bash
# update, build and start goretux from this checkout. arguments go to goretux (try --help)

NEED_RUST="1.88.0"

cd -- "$(dirname -- "${BASH_SOURCE[0]}")" || exit 1

if [ -d .git ]; then
  git pull --ff-only || echo "couldn't update, building what's here"
else
  echo "not a git checkout, skipping the update (clone the repository with git to get updates)"
fi

if ! command -v cargo > /dev/null; then
  echo "cargo not found. install Rust with rustup:"
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

HAVE_RUST=$(rustc --version 2> /dev/null | awk '{print $2}')
if [ -n "$HAVE_RUST" ] && [ "$(printf '%s\n%s\n' "$NEED_RUST" "$HAVE_RUST" | sort -V | head -n 1)" != "$NEED_RUST" ]; then
  echo "Rust $HAVE_RUST is too old, goretux needs $NEED_RUST or newer: rustup update"
  exit 1
fi

if ! command -v c++ > /dev/null && ! command -v g++ > /dev/null && ! command -v clang++ > /dev/null; then
  echo "no C++ compiler found, Dear ImGui needs one to build. install one with:"
  # shellcheck disable=SC1091
  . /etc/os-release 2> /dev/null
  case " ${ID:-} ${ID_LIKE:-} " in
    *" arch "*) echo "  sudo pacman -S --needed base-devel" ;;
    *" debian "* | *" ubuntu "*) echo "  sudo apt install build-essential" ;;
    *" fedora "* | *" rhel "*) echo "  sudo dnf install gcc-c++" ;;
    *" suse "* | *" opensuse "*) echo "  sudo zypper install gcc-c++" ;;
    *" void "*) echo "  sudo xbps-install -S gcc" ;;
    *) echo "  your distribution's g++ or clang package" ;;
  esac
  exit 1
fi

cargo run --release -- "$@"
