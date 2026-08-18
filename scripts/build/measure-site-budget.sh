#!/usr/bin/env bash
# measure-site-budget.sh: report what a first-time visitor downloads from a built site tree, against the
# first-paint and second-paint perf-budget targets.
#
# The targets bound artifact bytes, not client code. First paint is the embedded bundle that paints the map
# immediately; second paint adds the live-bundle files the client fetches on its first online connection,
# namely the geometry shard and every statistic shard the live manifest lists. The wasm, the wasm-bindgen
# JS shim, the stylesheet, and the prerendered document are reported alongside for information, because their
# size is a consequence of the framework and the renderer rather than of a data decision.
#
# A file counts at its brotli -q 11 size when Cloudflare's content-type list says the edge compresses it,
# and at its full size otherwise, which is the case for the geometry and statistic shards. Compression is
# computed here and streamed to stdout, leaving the tree untouched. Still an approximation: the edge picks
# its own algorithm and quality per plan, which we cannot read from a built tree.
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
#      release build with its prerendered document. That output goes to stderr so stdout carries only the report.
#      With --no-build the report describes whatever is in the tree, which may be a debug build.
#   2. Reads the site root and the pkg subdirectory from web/Cargo.toml so no path constant is duplicated.
#   3. Resolves the live bundle the way the client does: the repository base the built discovery document
#      names, then that base's latest/manifest.json, then the version that manifest names.
#   4. Reports any total whose parts the tree cannot supply as unmeasured, never as a smaller number, and
#      explains each gap under Notes. A component matching more than one file is a gap too, since which
#      file a visitor fetches is then unknown. A gap in the client code leaves the artifact totals intact,
#      since they no longer share any component.
#   5. Marks a totals line at or above 90% of its cap " near cap", and one over its cap "*** OVER CAP ***"
#      followed by a warning line.

set -euo pipefail

readonly FIRST_PAINT_CAP_BYTES=2000000
readonly SECOND_PAINT_CAP_BYTES=8000000
readonly NEAR_CAP_PERCENT=90
readonly UNMEASURED_VALUE="n/a"
readonly EMBEDDED_SUBDIR="embedded_artifacts"
readonly DISCOVERY_FILENAME="discovery"
readonly DOCUMENT_NAME="index.html"
readonly LATEST_MANIFEST_RELATIVE_PATH="latest/manifest.json"
readonly LOCAL_REPOSITORY_SUBDIR="repository"

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
    local declared_value
    declared_value="$(grep -E "^$key_name[[:space:]]*=" "$REPO_ROOT/web/Cargo.toml" | head -1 || true)"

    if [[ -z "$declared_value" ]]; then
        echo "error: web/Cargo.toml declares no $key_name under [package.metadata.leptos]" >&2
        exit 1
    fi

    printf '%s' "$declared_value" | sed -E 's/^[^=]*=[[:space:]]*"(.*)".*$/\1/'
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
DOCUMENT_PATH="$SITE_DIR/$DOCUMENT_NAME"

if [[ "$RUN_BUILD" -eq 1 ]]; then
    "$REPO_ROOT/scripts/build/build-site.sh" 1>&2
fi

if [[ ! -d "$PKG_DIR" ]]; then
    echo "error: $(to_repo_relative_path "$PKG_DIR") does not exist; run 'cargo leptos build --release' first" >&2
    exit 0
fi

# Cloudflare compresses a response only when its content type is on a fixed list, so a file whose type is
# absent from that list transfers at full size however well it would have compressed. Keyed on extension
# because the extension is what decides the type, with the extensionless discovery document covered by the
# Content-Type web/static/_headers assigns it.
function is_compressed_in_transit {
    local file_path="$1"

    case "$file_path" in
        *.wasm|*.js|*.css|*.html|*.json|*/"$DISCOVERY_FILENAME")
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

function transfer_size_of {
    local file_path="$1"

    if is_compressed_in_transit "$file_path"; then
        brotli -q 11 -c "$file_path" | wc -c | tr -d ' '
    else
        wc -c < "$file_path" | tr -d ' '
    fi
}

function sum_transfer_size_of_stream {
    local total_bytes=0
    local file_path

    while IFS= read -r -d '' file_path; do
        total_bytes=$((total_bytes + $(transfer_size_of "$file_path")))
    done

    echo "$total_bytes"
}

function sum_transfer_size_of_relative_paths {
    local base_dir="$1"
    local total_bytes=0
    local relative_path

    while IFS= read -r relative_path; do
        if [[ -z "$relative_path" ]]; then
            continue
        fi

        total_bytes=$((total_bytes + $(transfer_size_of "$base_dir/$relative_path")))
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
function sum_transfer_size_of_fetched_files_in {
    local directory="$1"

    find "$directory" -type f ! -name '*.br' ! -name '*.gz' ! -name '.DS_Store' -print0 | sum_transfer_size_of_stream
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
        # Newline-delimited: these labels contain spaces, so a space-delimited list would split mid-label.
        OVER_CAP_LABELS="${OVER_CAP_LABELS}${label%:}"$'\n'
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
ARTIFACT_UNAVAILABLE_REASON=""
CODE_UNAVAILABLE_REASON=""

function note_artifact_gap {
    local reason="$1"

    if [[ -z "$ARTIFACT_UNAVAILABLE_REASON" ]]; then
        ARTIFACT_UNAVAILABLE_REASON="$reason"
    fi
}

function note_code_gap {
    local reason="$1"

    if [[ -z "$CODE_UNAVAILABLE_REASON" ]]; then
        CODE_UNAVAILABLE_REASON="$reason"
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
        note_code_gap "$(to_repo_relative_path "$PKG_DIR") holds no $label"
        return 0
    fi

    if [[ "$match_count" -gt 1 ]]; then
        note_code_gap "$(to_repo_relative_path "$PKG_DIR") holds $match_count files matching $name_glob, so the $label a visitor fetches is ambiguous; a full ./scripts/build/build-site.sh clears the site root and rebuilds it"
        return 0
    fi

    MEASURED_COMPONENT_BYTES="$(transfer_size_of "$(find "$PKG_DIR" -type f -name "$name_glob")")"
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

if [[ -f "$DOCUMENT_PATH" ]]; then
    HTML_BYTES="$(transfer_size_of "$DOCUMENT_PATH")"
else
    note_code_gap "the site tree has no prerendered document at $(to_repo_relative_path "$DOCUMENT_PATH"); run ./scripts/build/build-site.sh, which writes it after the build"
fi

EMBEDDED_BYTES=0
EMBEDDED_UNAVAILABLE_REASON=""

EMBEDDED_MANIFEST_PATH="$EMBEDDED_DIR/manifest.json"

if [[ ! -d "$EMBEDDED_DIR" ]]; then
    EMBEDDED_UNAVAILABLE_REASON="$(to_repo_relative_path "$EMBEDDED_DIR") is absent; run scripts/build/sync-embedded-bundle.sh ./web/static/$EMBEDDED_SUBDIR before the build"
elif [[ ! -f "$EMBEDDED_MANIFEST_PATH" ]]; then
    EMBEDDED_UNAVAILABLE_REASON="$(to_repo_relative_path "$EMBEDDED_DIR") holds no manifest.json, so the bundle a visitor would open is incomplete"
else
    EMBEDDED_BYTES="$(sum_transfer_size_of_fetched_files_in "$EMBEDDED_DIR")"
fi

if [[ -n "$EMBEDDED_UNAVAILABLE_REASON" ]]; then
    note_artifact_gap "$EMBEDDED_UNAVAILABLE_REASON"
fi

FIRST_PAINT_BYTES="$EMBEDDED_BYTES"
CLIENT_CODE_BYTES=$((WASM_BYTES + JS_SHIM_BYTES + CSS_BYTES + HTML_BYTES))

LIVE_UNAVAILABLE_REASON=""
LIVE_GEOMETRY_BYTES=0
LIVE_SHARD_BYTES=0
LIVE_POINTER_BYTES=0
LIVE_MEASURED_LOCALLY=false

# Sets the LIVE_* globals above. The tree can hold several published versions at once; only the one that
# latest/manifest.json names is ever fetched, so the manifest picks the version rather than the filesystem.
function measure_live_bundle {
    if [[ ! -f "$DISCOVERY_PATH" ]]; then
        LIVE_UNAVAILABLE_REASON="the built site tree has no discovery document at $(to_repo_relative_path "$DISCOVERY_PATH")"
        return 0
    fi

    local repository_base_url
    repository_base_url="$(jq -r '.repository_base_url' "$DISCOVERY_PATH")"
    local repository_dir

    if [[ "$repository_base_url" == /* ]]; then
        repository_dir="$SITE_DIR${repository_base_url%/}"
    else
        # A deploy build names the CDN, which this tree does not serve. The shards are the same bytes
        # wherever they are served from, so the local publish tree stands in for them; only its origin
        # differs. Without this a deploy build could not report second paint at all.
        repository_dir="$SITE_DIR/$LOCAL_REPOSITORY_SUBDIR"
        LIVE_MEASURED_LOCALLY=true

        if [[ ! -d "$repository_dir" ]]; then
            LIVE_UNAVAILABLE_REASON="the discovery document points the live bundle at $repository_base_url, and no local publish tree at $(to_repo_relative_path "$repository_dir") stands in for it; run: cargo run -p ingestion -- publish local --root ./web/static/$LOCAL_REPOSITORY_SUBDIR --public-base-url /$LOCAL_REPOSITORY_SUBDIR"
            return 0
        fi
    fi
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

    LIVE_GEOMETRY_BYTES="$(printf '%s\n' "$geometry_relative_paths" | sum_transfer_size_of_relative_paths "$version_dir")"
    LIVE_SHARD_BYTES="$(printf '%s\n' "$shard_relative_paths" | sum_transfer_size_of_relative_paths "$version_dir")"
    LIVE_POINTER_BYTES=$(($(transfer_size_of "$DISCOVERY_PATH") + $(transfer_size_of "$latest_manifest_path")))
}
measure_live_bundle

echo "Artifact bytes, which are what the targets bound:"
echo ""

if [[ -n "$ARTIFACT_UNAVAILABLE_REASON" ]]; then
    print_unmeasured_total_line "First paint:"
else
    print_total_line "First paint:" "$FIRST_PAINT_BYTES" "$FIRST_PAINT_CAP_BYTES"
fi

if [[ -n "$EMBEDDED_UNAVAILABLE_REASON" ]]; then
    print_component_line "embedded bundle" "$UNMEASURED_VALUE"
else
    print_component_line "embedded bundle" "$(format_size "$EMBEDDED_BYTES")"
fi

echo ""

if [[ -n "$ARTIFACT_UNAVAILABLE_REASON" || -n "$LIVE_UNAVAILABLE_REASON" ]]; then
    print_unmeasured_total_line "Second paint:"
else
    print_total_line "Second paint:" "$((FIRST_PAINT_BYTES + LIVE_GEOMETRY_BYTES + LIVE_SHARD_BYTES))" "$SECOND_PAINT_CAP_BYTES"
fi

# Repeats the embedded bundle, which the client has already fetched by this point, so the components sum
# to the printed total instead of leaving the reader to carry it over from first paint.
if [[ -n "$EMBEDDED_UNAVAILABLE_REASON" ]]; then
    print_component_line "embedded bundle" "$UNMEASURED_VALUE"
else
    print_component_line "embedded bundle" "$(format_size "$EMBEDDED_BYTES")"
fi

if [[ -n "$LIVE_UNAVAILABLE_REASON" ]]; then
    print_component_line "+ geometry" "$UNMEASURED_VALUE"
    print_component_line "+ statistic shards" "$UNMEASURED_VALUE"
else
    print_component_line "+ geometry" "$(format_size "$LIVE_GEOMETRY_BYTES")"
    print_component_line "+ statistic shards" "$(format_size "$LIVE_SHARD_BYTES")"
fi

echo ""
echo "Client code, reported but not capped:"
echo ""

if [[ -n "$CODE_UNAVAILABLE_REASON" ]]; then
    print_unmeasured_total_line "Total:"
else
    printf '%-14s%s\n' "Total:" "$(format_size "$CLIENT_CODE_BYTES")"
fi

print_component_line "wasm" "$WASM_DISPLAY"
print_component_line "js shim" "$JS_SHIM_DISPLAY"
print_component_line "css" "$CSS_DISPLAY"

if [[ -f "$DOCUMENT_PATH" ]]; then
    print_component_line "document" "$(format_size "$HTML_BYTES")"
else
    print_component_line "document" "$UNMEASURED_VALUE"
fi

echo ""

if [[ -n "$OVER_CAP_LABELS" ]]; then
    while IFS= read -r over_cap_label; do
        if [[ -z "$over_cap_label" ]]; then
            continue
        fi

        echo "WARNING: $over_cap_label is over its cap."
    done <<< "$OVER_CAP_LABELS"

    echo "The caps are targets to be read by a person, so this is a warning and never a build failure."
    echo ""
fi

echo "Notes:"

if [[ -n "$ARTIFACT_UNAVAILABLE_REASON" ]]; then
    echo "  - The artifact totals are unmeasured: $ARTIFACT_UNAVAILABLE_REASON."
fi

if [[ -n "$CODE_UNAVAILABLE_REASON" ]]; then
    echo "  - The client-code total is unmeasured: $CODE_UNAVAILABLE_REASON."
fi

if [[ -n "$LIVE_UNAVAILABLE_REASON" ]]; then
    echo "  - Second paint is unmeasured: $LIVE_UNAVAILABLE_REASON."
else
    if [[ "$LIVE_MEASURED_LOCALLY" == true ]]; then
        echo "  - The live shards were measured in the local publish tree, since the discovery document names"
        echo "    a remote repository this tree does not serve. Same bytes, different origin."
    fi

    echo "  - Second paint counts the geometry shard and the statistic shards, not the discovery document"
    echo "    and latest/manifest.json the client fetches to find them ($(format_size "$LIVE_POINTER_BYTES") combined)."
fi

echo "  - Sizes are what a client transfers: brotli -q 11 for the types Cloudflare compresses (wasm, js,"
echo "    css, html, json), and the full file size for the rest. The geometry and statistic shards are"
echo "    served with content types absent from Cloudflare's auto-compress list, so they transfer whole."
echo "  - Decimal megabytes: 1 MB is 1000 KB."
