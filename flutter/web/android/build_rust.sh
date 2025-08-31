#!/usr/bin/env bash
# Build the monero_wasm Rust native library for Android targets.
#
# This script detects the Android NDK, adds its toolchain to PATH,
# then cross-compiles for aarch64, armv7, and x86_64.
# The resulting .so files are copied into src/main/jniLibs/<abi>/.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MANIFEST="${SCRIPT_DIR}/../native/monero_wasm/Cargo.toml"
JNILIBS_DIR="${SCRIPT_DIR}/app/src/main/jniLibs"

# --- Detect NDK path ---
if [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
  NDK_PATH="$ANDROID_NDK_HOME"
elif [[ -f "${SCRIPT_DIR}/local.properties" ]]; then
  NDK_PATH=$(grep '^ndk.dir=' "${SCRIPT_DIR}/local.properties" | cut -d'=' -f2 | tr -d '[:space:]')
elif [[ -n "${ANDROID_HOME:-}" ]]; then
  # Pick the highest installed NDK version.
  NDK_PATH=$(ls -d "${ANDROID_HOME}/ndk/"* 2>/dev/null | sort -V | tail -n1)
fi

if [[ -z "${NDK_PATH:-}" || ! -d "${NDK_PATH}" ]]; then
  echo "ERROR: Cannot find Android NDK." >&2
  echo "Set ANDROID_NDK_HOME, add ndk.dir to local.properties," >&2
  echo "or ensure ANDROID_HOME/ndk/ contains an installed NDK." >&2
  exit 1
fi

echo "Using NDK at: ${NDK_PATH}"

# Add NDK toolchain to PATH so cargo can find the linkers
# configured in native/monero_wasm/.cargo/config.toml.
HOST_TAG=""
case "$(uname -s)" in
  Linux*)  HOST_TAG="linux-x86_64" ;;
  Darwin*) HOST_TAG="darwin-x86_64" ;;
  *)       echo "ERROR: Unsupported host OS" >&2; exit 1 ;;
esac

TOOLCHAIN_BIN="${NDK_PATH}/toolchains/llvm/prebuilt/${HOST_TAG}/bin"
if [[ ! -d "${TOOLCHAIN_BIN}" ]]; then
  echo "ERROR: NDK toolchain not found at ${TOOLCHAIN_BIN}" >&2
  exit 1
fi
export PATH="${TOOLCHAIN_BIN}:${PATH}"

# --- Build targets ---
declare -A ABI_MAP=(
  ["aarch64-linux-android"]="arm64-v8a"
  ["armv7-linux-androideabi"]="armeabi-v7a"
  ["x86_64-linux-android"]="x86_64"
)

CARGO_PROFILE="${CARGO_PROFILE:-release}"
CARGO_FLAGS=""
if [[ "$CARGO_PROFILE" == "release" ]]; then
  CARGO_FLAGS="--release"
fi

for RUST_TARGET in "${!ABI_MAP[@]}"; do
  ABI="${ABI_MAP[$RUST_TARGET]}"
  echo "Building for ${RUST_TARGET} (${ABI})..."

  cargo build --manifest-path "${MANIFEST}" --target "${RUST_TARGET}" ${CARGO_FLAGS}

  # Copy the .so into jniLibs.
  SRC="${SCRIPT_DIR}/../target/${RUST_TARGET}/${CARGO_PROFILE}/libmonero_wasm.so"
  DEST="${JNILIBS_DIR}/${ABI}"
  mkdir -p "${DEST}"
  cp "${SRC}" "${DEST}/libmonero_wasm.so"
  echo "  -> ${DEST}/libmonero_wasm.so"
done

echo "Android Rust build complete."
