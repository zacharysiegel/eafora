#!/bin/zsh
# Compiles the wgpu render pipelines against the local GPU adapter, which forces wgpu's shader
# frontend to parse the WGSL in shaders/ and translate it to the platform's shading language (Metal
# on macOS). WGSL is only validated at pipeline-creation time, not by `cargo build`, so this is the
# check that catches shader errors. Requires a working GPU adapter; the test is `#[ignore]` so it
# does not run in the ordinary `cargo test` sweep.
#
# Extra arguments are forwarded to libtest, e.g.:
#   ./scripts/test/test-shaders.sh --nocapture

set -euo pipefail

repo_dir=$(git rev-parse --show-toplevel)
cd "${repo_dir}"
exec cargo test -p shared --features render -- --ignored render_pipelines_compile "$@"
