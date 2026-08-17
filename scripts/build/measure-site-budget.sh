#!/usr/bin/env bash
# measure-site-budget.sh: report what a first-time visitor downloads from a built site tree, against the
# first-paint and second-paint perf-budget targets.
#
# First paint is the WASM bundle, the wasm-bindgen JS shim, the bundled CSS, the page-shell HTML, and the
# embedded artifact bundle. Second paint adds the live-bundle files the client fetches on its first online
# connection: the geometry shard and every statistic shard the live manifest lists.
#
# Sizes are brotli -q 11 computed here and streamed to stdout, which leaves the tree untouched. This is a
# stated approximation of transfer size rather than a measurement of it: the CDN compresses responses with
# its own settings, which we neither control nor can read from a built tree.
#
# The targets are targets, not contracts, so no measurement outcome reaches the exit code: an overage, a
# failed build, and a tree missing the files to measure all print their verdict and exit zero. Only the two
# checks that run before any measurement exit non-zero, because neither is a fact about the site: an
# unrecognized argument, and a missing program.
#
# Usage:
#   ./scripts/build/measure-site-budget.sh [--no-build]
# e.g.
#   ./scripts/build/measure-site-budget.sh
#   ./scripts/build/measure-site-budget.sh --no-build
#
# Behavior:
#   1. Runs ./scripts/build/build-site.sh first unless --no-build is passed, so the tree measured is a
#      release build with its shell document. That output goes to stderr so stdout carries only the report.
#      With --no-build the report describes whatever is in the tree, which may be a debug build.
#   2. Reads the site root and the pkg subdirectory from web/Cargo.toml so no path constant is duplicated.
#   3. Resolves the live bundle the way the client does: the repository base the built discovery document
#      names, then that base's latest/manifest.json, then the version that manifest names.
#   4. Reports any total whose parts the tree cannot supply as unmeasured, never as a smaller number, and
#      explains each gap under Notes. A component matching more than one file is a gap too, since which
#      file a visitor fetches is then unknown.
#   5. Marks a totals line at or above 90% of its cap " near cap", and one over its cap "*** OVER CAP ***"
#      followed by a warning line.

set -euo pipefail

readonly FIRST_PAINT_CAP_BYTES=2000000
readonly SECOND_PAINT_CAP_BYTES=3000000
readonly NEAR_CAP_PERCENT=90
readonly UNMEASURED_VALUE="n/a"
readonly EMBEDDED_SUBDIR="embedded_artifacts"
readonly DISCOVERY_FILENAME="discovery"
readonly SHELL_DOCUMENT_NAME="index.html"
readonly LATEST_MANIFEST_RELATIVE_PATH="latest/manifest.json"

function required_program {
    local program_name="$1"
    local install_hint="$2"

    if ! which "$program_name" 1>/dev/null 2>&1; then
        echo "The \`$program_name\` program is required" >&2
        echo "  install: $install_hint" >&2
        exit 1
    fi
}

RUN_BUILD=1

while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-build)
            RUN_BUILD=0
            shift
            ;;
        *)
            echo "usage: $0 [--no-build]" >&2
            exit 64
            ;;
    esac
done

required_program "brotli" "brew install brotli"
required_program "jq"     "brew install jq"

if [[ "$RUN_BUILD" -eq 1 ]]; then
    required_program "cargo-leptos" "cargo install --locked cargo-leptos"
fi

function exit_zero_after_reporting_failure {
    local exit_status="$?"

    if [[ "$exit_status" -ne 0 ]]; then
        echo "" >&2
        echo "note: the report above is incomplete; a command failed with status $exit_status" >&2
    fi

    exit 0
}
trap exit_zero_after_reporting_failure EXIT

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

function get_leptos_metadata_value {
    local key_name="$1"

    grep -E "^$key_name[[:space:]]*=" "$REPO_ROOT/web/Cargo.toml" \
        | head -1 \
        | sed -E 's/^[^=]*=[[:space:]]*"(.*)".*$/\1/'
}

function to_repo_relative_path {
    local absolute_path="$1"

    if [[ "$absolute_path" == "$REPO_ROOT"/* ]]; then
        echo "./${absolute_path#$REPO_ROOT/}"
    else
        echo "$absolute_path"
    fi
}

SITE_DIR="$REPO_ROOT/$(get_leptos_metadata_value "site-root")"
PKG_DIR="$SITE_DIR/$(get_leptos_metadata_value "site-pkg-dir")"
EMBEDDED_DIR="$SITE_DIR/$EMBEDDED_SUBDIR"
DISCOVERY_PATH="$SITE_DIR/$DISCOVERY_FILENAME"
SHELL_PATH="$SITE_DIR/$SHELL_DOCUMENT_NAME"

if [[ "$RUN_BUILD" -eq 1 ]]; then
    "$REPO_ROOT/scripts/build/build-site.sh" 1>&2
fi

if [[ ! -d "$PKG_DIR" ]]; then
    echo "error: $(to_repo_relative_path "$PKG_DIR") does not exist; run 'cargo leptos build --release' first" >&2
    exit 0
fi

function sum_brotli_size_of_stream {
    local total_bytes=0
    local file_path

    while IFS= read -r -d '' file_path; do
        total_bytes=$((total_bytes + $(brotli -q 11 -c "$file_path" | wc -c)))
    done

    echo "$total_bytes"
}

function sum_brotli_size_of_relative_paths {
    local base_dir="$1"
    local total_bytes=0
    local relative_path

    while IFS= read -r relative_path; do
        if [[ -z "$relative_path" ]]; then
            continue
        fi

        total_bytes=$((total_bytes + $(brotli -q 11 -c "$base_dir/$relative_path" | wc -c)))
    done

    echo "$total_bytes"
}

function find_first_absent_relative_path {
    local base_dir="$1"
    local relative_path

    while IFS= read -r relative_path; do
        if [[ -z "$relative_path" ]]; then
            continue
        fi

        if [[ ! -f "$base_dir/$relative_path" ]]; then
            echo "$relative_path"
            return 0
        fi
    done

    return 0
}

# A .br or .gz sibling is the same bytes measured twice, and .DS_Store is Finder metadata no client fetches.
function sum_brotli_size_of_fetched_files_in {
    local directory="$1"

    find "$directory" -type f ! -name '*.br' ! -name '*.gz' ! -name '.DS_Store' -print0 | sum_brotli_size_of_stream
}

# Megabytes are decimal so the printed components sum to the printed total: 612 KB + 38 KB + ... reads as
# 1.42 MB only under 1 MB = 1000 KB.
function format_megabytes {
    local byte_count="$1"
    local hundredths_of_a_megabyte=$(((byte_count + 5000) / 10000))

    printf '%d.%02d MB' "$((hundredths_of_a_megabyte / 100))" "$((hundredths_of_a_megabyte % 100))"
}

function format_size {
    local byte_count="$1"
    local rounded_kilobytes=$(((byte_count + 500) / 1000))

    if [[ "$rounded_kilobytes" -ge 1000 ]]; then
        format_megabytes "$byte_count"
    elif [[ "$byte_count" -ge 1000 ]]; then
        printf '%d KB' "$rounded_kilobytes"
    else
        printf '%d B' "$byte_count"
    fi
}

function print_total_line {
    local label="$1"
    local total_bytes="$2"
    local cap_bytes="$3"
    local percent_of_cap=$(((total_bytes * 100 + cap_bytes / 2) / cap_bytes))
    local cap_suffix=""

    if [[ "$total_bytes" -gt "$cap_bytes" ]]; then
        cap_suffix="  *** OVER CAP ***"
        OVER_CAP_LABELS="${OVER_CAP_LABELS}${label} "
    elif [[ $((total_bytes * 100)) -ge $((cap_bytes * NEAR_CAP_PERCENT)) ]]; then
        cap_suffix="  near cap"
    fi

    printf '%-14s%s / %s  (%d%%)%s\n' \
        "$label" \
        "$(format_megabytes "$total_bytes")" \
        "$(format_megabytes "$cap_bytes")" \
        "$percent_of_cap" \
        "$cap_suffix"
}

function print_unmeasured_total_line {
    local label="$1"

    printf '%-14s%s\n' "$label" "not measured; see notes"
}

function print_component_line {
    local label="$1"
    local value="$2"

    printf '  %-18s %9s\n' "$label" "$value"
}

OVER_CAP_LABELS=""
FIRST_PAINT_UNAVAILABLE_REASON=""

function note_first_paint_gap {
    local reason="$1"

    if [[ -z "$FIRST_PAINT_UNAVAILABLE_REASON" ]]; then
        FIRST_PAINT_UNAVAILABLE_REASON="$reason"
    fi
}

# Sets MEASURED_COMPONENT_BYTES to the size of the single file a visitor fetches for this component, or
# records why it cannot be measured and leaves it 0. Two matches is as unmeasurable as none: with
# content-hashed names a leftover copy from an earlier build would be summed in as bytes nobody downloads.
# Assigns to globals rather than echoing a value because a command substitution would run it in a subshell,
# where the recorded reason would be discarded.
function measure_single_pkg_file {
    local label="$1"
    local name_glob="$2"
    local match_count
    match_count="$(find "$PKG_DIR" -type f -name "$name_glob" | wc -l | tr -d ' ')"

    MEASURED_COMPONENT_BYTES=0
    MEASURED_COMPONENT_DISPLAY="$UNMEASURED_VALUE"

    if [[ "$match_count" -eq 0 ]]; then
        note_first_paint_gap "$(to_repo_relative_path "$PKG_DIR") holds no $label"
        return 0
    fi

    if [[ "$match_count" -gt 1 ]]; then
        note_first_paint_gap "$(to_repo_relative_path "$PKG_DIR") holds $match_count files matching $name_glob, so the $label a visitor fetches is ambiguous; ./scripts/build/build-site.sh empties the site root first"
        return 0
    fi

    MEASURED_COMPONENT_BYTES="$(brotli -q 11 -c "$(find "$PKG_DIR" -type f -name "$name_glob")" | wc -c | tr -d ' ')"
    MEASURED_COMPONENT_DISPLAY="$(format_size "$MEASURED_COMPONENT_BYTES")"
}

measure_single_pkg_file "wasm bundle" '*.wasm'
WASM_BYTES="$MEASURED_COMPONENT_BYTES"
WASM_DISPLAY="$MEASURED_COMPONENT_DISPLAY"

measure_single_pkg_file "js shim" '*.js'
JS_SHIM_BYTES="$MEASURED_COMPONENT_BYTES"
JS_SHIM_DISPLAY="$MEASURED_COMPONENT_DISPLAY"

measure_single_pkg_file "stylesheet" '*.css'
CSS_BYTES="$MEASURED_COMPONENT_BYTES"
CSS_DISPLAY="$MEASURED_COMPONENT_DISPLAY"

HTML_BYTES=0

if [[ -f "$SHELL_PATH" ]]; then
    HTML_BYTES="$(brotli -q 11 -c "$SHELL_PATH" | wc -c | tr -d ' ')"
else
    note_first_paint_gap "the site tree has no shell document at $(to_repo_relative_path "$SHELL_PATH"); run ./scripts/build/build-site.sh, which writes it after the build"
fi

EMBEDDED_BYTES=0
EMBEDDED_UNAVAILABLE_REASON=""

if [[ -d "$EMBEDDED_DIR" ]]; then
    EMBEDDED_BYTES="$(sum_brotli_size_of_fetched_files_in "$EMBEDDED_DIR")"
else
    EMBEDDED_UNAVAILABLE_REASON="$(to_repo_relative_path "$EMBEDDED_DIR") is absent; run scripts/build/sync-embedded-bundle.sh ./web/static/$EMBEDDED_SUBDIR before the build"
    note_first_paint_gap "$EMBEDDED_UNAVAILABLE_REASON"
fi

FIRST_PAINT_BYTES=$((WASM_BYTES + JS_SHIM_BYTES + CSS_BYTES + HTML_BYTES + EMBEDDED_BYTES))

LIVE_UNAVAILABLE_REASON=""
LIVE_GEOMETRY_BYTES=0
LIVE_SHARD_BYTES=0
LIVE_POINTER_BYTES=0

# Sets the LIVE_* globals above. The tree can hold several published versions at once; only the one that
# latest/manifest.json names is ever fetched, so the manifest picks the version rather than the filesystem.
function measure_live_bundle {
    if [[ ! -f "$DISCOVERY_PATH" ]]; then
        LIVE_UNAVAILABLE_REASON="the built site tree has no discovery document at $(to_repo_relative_path "$DISCOVERY_PATH")"
        return 0
    fi

    local repository_base_url
    repository_base_url="$(jq -r '.repository_base_url' "$DISCOVERY_PATH")"

    if [[ "$repository_base_url" != /* ]]; then
        LIVE_UNAVAILABLE_REASON="the discovery document points the live bundle at $repository_base_url, which this site tree does not serve"
        return 0
    fi

    local repository_dir="$SITE_DIR${repository_base_url%/}"
    local latest_manifest_path="$repository_dir/$LATEST_MANIFEST_RELATIVE_PATH"

    if [[ ! -f "$latest_manifest_path" ]]; then
        LIVE_UNAVAILABLE_REASON="the live bundle pointer $(to_repo_relative_path "$latest_manifest_path") is absent"
        return 0
    fi

    local live_version
    live_version="$(jq -r '.version' "$latest_manifest_path")"

    local version_dir="$repository_dir/$live_version"

    if [[ ! -d "$version_dir" ]]; then
        LIVE_UNAVAILABLE_REASON="the live manifest names version $live_version but $(to_repo_relative_path "$version_dir") is absent"
        return 0
    fi

    local geometry_relative_paths
    local shard_relative_paths
    geometry_relative_paths="$(jq -r '.geometry.relative_path' "$latest_manifest_path")"
    shard_relative_paths="$(jq -r '.statistics[][].relative_path' "$latest_manifest_path")"

    local absent_relative_path
    absent_relative_path="$(printf '%s\n%s\n' "$geometry_relative_paths" "$shard_relative_paths" \
        | find_first_absent_relative_path "$version_dir")"

    if [[ -n "$absent_relative_path" ]]; then
        LIVE_UNAVAILABLE_REASON="the live manifest lists $absent_relative_path, which is absent from $(to_repo_relative_path "$version_dir")"
        return 0
    fi

    LIVE_GEOMETRY_BYTES="$(printf '%s\n' "$geometry_relative_paths" | sum_brotli_size_of_relative_paths "$version_dir")"
    LIVE_SHARD_BYTES="$(printf '%s\n' "$shard_relative_paths" | sum_brotli_size_of_relative_paths "$version_dir")"
    LIVE_POINTER_BYTES=$(($(brotli -q 11 -c "$DISCOVERY_PATH" | wc -c) + $(brotli -q 11 -c "$latest_manifest_path" | wc -c)))
}
measure_live_bundle

if [[ -n "$FIRST_PAINT_UNAVAILABLE_REASON" ]]; then
    print_unmeasured_total_line "First paint:"
else
    print_total_line "First paint:" "$FIRST_PAINT_BYTES" "$FIRST_PAINT_CAP_BYTES"
fi

print_component_line "wasm" "$WASM_DISPLAY"
print_component_line "js shim" "$JS_SHIM_DISPLAY"
print_component_line "css" "$CSS_DISPLAY"

if [[ -f "$SHELL_PATH" ]]; then
    print_component_line "html shell" "$(format_size "$HTML_BYTES")"
else
    print_component_line "html shell" "$UNMEASURED_VALUE"
fi

if [[ -n "$EMBEDDED_UNAVAILABLE_REASON" ]]; then
    print_component_line "embedded artifacts" "$UNMEASURED_VALUE"
else
    print_component_line "embedded artifacts" "$(format_size "$EMBEDDED_BYTES")"
fi

echo ""

if [[ -n "$FIRST_PAINT_UNAVAILABLE_REASON" || -n "$LIVE_UNAVAILABLE_REASON" ]]; then
    print_unmeasured_total_line "Second paint:"
else
    print_total_line "Second paint:" "$((FIRST_PAINT_BYTES + LIVE_GEOMETRY_BYTES + LIVE_SHARD_BYTES))" "$SECOND_PAINT_CAP_BYTES"
fi

if [[ -n "$LIVE_UNAVAILABLE_REASON" ]]; then
    print_component_line "+ geometry" "$UNMEASURED_VALUE"
    print_component_line "+ statistic shards" "$UNMEASURED_VALUE"
else
    print_component_line "+ geometry" "$(format_size "$LIVE_GEOMETRY_BYTES")"
    print_component_line "+ statistic shards" "$(format_size "$LIVE_SHARD_BYTES")"
fi

echo ""

if [[ -n "$OVER_CAP_LABELS" ]]; then
    for over_cap_label in $OVER_CAP_LABELS; do
        echo "WARNING: ${over_cap_label%:} is over its cap."
    done
    echo "The caps are targets to be read by a person, so this is a warning and never a build failure."
    echo ""
fi

echo "Notes:"

if [[ -n "$FIRST_PAINT_UNAVAILABLE_REASON" ]]; then
    echo "  - First paint is unmeasured: $FIRST_PAINT_UNAVAILABLE_REASON."
fi

if [[ -n "$LIVE_UNAVAILABLE_REASON" ]]; then
    echo "  - Second paint is unmeasured: $LIVE_UNAVAILABLE_REASON."
else
    echo "  - Second paint counts the geometry shard and the statistic shards, not the discovery document"
    echo "    and latest/manifest.json the client fetches to find them ($(format_size "$LIVE_POINTER_BYTES") combined)."
fi

echo "  - Sizes are brotli -q 11 over the built tree. Decimal megabytes: 1 MB is 1000 KB."
