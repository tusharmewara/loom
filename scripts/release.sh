#!/usr/bin/env bash
# Loom release automation: version bump -> changelog -> tag -> (opt-in) push + CI dispatch.
#
# Usage: scripts/release.sh <version> [--dry-run] [--allow-dirty] [--push]
#
# Default behaviour is offline and side-effect-free: it edits files, commits and
# tags locally, and prints the exact CI command to run. Nothing is pushed and no
# workflow is dispatched unless --push is given.
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

CARGO_TOML="Cargo.toml"
CARGO_LOCK="Cargo.lock"
CHANGELOG="CHANGELOG.md"
NOTES_FILE="RELEASE_NOTES.md"

DRY_RUN=0
ALLOW_DIRTY=0
DO_PUSH=0
VERSION=""

usage() {
  cat <<'EOF'
Usage: scripts/release.sh <version> [options]

Release Loom: bump the workspace version, add a changelog section, commit,
tag, and (opt-in) push + dispatch the Homebrew bottles workflow.

Arguments:
  <version>            New version, "X.Y.Z" or "vX.Y.Z" (optional pre-release
                       suffix, e.g. 0.10.0-rc.1). Must be strictly greater than
                       the current version in Cargo.toml.

Options:
  --dry-run            Print every step and change nothing.
  --allow-dirty        Allow an uncommitted working tree (loud warning).
  --push               Push the commit + tag and dispatch the bottles
                       workflow. Off by default (no network side effects).
  -h, --help           Show this help.

Examples:
  scripts/release.sh 0.9.7 --dry-run
  scripts/release.sh 0.9.7
  scripts/release.sh v0.10.0 --push
EOF
}

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
info() { printf '%s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }

# ---------------------------------------------------------------- arguments
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    --dry-run) DRY_RUN=1 ;;
    --allow-dirty) ALLOW_DIRTY=1 ;;
    --push) DO_PUSH=1 ;;
    -*) die "unknown option: $1 (try --help)" ;;
    *)
      [ -z "$VERSION" ] || die "unexpected argument: $1"
      VERSION="$1" ;;
  esac
  shift
done

[ -n "$VERSION" ] || { usage; exit 1; }

# ---------------------------------------------------------------- version validation
VERSION="${VERSION#v}"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'; then
  die "invalid version '$VERSION': expected X.Y.Z (optional -prerelease suffix)"
fi
# semver: numeric identifiers must not have leading zeros
if printf '%s' "$VERSION" | grep -Eq '(^|\.)0[0-9]'; then
  die "invalid version '$VERSION': numeric identifiers must not have leading zeros"
fi

read_workspace_version() {
  awk '
    /^\[workspace\.package\]/ { inpkg=1; next }
    /^\[/ { inpkg=0 }
    inpkg && /^version[[:space:]]*=/ {
      gsub(/[ "]/, "", $0); sub(/^version=/, "", $0); print; exit
    }' "$1"
}

CURRENT_VERSION="$(read_workspace_version "$CARGO_TOML")"
[ -n "$CURRENT_VERSION" ] || die "could not read [workspace.package] version from $CARGO_TOML"

# semver comparison: prints -1 (a<b), 0 (a==b), 1 (a>b)
semver_cmp() {
  awk -v A="$1" -v B="$2" '
    function cmp(a, b,   ac, ap, bc, bp, ai, bi, na, nb, n, i, x, y, xn, yn) {
      sub(/\+.*/, "", a); sub(/\+.*/, "", b)
      ai = index(a, "-"); bi = index(b, "-")
      if (ai) { ac = substr(a, 1, ai-1); ap = substr(a, ai+1) } else { ac = a; ap = "" }
      if (bi) { bc = substr(b, 1, bi-1); bp = substr(b, bi+1) } else { bc = b; bp = "" }
      na = split(ac, an, "."); nb = split(bc, bn, ".")
      n = (na > nb) ? na : nb
      for (i = 1; i <= n; i++) {
        x = (i <= na) ? an[i] + 0 : 0
        y = (i <= nb) ? bn[i] + 0 : 0
        if (x > y) return 1
        if (x < y) return -1
      }
      if (ap == "" && bp == "") return 0
      if (ap == "") return 1          # release > pre-release
      if (bp == "") return -1
      na = split(ap, an, "."); nb = split(bp, bn, ".")
      n = (na > nb) ? na : nb
      for (i = 1; i <= n; i++) {
        x = (i <= na) ? an[i] : ""
        y = (i <= nb) ? bn[i] : ""
        if (x == y) continue
        if (x == "") return -1
        if (y == "") return 1
        xn = (x ~ /^[0-9]+$/); yn = (y ~ /^[0-9]+$/)
        if (xn && yn) return (x + 0 > y + 0) ? 1 : -1
        if (xn) return -1             # numeric < alphanumeric
        if (yn) return 1
        return (x > y) ? 1 : -1
      }
      return 0
    }
    BEGIN { print cmp(A, B) }'
}

CMP="$(semver_cmp "$VERSION" "$CURRENT_VERSION")"
[ "$CMP" = 1 ] || die "version $VERSION is not greater than the current version $CURRENT_VERSION"

if git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null 2>&1; then
  die "tag v$VERSION already exists"
fi

# A dirty tree means: uncommitted changes to tracked files, staged changes, or
# untracked files (a forgotten `git add` would silently skip a file from the
# release). RELEASE_NOTES.md is release input, not repo state: it is expected
# to be edited before a release, so it does not count as "dirty".
if [ -n "$(git status --porcelain -- . ":(exclude)$NOTES_FILE")" ]; then
  if [ "$ALLOW_DIRTY" -eq 1 ]; then
    printf '\n*** WARNING: the working tree has uncommitted changes and --allow-dirty was passed. ***\n' >&2
    git status --short >&2
  else
    die "working tree is dirty (uncommitted changes); commit or stash them, or pass --allow-dirty"
  fi
fi

REPO_SLUG="$(git remote get-url origin 2>/dev/null | sed -E 's#^https?://[^/]+/##; s#\.git$##' || true)"
case "$REPO_SLUG" in
  */*) ;;
  *) REPO_SLUG="tusharmewara/loom" ;;
esac
DATE="$(date +%Y-%m-%d)"
TAG="v$VERSION"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# ---------------------------------------------------------------- changelog body
# Extract the "## Notes" section of RELEASE_NOTES.md, strip HTML comments
# (including multi-line ones) and blank lines.
extract_notes() {
  awk '
    /^##[[:space:]]+Notes[[:space:]]*$/ { f=1; next }
    f && /^##[[:space:]]/ { f=0 }
    f { print }
  ' "$NOTES_FILE" | awk '
    { line = $0 }
    incomment {
      if (line ~ /-->/) { sub(/.*-->/, "", line); incomment = 0 } else { next }
    }
    {
      while (match(line, /<!--/)) {
        pre = substr(line, 1, RSTART - 1)
        rest = substr(line, RSTART + 4)
        if (rest ~ /-->/) {
          sub(/^.*-->/, "", rest)
          line = pre rest
        } else {
          line = pre
          incomment = 1
          break
        }
      }
      if (incomment) next
      print line
    }
  ' | awk 'NR==1 && /^[ \t]*$/ {next} {a[++n]=$0} END {last=n; while (last>0 && a[last] ~ /^[ \t]*$/) last--; for (i=1;i<=last;i++) print a[i]}'
}

build_notes_body() {
  local out="$1"
  if [ -f "$NOTES_FILE" ] && [ -n "$(extract_notes | head -n 1)" ]; then
    {
      printf '<!-- Release notes copied from %s -->\n\n' "$NOTES_FILE"
      extract_notes
    } > "$out"
    return
  fi
  cat > "$out" <<EOF
### Added

- TODO: describe this release. Either fill in the "## Notes" section of
  \`$NOTES_FILE\` (its content is copied here verbatim) or edit this section
  before tagging.
EOF
}

NOTES_BODY="$WORK/body.md"
build_notes_body "$NOTES_BODY"
if grep -q 'TODO: describe this release' "$NOTES_BODY"; then
  printf 'NOTE: no release notes found — the changelog section carries a placeholder.\n' >&2
fi

SECTION_FILE="$WORK/section.md"
{
  printf '## [%s] - %s\n\n' "$VERSION" "$DATE"
  cat "$NOTES_BODY"
  printf '\n'
} > "$SECTION_FILE"

DISPATCH_CMD="gh workflow run homebrew-bottles.yml -f version=$VERSION -R $REPO_SLUG"

# ---------------------------------------------------------------- dry run
if [ "$DRY_RUN" -eq 1 ]; then
  step "Release plan (dry run — nothing is changed)"
  info "version      : $VERSION  (current: $CURRENT_VERSION)"
  info "tag          : $TAG"
  info "date         : $DATE"
  info "repo         : $REPO_SLUG"
  info "notes source : $( [ -f "$NOTES_FILE" ] && echo "$NOTES_FILE" || echo "(none — placeholder body)")"

  step "Step 1/6: bump $CARGO_TOML [workspace.package] version"
  info "  - version = \"$CURRENT_VERSION\"  ->  version = \"$VERSION\""

  step "Step 2/6: prepend changelog section to $CHANGELOG"
  info "  section:"
  sed 's/^/    /' "$SECTION_FILE"

  step "Step 3/6: refresh $CARGO_LOCK"
  info "  run: cargo metadata --offline --format-version 1   (updates the lock file)"
  info "  verify: every loom-* package in $CARGO_LOCK carries version $VERSION"

  step "Step 4/6: commit + tag"
  info "  git commit -m \"chore(release): $VERSION\""
  info "  git tag -a $TAG -m \"Loom $VERSION\""

  step "Step 5/6: push + dispatch"
  if [ "$DO_PUSH" -eq 1 ]; then
    info "  git push origin HEAD"
    info "  git push origin $TAG"
    info "  $DISPATCH_CMD"
  else
    info "  (skipped — pass --push to push and dispatch)"
  fi

  step "Next command"
  info "  $DISPATCH_CMD"
  exit 0
fi

# ---------------------------------------------------------------- 1. version bump
step "Step 1/6: bump $CARGO_TOML [workspace.package] version to $VERSION"
awk -v v="$VERSION" '
  /^\[workspace\.package\]/ { inpkg=1; print; next }
  /^\[/ { inpkg=0 }
  inpkg && /^version[[:space:]]*=/ { print "version = \"" v "\""; next }
  { print }
' "$CARGO_TOML" > "$WORK/Cargo.toml.new"
mv "$WORK/Cargo.toml.new" "$CARGO_TOML"

NEW_VERSION="$(read_workspace_version "$CARGO_TOML")"
[ "$NEW_VERSION" = "$VERSION" ] || die "failed to bump $CARGO_TOML (got '$NEW_VERSION')"
info "  $CARGO_TOML: $CURRENT_VERSION -> $NEW_VERSION"

# ---------------------------------------------------------------- 2. changelog
step "Step 2/6: prepend '## [$VERSION] - $DATE' to $CHANGELOG"
awk -v sectionfile="$SECTION_FILE" '
  BEGIN { while ((getline line < sectionfile) > 0) block = block line "\n" }
  !done && /^## \[/ { printf "%s", block; done=1 }
  { print }
  END { if (!done) printf "%s", block }
' "$CHANGELOG" > "$WORK/CHANGELOG.md.new"
mv "$WORK/CHANGELOG.md.new" "$CHANGELOG"
sed -n '1,14p' "$CHANGELOG" | sed 's/^/  /'

# ---------------------------------------------------------------- 3. cargo lock
step "Step 3/6: refresh $CARGO_LOCK"
if ! cargo metadata --offline --format-version 1 >/dev/null 2>"$WORK/cargo.err"; then
  info "  offline metadata failed, retrying with network access:"
  cargo metadata --format-version 1 >/dev/null 2>"$WORK/cargo.err" \
    || { cat "$WORK/cargo.err" >&2; die "cargo could not update $CARGO_LOCK"; }
fi
BAD="$(awk -v v="$VERSION" '
  /^\[\[package\]\]/ { name=""; ver=""; next }
  /^name = / { gsub(/[ "]/, "", $0); sub(/^name=/, "", $0); name=$0 }
  /^version = / { gsub(/[ "]/, "", $0); sub(/^version=/, "", $0); ver=$0; if (name ~ /^loom/ && ver != v) print name " = " ver }
' "$CARGO_LOCK")"
[ -z "$BAD" ] || die "$CARGO_LOCK is out of sync with $VERSION:$BAD"
info "  $CARGO_LOCK: every loom-* package at $VERSION"

# ---------------------------------------------------------------- 4. commit + tag
step "Step 4/6: commit and tag"
git add "$CARGO_TOML" "$CARGO_LOCK" "$CHANGELOG"
git commit -q -m "chore(release): $VERSION" || die "commit failed"
git tag -a "$TAG" -m "Loom $VERSION" || die "tag failed"
info "  commit: $(git rev-parse --short HEAD)"
info "  tag   : $TAG"

# ---------------------------------------------------------------- 5. push + dispatch
if [ "$DO_PUSH" -eq 1 ]; then
  step "Step 5/6: push and dispatch"
  git push origin HEAD
  git push origin "$TAG"
  if command -v gh >/dev/null 2>&1; then
    eval "$DISPATCH_CMD"
    info "  dispatched: $DISPATCH_CMD"
  else
    info "  gh not found — run manually: $DISPATCH_CMD"
  fi
else
  step "Step 5/6: push and dispatch (skipped — pass --push to enable)"
fi

step "Next command"
info "  $DISPATCH_CMD"
info ""
info "Release $VERSION prepared locally (tag $TAG). Push and dispatch when ready."
