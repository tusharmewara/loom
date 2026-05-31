#!/bin/bash
# =============================================================================
# Loom Release Build Script — builds cross-platform bottles + publishes to
#                      tusharmewara/homebrew-loom
# =============================================================================
# Usage:
#   ./release-bottle.sh [version] [git-ref]
#
# Examples:
#   ./release-bottle.sh                    # Use current version from Cargo.toml
#   ./release-bottle.sh 0.3.0             # Build version 0.3.0
#   ./release-bottle.sh 0.3.0 v0.3.0      # Build and tag v0.3.0
#   ./release-bottle.sh --linux-only       # Linux x86_64 bottle only (via cargo-zigbuild)
#   SKIP_UPLOAD=1 ./release-bottle.sh      # Build bottles locally, no upload
# =============================================================================

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
HOMEBREW_REPO="tusharmewara/homebrew-loom"
LOOM_REPO="tusharmewara/loom"
WORK_DIR="$(mktemp -d)"
CLEANUP=()

# ── Colors ─────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()   { echo -e "${GREEN}[LOOM]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1" >&2; }

cleanup() {
    log "Cleaning up..."
    rm -rf "$WORK_DIR"
}
trap cleanup EXIT

# ── Platform detection ──────────────────────────────────────────────────────
detect_macos_codename() {
    local major
    major=$(sw_vers -productVersion 2>/dev/null | cut -d. -f1 || echo "0")
    case "$major" in
        13) echo "ventura"  ;;
        14) echo "sonoma"   ;;
        15) echo "sequoia"  ;;
        26) echo "tahoe"    ;;  # macOS 26+
        *)  echo "unknown"  ;;
    esac
}

is_macos() { [[ "$(uname)" == "Darwin" ]]; }
is_arm64() { [[ "$(uname -m)" == "arm64" ]]; }

# ── Rust toolchain helpers ───────────────────────────────────────────────────
# Ensure we use the rustup-managed toolchain (not Homebrew's), which has
# cross-compilation target std libraries installed via rustup.
ensure_rustup_toolchain() {
    if command -v rustup &>/dev/null; then
        local rustup_cargo
        rustup_cargo="$(rustup which cargo 2>/dev/null || true)"
        if [[ -n "$rustup_cargo" ]]; then
            local rustup_bin
            rustup_bin="$(dirname "$rustup_cargo")"
            export PATH="$rustup_bin:$PATH"
            log "Using rustup-managed toolchain: $(rustc --version)"
        fi
    fi
}

ensure_target() {
    local target="$1"
    rustup target add "$target" 2>/dev/null || true
}

# ── Parse Arguments ──────────────────────────────────────────────────────────
VERSION="${1:-}"
GIT_REF="${2:-}"
LINUX_ONLY=false
[[ "${1:-}" == "--linux-only" ]] && LINUX_ONLY=true && VERSION="${2:-}"
[[ "${2:-}" == "--linux-only" ]] && LINUX_ONLY=true && GIT_REF="${3:-}"

if [[ -z "$VERSION" ]]; then
    VERSION=$(grep '^version = ' "$REPO_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
    log "Detected version: $VERSION"
fi

if [[ -z "$GIT_REF" ]]; then
    GIT_REF="v$VERSION"
fi

TAG="v$VERSION"
ASSET_PREFIX="loom-$VERSION"

# =============================================================================
# Build Functions
# =============================================================================

build_macos_x86_64() {
    log "Building x86_64-apple-darwin binaries..."
    ensure_target "x86_64-apple-darwin"
    CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="$(xcrun -f clang)" \
    cargo build --release --bins -p loom -p loomd \
        --target x86_64-apple-darwin 2>&1 | tail -3
}

build_macos_arm64() {
    log "Building aarch64-apple-darwin binaries..."
    ensure_target "aarch64-apple-darwin"
    cargo build --release --bins -p loom -p loomd \
        --target aarch64-apple-darwin 2>&1 | tail -3
}

build_linux_x86_64_via_zigbuild() {
    log "Building x86_64-unknown-linux-gnu binaries via cargo-zigbuild..."

    # Ensure cargo-zigbuild is installed
    if ! command -v cargo-zigbuild &>/dev/null && ! cargo install --list 2>/dev/null | grep -q cargo-zigbuild; then
        log "Installing cargo-zigbuild (requires Zig)..."
        if ! command -v zig &>/dev/null; then
            error "Zig not found. Install it: brew install zig"
            return 1
        fi
        cargo install cargo-zigbuild 2>&1 | tail -3
    fi

    cargo zigbuild --release --bins -p loom -p loomd \
        --target x86_64-unknown-linux-gnu 2>&1 | tail -5
}

# =============================================================================
# Bottle Creation
# =============================================================================
create_bottle() {
    local arch="$1"        # e.g. "arm64" or "x86_64"
    local target="$2"      # e.g. "aarch64-apple-darwin"
    local os_label="$3"    # e.g. "sonoma" or "linux"
    local binary_dir="$4"
    local tag="${os_label}"
    [[ "$arch" == "arm64" ]] && tag="arm64_${os_label}"
    [[ "$os_label" == "linux" ]] && tag="x86_64_linux"
    local output_file="$WORK_DIR/bottles/loom-$VERSION.${tag}.bottle.tar.gz"

    log "Creating $os_label/$arch bottle..."

    # Homebrew bottles must have a <formula_name>/<version>/ prefix inside the
    # tarball (e.g. "loom/0.3.0/bin/loom"). This matches what `brew bottle`
    # creates in bottle.rb line 533:
    #   safe_system tar, "--create", ..., "#{formula.name}/#{formula.pkg_version}"
    local bottle_prefix="loom/$VERSION"
    mkdir -p "$WORK_DIR/stage-$os_label-$arch/$bottle_prefix/bin"

    cp "$binary_dir/loom"  "$WORK_DIR/stage-$os_label-$arch/$bottle_prefix/bin/"
    cp "$binary_dir/loomd" "$WORK_DIR/stage-$os_label-$arch/$bottle_prefix/bin/"
    chmod 755 "$WORK_DIR/stage-$os_label-$arch/$bottle_prefix/bin/loom" \
              "$WORK_DIR/stage-$os_label-$arch/$bottle_prefix/bin/loomd"

    mkdir -p "$WORK_DIR/bottles"
    tar -czf "$output_file" -C "$WORK_DIR/stage-$os_label-$arch" "loom"
    rm -rf "$WORK_DIR/stage-$os_label-$arch"

    local sha
    sha=$(shasum -a 256 "$output_file" | cut -d' ' -f1)
    log "  → ${tag} SHA: $sha"
    echo "${tag}=${sha}"
}

# =============================================================================
# Main Build
# =============================================================================
log "Loom Release Build v$VERSION (ref: $GIT_REF)"
cd "$REPO_DIR"

# Use rustup-managed toolchain for cross-compilation target std libraries
ensure_rustup_toolchain

declare -a BOTTLE_SHAS=()

if $LINUX_ONLY; then
    # ── Linux only (via cargo-zigbuild) ────────────────────────────────────────
    build_linux_x86_64_via_zigbuild || {
        error "Linux cross-compilation via zigbuild failed."
        exit 1
    }

    local binary_dir="$REPO_DIR/target/x86_64-unknown-linux-gnu/release"
    if [[ -f "$binary_dir/loom" ]]; then
        BOTTLE_SHAS+=("$(create_bottle "x86_64" "x86_64-unknown-linux-gnu" "linux" "$binary_dir")")
    else
        error "Linux binaries not found at $binary_dir"
        exit 1
    fi

elif is_macos; then
    # ── macOS (native ARM64 + cross-compiled x86_64) ────────────────────────
    local_codename=$(detect_macos_codename)
    log "Native host: macOS $(sw_vers -productVersion) (codename: $local_codename)"

    build_macos_arm64
    build_macos_x86_64

    # ARM64 bottle (native)
    BOTTLE_SHAS+=("$(create_bottle "arm64" "aarch64-apple-darwin" "$local_codename" \
        "$REPO_DIR/target/aarch64-apple-darwin/release")")

    # x86_64 bottle (cross-compiled)
    BOTTLE_SHAS+=("$(create_bottle "x86_64" "x86_64-apple-darwin" "$local_codename" \
        "$REPO_DIR/target/x86_64-apple-darwin/release")")

    # ── Linux (via cargo-zigbuild if available) ────────────────────────────────
    if command -v zig &>/dev/null; then
        log "Zig found — building Linux bottle via cargo-zigbuild..."
        if build_linux_x86_64_via_zigbuild; then
            local binary_dir="$REPO_DIR/target/x86_64-unknown-linux-gnu/release"
            if [[ -f "$binary_dir/loom" ]]; then
                BOTTLE_SHAS+=("$(create_bottle "x86_64" "x86_64-unknown-linux-gnu" "linux" "$binary_dir")")
            else
                warn "Linux build via zigbuild produced no binaries — skipping Linux bottle."
            fi
        else
            warn "Linux build via zigbuild failed — skipping Linux bottle."
        fi
    else
        warn "Zig not installed — skipping Linux bottle (brew install zig to enable)."
    fi

else
    error "Unsupported build host (expected macOS or --linux-only)."
    exit 1
fi

# =============================================================================
# Create Source Tarball
# =============================================================================
log "Creating source tarball..."
git archive --format=tar.gz --prefix="loom-$VERSION/" "$GIT_REF" \
    -o "$WORK_DIR/loom-$VERSION.tar.gz" 2>/dev/null || {
    # Fallback: use HEAD if tag doesn't exist locally
    warn "Tag $GIT_REF not found locally, using HEAD for source archive."
    git archive --format=tar.gz --prefix="loom-$VERSION/" HEAD \
        -o "$WORK_DIR/loom-$VERSION.tar.gz"
}
SOURCE_SHA=$(shasum -a 256 "$WORK_DIR/loom-$VERSION.tar.gz" | cut -d' ' -f1)
log "Source SHA: $SOURCE_SHA"

# =============================================================================
# Parse bottle SHAs
# =============================================================================
# Helper: get value by key prefix from BOTTLE_SHAS array
get_bottle_sha() {
    local prefix="$1"
    for entry in "${BOTTLE_SHAS[@]}"; do
        if [[ "$entry" == "${prefix}="* ]]; then
            echo "${entry#*=}"
            return 0
        fi
    done
    echo ""
}

# =============================================================================
# Upload (unless SKIP_UPLOAD=1)
# =============================================================================
if [[ "${SKIP_UPLOAD:-0}" != "1" ]]; then
    # ── Upload source tarball to main loom release ─────────────────────────
    log "Uploading source tarball to $LOOM_REPO release $TAG..."
    if ! gh release view "$TAG" -R "$LOOM_REPO" &>/dev/null; then
        warn "Release $TAG not found in $LOOM_REPO — creating it..."
        gh release create "$TAG" -R "$LOOM_REPO" --title "Loom $VERSION" \
            --notes "See https://github.com/tusharmewara/loom/releases/tag/$TAG" || true
    fi
    gh release upload "$TAG" "$WORK_DIR/loom-$VERSION.tar.gz" \
        -R "$LOOM_REPO" --clobber

    # ── Upload bottles to homebrew-loom ────────────────────────────────────
    log "Ensuring release in $HOMEBREW_REPO..."
    if ! gh release view "$TAG" -R "$HOMEBREW_REPO" &>/dev/null; then
        gh release create "$TAG" -R "$HOMEBREW_REPO" \
            --title "Loom $VERSION" --notes "Bottles for Loom v$VERSION" || true
    fi

    log "Uploading bottles to $HOMEBREW_REPO..."
    gh release upload "$TAG" "$WORK_DIR/bottles"/*.tar.gz \
        -R "$HOMEBREW_REPO" --clobber
else
    log "SKIP_UPLOAD=1 — skipping GitHub uploads."
    log "Bottles are at: $WORK_DIR/bottles/"
    ls -la "$WORK_DIR/bottles/"
fi

# =============================================================================
# Generate Formula
# =============================================================================
log "Generating Formula/loom.rb..."

# Build bottle stanza — keys are already in Homebrew tag format
BOTTLE_STANZA=""
for entry in "${BOTTLE_SHAS[@]}"; do
    key="${entry%%=*}"
    val="${entry#*=}"

    if [[ "$key" == "x86_64_linux" ]]; then
        BOTTLE_STANZA+="    sha256 cellar: :any_skip_relocation, x86_64_linux: \"${val}\"\n"
    elif [[ "$key" == arm64_* ]]; then
        BOTTLE_STANZA+="    sha256 cellar: :any, ${key}: \"${val}\"\n"
    else
        BOTTLE_STANZA+="    sha256 cellar: :any, ${key}: \"${val}\"\n"
    fi
done

cat > "$WORK_DIR/loom.rb" << FORMULA
class Loom < Formula
  desc "Unified agentic ecosystem for sharing skills, sessions, and MCP servers"
  homepage "https://github.com/tusharmewara/loom"
  version "${VERSION}"
  license "MIT"

  url "https://github.com/${LOOM_REPO}/archive/refs/tags/${TAG}.tar.gz"
  sha256 "${SOURCE_SHA}"

  bottle do
    root_url "https://github.com/${HOMEBREW_REPO}/releases/download/${TAG}"
$(echo -e "$BOTTLE_STANZA")  end

  depends_on "rust" => :build

  def install
    bin.install "bin/loom"
    bin.install "bin/loomd"
  end

  service do
    run [opt_bin/"loomd"]
    working_dir Dir.home
    keep_alive true
  end

  test do
    assert_match "Loom", shell_output("\#{bin}/loom --help")
  end
end
FORMULA

log "Formula generated at: $WORK_DIR/loom.rb"
echo ""
log "=== Next Steps ==="
echo "1. Review the formula:"
echo "   cat $WORK_DIR/loom.rb"
echo ""
echo "2. Copy to homebrew-loom repo:"
echo "   cp $WORK_DIR/loom.rb ~/codebase/homebrew-loom/Formula/loom.rb"
echo "   cd ~/codebase/homebrew-loom && git add Formula/loom.rb && git commit -m 'Loom v$VERSION' && git push"
echo ""
echo "3. Test locally:"
echo "   brew install --build-from-source ~/codebase/homebrew-loom/Formula/loom.rb"
echo "   loom --version"
echo ""
echo "4. Verify bottles download correctly:"
echo "   brew reinstall --force-cleanup tusharmewara/homebrew-loom/loom"
echo ""
echo "5. Push to test-bot (optional):"
echo "   brew test-bot --cleanloom tusharmewara/homebrew-loom/Formula/loom.rb"
