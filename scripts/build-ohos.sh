#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: %s <arm64-v8a|x86_64> [build-dir]\n' "$0" >&2
  printf 'Requires OHOS_NDK_HOME, cargo, rustup, and the matching installed Rust target.\n' >&2
}

if [[ $# -lt 1 || $# -gt 2 ]]; then usage; exit 2; fi
arch="$1"
case "$arch" in
  arm64-v8a) rust_target=aarch64-linux-ohos ;;
  x86_64) rust_target=x86_64-linux-ohos ;;
  *) usage; printf 'Unsupported architecture: %s\n' "$arch" >&2; exit 2 ;;
esac

if [[ -z "${OHOS_NDK_HOME:-}" ]]; then
  printf 'OHOS_NDK_HOME is not set; point it to an installed OpenHarmony SDK/NDK.\n' >&2
  exit 1
fi
for tool in cargo rustup cmake; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Required tool not found in PATH: %s\n' "$tool" >&2
    exit 1
  fi
done
toolchain="$OHOS_NDK_HOME/native/build/cmake/ohos.toolchain.cmake"
if [[ ! -f "$toolchain" ]]; then
  printf 'OpenHarmony CMake toolchain not found: %s\n' "$toolchain" >&2
  exit 1
fi
if ! rustup target list --installed | grep -Fxq "$rust_target"; then
  printf 'Rust target is not installed: %s (install it with rustup before building)\n' "$rust_target" >&2
  exit 1
fi

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd -- "$script_dir/.." && pwd)"
build_dir="${2:-$project_dir/build-ohos-$arch}"
mkdir -p "$build_dir"

# Rust invokes this wrapper with the compiler and all flags as separate args.
# The wrapper adds only Cargo's requested target and forwards everything else.
linker_dir="$build_dir/linker"
mkdir -p "$linker_dir"
cat > "$linker_dir/rust-clang" <<'WRAPPER'
#!/usr/bin/env bash
set -euo pipefail
if [[ -z "${OHOS_NDK_HOME:-}" || -z "${OHOS_RUST_TARGET:-}" ]]; then
  echo 'OHOS_NDK_HOME and OHOS_RUST_TARGET are required by rust-clang' >&2
  exit 1
fi
case "$OHOS_RUST_TARGET" in
  aarch64-linux-ohos) target=aarch64-linux-ohos ;;
  x86_64-linux-ohos) target=x86_64-linux-ohos ;;
  *) echo "Unsupported Rust target: $OHOS_RUST_TARGET" >&2; exit 1 ;;
esac
compiler="${OHOS_NDK_HOME}/llvm/bin/clang"
if [[ ! -x "$compiler" ]]; then
  echo "OpenHarmony clang not found or not executable: $compiler" >&2
  exit 1
fi
exec "$compiler" "--target=$target" "$@"
WRAPPER
chmod +x "$linker_dir/rust-clang"

export OHOS_RUST_TARGET="$rust_target"
export CARGO_TARGET_AARCH64_LINUX_OHOS_LINKER="$linker_dir/rust-clang"
export CARGO_TARGET_X86_64_LINUX_OHOS_LINKER="$linker_dir/rust-clang"
export CARGO_TARGET_AARCH64_LINUX_OHOS_AR="$OHOS_NDK_HOME/llvm/bin/llvm-ar"
export CARGO_TARGET_X86_64_LINUX_OHOS_AR="$OHOS_NDK_HOME/llvm/bin/llvm-ar"

# Build the Rust static library explicitly; CMake consumes this exact archive.
(
  cd "$project_dir"
  cargo build --locked --release --target "$rust_target"
)

cmake -S "$project_dir" -B "$build_dir" \
  -DCMAKE_TOOLCHAIN_FILE="$toolchain" \
  -DOHOS_NDK_HOME="$OHOS_NDK_HOME" \
  -DOHOS_ARCH="$arch" \
  -DRUST_TARGET="$rust_target" \
  -DBUILD_TESTING=ON
cmake --build "$build_dir" --target xiaohong_perception ffi_smoke --parallel
printf 'Built libxiaohong_perception.so and ffi_smoke for %s in %s\n' "$arch" "$build_dir"
printf 'Cross-build completed; this does not imply device validation.\n'