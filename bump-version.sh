#!/bin/bash
set -euo pipefail

if [ $# -ne 1 ]; then
    echo "Usage: ./bump-version.sh <version>"
    echo "Example: ./bump-version.sh 0.2.0"
    exit 1
fi

VERSION="$1"

# Validate version format (semver without v prefix)
if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "Error: version must be in semver format (e.g. 0.2.0)"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Update Cargo.toml files
for crate in tuck-cli tuck-core tuck-ffi; do
    sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" "$SCRIPT_DIR/crates/$crate/Cargo.toml"
    echo "Updated crates/$crate/Cargo.toml"
done

# Update TuckApp/project.yml
sed -i '' "s/MARKETING_VERSION: \".*\"/MARKETING_VERSION: \"$VERSION\"/" "$SCRIPT_DIR/TuckApp/project.yml"
echo "Updated TuckApp/project.yml"

# Update Cargo.lock
cd "$SCRIPT_DIR"
cargo check --quiet 2>/dev/null
echo "Updated Cargo.lock"

echo ""
echo "Version bumped to $VERSION"
echo "Next steps:"
echo "  git add -A && git commit -m \"Bump version to $VERSION\""
echo "  git tag v$VERSION"
echo "  git push origin main v$VERSION"
