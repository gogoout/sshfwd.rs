#!/usr/bin/env bash
set -euo pipefail

# Cross-compile sshfwd-agent for all supported platforms.
# Output goes to prebuilt-agents/{platform}/sshfwd-agent

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="$PROJECT_ROOT/prebuilt-agents"

# Platform matrix: directory-name rust-target
PLATFORMS=(
    "linux-x86_64    x86_64-unknown-linux-musl"
    "linux-aarch64   aarch64-unknown-linux-musl"
    "darwin-x86_64   x86_64-apple-darwin"
    "darwin-aarch64  aarch64-apple-darwin"
)

PROFILE="release-agent"

echo "Building sshfwd-agent for all platforms..."
echo "Output directory: $OUTPUT_DIR"
echo ""

failed=()

for entry in "${PLATFORMS[@]}"; do
    dir_name=$(echo "$entry" | awk '{print $1}')
    target=$(echo "$entry" | awk '{print $2}')
    dest_dir="$OUTPUT_DIR/$dir_name"

    echo "=== $dir_name ($target) ==="

    # Ensure the target is installed
    if ! rustup target list --installed | grep -q "^${target}$"; then
        echo "  Installing target: $target"
        rustup target add "$target" || {
            echo "  SKIP: failed to install target $target"
            failed+=("$dir_name")
            echo ""
            continue
        }
    fi

    # Build
    if cargo build -p sshfwd-agent --target "$target" --profile "$PROFILE" 2>&1; then
        # Copy binary to output directory
        mkdir -p "$dest_dir"
        src="$PROJECT_ROOT/target/$target/$PROFILE/sshfwd-agent"
        cp "$src" "$dest_dir/sshfwd-agent"
        size=$(du -h "$dest_dir/sshfwd-agent" | awk '{print $1}')
        echo "  OK: $dest_dir/sshfwd-agent ($size)"
    else
        echo "  FAIL: build failed for $target"
        failed+=("$dir_name")
    fi
    echo ""
done

echo "=== Summary ==="
total=${#PLATFORMS[@]}
fail_count=${#failed[@]}
pass_count=$((total - fail_count))
echo "Built: $pass_count/$total"

if [ ${#failed[@]} -gt 0 ]; then
    echo "Failed: ${failed[*]}"
    exit 1
fi

echo "All builds succeeded."
