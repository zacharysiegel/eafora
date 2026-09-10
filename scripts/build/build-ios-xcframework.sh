#!/usr/bin/env bash

# Builds target/uniffi/EaforaCore.xcframework from the ios/ffi crate, together with the Swift sources that
# call into it.
#
# Usage:
#   ./scripts/build/build-ios-xcframework.sh
#   ./scripts/build/build-ios-xcframework.sh --debug    (faster; unshippable)
#
# The ordering matters and is the reason this is a script. The bindgen reads the compiled archive to find
# the exported surface, so both slices must exist before it runs; the three generator invocations are
# separate because each emits one kind of file; and -create-xcframework refuses to overwrite, so the
# previous output has to go first.

set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
readonly DEVICE_TARGET="aarch64-apple-ios"
readonly SIMULATOR_TARGET="aarch64-apple-ios-sim"
readonly LIBRARY_FILENAME="libeafora_core.a"
readonly MODULE_NAME="EaforaCore"
readonly OUTPUT_DIR="${REPO_ROOT}/target/uniffi"
readonly XCFRAMEWORK_PATH="${OUTPUT_DIR}/${MODULE_NAME}.xcframework"
readonly HEADERS_DIR="${OUTPUT_DIR}/headers"
readonly SWIFT_SOURCES_DIR="${OUTPUT_DIR}/swift"

CARGO_PROFILE_FLAG="--release"
CARGO_PROFILE_DIR="release"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug)
            CARGO_PROFILE_FLAG=""
            CARGO_PROFILE_DIR="debug"
            shift
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 64
            ;;
    esac
done

readonly CARGO_PROFILE_FLAG
readonly CARGO_PROFILE_DIR

cd "${REPO_ROOT}"

function require_rust_target {
    local rust_target="$1"

    local installed_target_count
    installed_target_count=$(rustup target list --installed | grep -cx "${rust_target}" || true)

    if test "${installed_target_count}" -eq 0; then
        echo "the ${rust_target} Rust target is required" >&2
        echo "  install: rustup target add ${rust_target}    (or run ./setup.sh)" >&2
        exit 1
    fi
}

function build_slice {
    local rust_target="$1"

    echo "building the ${rust_target} slice"
    cargo build -p ffi --target "${rust_target}" ${CARGO_PROFILE_FLAG}
}

# One invocation per kind of file, which is the only shape the generator offers.
function generate_swift_bindings {
    local archive_path="$1"

    echo "generating Swift bindings from ${archive_path#"${REPO_ROOT}/"}"
    rm -rf "${HEADERS_DIR}" "${SWIFT_SOURCES_DIR}"
    mkdir -p "${HEADERS_DIR}" "${SWIFT_SOURCES_DIR}"

    cargo run --quiet -p uniffi_bindgen_swift -- "${archive_path}" "${SWIFT_SOURCES_DIR}" --swift-sources
    cargo run --quiet -p uniffi_bindgen_swift -- "${archive_path}" "${HEADERS_DIR}" --headers
    cargo run --quiet -p uniffi_bindgen_swift -- "${archive_path}" "${HEADERS_DIR}" \
        --modulemap --xcframework --module-name "${MODULE_NAME}" --modulemap-filename module.modulemap
}

function combine_slices {
    local device_archive="$1"
    local simulator_archive="$2"

    echo "combining both slices into ${XCFRAMEWORK_PATH#"${REPO_ROOT}/"}"
    rm -rf "${XCFRAMEWORK_PATH}"

    xcodebuild -create-xcframework \
        -library "${device_archive}" -headers "${HEADERS_DIR}" \
        -library "${simulator_archive}" -headers "${HEADERS_DIR}" \
        -output "${XCFRAMEWORK_PATH}"
}

require_rust_target "${DEVICE_TARGET}"
require_rust_target "${SIMULATOR_TARGET}"

build_slice "${DEVICE_TARGET}"
build_slice "${SIMULATOR_TARGET}"

readonly DEVICE_ARCHIVE="${REPO_ROOT}/target/${DEVICE_TARGET}/${CARGO_PROFILE_DIR}/${LIBRARY_FILENAME}"
readonly SIMULATOR_ARCHIVE="${REPO_ROOT}/target/${SIMULATOR_TARGET}/${CARGO_PROFILE_DIR}/${LIBRARY_FILENAME}"

generate_swift_bindings "${SIMULATOR_ARCHIVE}"
combine_slices "${DEVICE_ARCHIVE}" "${SIMULATOR_ARCHIVE}"

echo
echo "${XCFRAMEWORK_PATH#"${REPO_ROOT}/"}: $(ls "${XCFRAMEWORK_PATH}" | tr '\n' ' ')"
echo "${SWIFT_SOURCES_DIR#"${REPO_ROOT}/"}: $(ls "${SWIFT_SOURCES_DIR}" | tr '\n' ' ')"
