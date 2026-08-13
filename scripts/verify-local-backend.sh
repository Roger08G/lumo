#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "$script_dir/.." && pwd)"
cargo_script="$script_dir/cargo-lumo.sh"

cd "$project_root"

windows_shell=false
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) windows_shell=true ;;
esac

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
    if $windows_shell && ! command -v cargo >/dev/null 2>&1 && [[ -x /c/.android/cargo/bin/cargo.exe ]]; then
        export CARGO_TARGET_DIR='C:\.android\lumo-target'
        binary_directory='/c/.android/lumo-target/release'
    else
        binary_directory="$project_root/target/release"
    fi
else
    binary_directory="$CARGO_TARGET_DIR/release"
    if $windows_shell && command -v cygpath >/dev/null 2>&1; then
        binary_directory="$(cygpath -u "$binary_directory")"
    elif [[ "$binary_directory" != /* ]]; then
        binary_directory="$project_root/$binary_directory"
    fi
fi

"$cargo_script" fmt --all -- --check
"$cargo_script" check --workspace --all-targets --all-features --locked
"$cargo_script" clippy --workspace --all-targets --all-features --locked -- -D warnings
"$cargo_script" test --workspace --all-targets --all-features --locked
"$cargo_script" build --workspace --all-targets --all-features --locked
"$cargo_script" build -p lumo-runtime --bins --release --features local-tools --locked
"$cargo_script" build -p lumo-api --release --locked

binary_suffix=''
if $windows_shell; then
    binary_suffix='.exe'
fi

for binary in lumo-controller lumo-controlled lumo-debug; do
    "$binary_directory/$binary$binary_suffix" self-test
done

printf 'Lumo local backend verification passed.\n'
