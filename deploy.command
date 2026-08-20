#!/usr/bin/env bash
# felyne-tauri release deployer.
#
# Flow: sync main to GitLab (main repo) + GitHub -> push the tag -> GitHub
# Actions builds all platforms and publishes a GitHub release -> this script
# waits for it, then mirrors the assets as a GitLab release.
#
# Usage: ./deploy.command v0.1.0
set -euo pipefail
cd "$(dirname "$0")"

TAG="${1:-}"
if [ -z "$TAG" ]; then
  echo "usage: $0 v0.1.0" >&2
  exit 1
fi

GH_REPO="whyskr-dev/felyne-tauri"
GL_PROJECT="whyskr-club%2Ffelyne-tauri"
GL_API="https://gitlab.com/api/v4/projects/$GL_PROJECT"
GL_TOKEN="$(git config --get remote.gitlab.url | sed -E 's#https://oauth2:([^@]+)@.*#\1#')"
[ -n "$GL_TOKEN" ] || { echo "gitlab remote token not found" >&2; exit 1; }

echo "==> Pushing main to gitlab + github"
git push gitlab main
git push github main

echo "==> Tagging $TAG"
if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "tag exists locally, skipping create"
else
  git tag -a "$TAG" -m "felyne $TAG"
fi
git push gitlab "$TAG"
git push github "$TAG"

echo "==> Waiting for GitHub release $TAG (builds all platforms)..."
while ! ASSETS="$(gh release view "$TAG" --repo "$GH_REPO" --json assets \
      --jq '[.assets[] | select(.name | endswith(".dmg") or endswith(".msi") or endswith(".exe") or endswith(".deb") or endswith(".AppImage"))]' 2>/dev/null)" \
      || [ "$(echo "$ASSETS" | jq 'length')" -eq 0 ]; do
  sleep 20
done
echo "$ASSETS" | jq -r '.[].name'

echo "==> Mirroring release to GitLab"
LINKS="$(echo "$ASSETS" | jq '[.[] | {name: .name, url: ("https://github.com/whyskr-dev/felyne-tauri/releases/download/'$TAG'/" + .name)}]')"
BODY="$(jq -n \
  --arg tag "$TAG" \
  --arg url "https://github.com/$GH_REPO/releases/tag/$TAG" \
  --argjson links "$LINKS" \
  '{name: $tag, tag_name: $tag,
    description: ("Built on GitHub Actions — see " + $url),
    assets: {links: $links}}')"

RESP="$(curl -sS -X POST -H "PRIVATE-TOKEN: $GL_TOKEN" -H "Content-Type: application/json" \
  -d "$BODY" "$GL_API/releases" 2>&1 || true)"
if echo "$RESP" | jq -e '.tag_name' >/dev/null 2>&1; then
  echo "Created GitLab release $TAG"
else
  echo "POST failed: $(echo "$RESP" | head -c 300)"
  RESP="$(curl -sS -X PUT -H "PRIVATE-TOKEN: $GL_TOKEN" -H "Content-Type: application/json" \
    -d "$BODY" "$GL_API/releases/$TAG" 2>&1)"
  if echo "$RESP" | jq -e '.tag_name' >/dev/null 2>&1; then
    echo "Updated GitLab release $TAG"
  else
    echo "PUT failed: $(echo "$RESP" | head -c 300)" >&2
    exit 1
  fi
fi

echo "==> Done. GitHub: https://github.com/$GH_REPO/releases/tag/$TAG"
echo "       GitLab: https://gitlab.com/whyskr-club/felyne-tauri/-/releases/$TAG"