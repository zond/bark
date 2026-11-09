#!/usr/bin/env bash
# scripts/release-changed.sh against commits of this repository whose
# changes are known: the answer for each side, as the release workflow
# would take it. Needs the full history (`git fetch --unshallow`).
set -u
cd "$(dirname "$0")/.."

fail=0
# expect <from> <to> <side> <0 changed | 1 unchanged | 2 error> <what>
expect() {
  scripts/release-changed.sh "$1" "$2" "$3" 2>/dev/null
  local got=$?
  if [ "$got" != "$4" ]; then
    echo "FAIL $3 $5: exit $got, expected $4"
    fail=1
  else
    echo "ok   $3 $5"
  fi
}

# The enclosure, hardware/ and docs/ alone.
enclosure=(d10d7923f187db733300f497820ee37962d82d82^ 0f04b2273ad0dcf6104dfe028780da7c5c7a439f)
expect "${enclosure[@]}" fw 1 'the enclosure'
expect "${enclosure[@]}" app 1 'the enclosure'
# bark-app alone.
app=(943effd437870396126b3cccf7a5cab0253ec977^ 943effd437870396126b3cccf7a5cab0253ec977)
expect "${app[@]}" fw 1 'an app change'
expect "${app[@]}" app 0 'an app change'
# bark-fw's sources and lock alone.
fw=(8de44050c66f687fd4de5666651fb3165aafb7fe^ 8de44050c66f687fd4de5666651fb3165aafb7fe)
expect "${fw[@]}" fw 0 'a firmware change'
expect "${fw[@]}" app 1 'a firmware change'
# bark-fw/README.md alone: no build.
readme=(32584be7daa20cfbd4a7e7820fec9f18dfa9344f^ 32584be7daa20cfbd4a7e7820fec9f18dfa9344f)
expect "${readme[@]}" fw 1 'the firmware README'
# The release workflow: both UF2s gained the image's header, so v21's
# cannot be carried on.
workflow=(32ef9f6a5a6b728a11a360e512b952c8b62879b6 d0a3bcd27f4509a8c44f01b41bc8c6490dc367b0)
expect "${workflow[@]}" fw 0 'the release workflow'
expect "${workflow[@]}" app 0 'the release workflow'
# Nothing to compare is an error, not "unchanged".
expect 0000000000000000000000000000000000000000 HEAD fw 2 'an unknown commit'
expect HEAD HEAD neither 2 'an unknown side'

exit $fail
