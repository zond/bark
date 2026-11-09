#!/usr/bin/env bash
# Whether the node firmware (`fw`) or the app (`app`) changed between two
# commits, by the files that build it: exit 0 if it did, 1 if not, 2 on an
# error. The release workflow (.github/workflows/release.yml) builds a side
# only when it changed and otherwise carries the previous release's files,
# so that an app fix does not mint an image every node gathers and swaps
# in for nothing, nor a firmware fix make every phone download the app
# (roadmap D164).
#
#   scripts/release-changed.sh <previous release's commit> <commit> fw|app
#
# Markdown builds nothing, so it is left out. The release workflow counts
# for both, since it holds how each is built and packed.
set -u

if [ $# -ne 3 ]; then
  echo "usage: $0 <from> <to> fw|app" >&2
  exit 2
fi

case "$3" in
  # The firmware, the loader, the crates they link and the scripts that
  # pack and sign them (bark-fw/scripts). bark-app-layer is linked into
  # the firmware.
  fw)
    paths=(bark-fw bark-loader bark-proto bark-image bark-driver bark-link
      bark-app-layer bark-screen Cargo.toml Cargo.lock) ;;
  # The app, and of bark-app-layer what the app's tests check it against:
  # the app layer is one format on both ends, the firmware's in Rust and
  # the app's in Dart, and a changed vector or word list is a change the
  # app must carry too.
  app)
    paths=(bark-app bark-app-layer/vectors.json bark-app-layer/src/words.txt) ;;
  *)
    echo "$0: $3 is neither fw nor app" >&2
    exit 2 ;;
esac

git diff --quiet "$1" "$2" -- "${paths[@]}" .github/workflows/release.yml \
  ':(exclude,glob)**/*.md'
case $? in
  0) exit 1 ;;
  1) exit 0 ;;
  *) exit 2 ;;
esac
