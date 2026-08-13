#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        if command -v powershell.exe >/dev/null 2>&1; then
            powershell_script="$script_dir/cargo-lumo.ps1"
            if command -v cygpath >/dev/null 2>&1; then
                powershell_script="$(cygpath -w "$powershell_script")"
            fi
            exec powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$powershell_script" "$@"
        fi
        ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
    printf 'Cargo is not installed or available in PATH.\n' >&2
    exit 127
fi

exec cargo "$@"
