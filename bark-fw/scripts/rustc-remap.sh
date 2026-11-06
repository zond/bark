#!/bin/sh
# Cargo runs this in place of rustc for the firmware and the loader
# (`build.rustc-wrapper` in their `.cargo/config.toml`), with rustc and its
# arguments after it. It runs them with the checkout's root and Cargo's
# home remapped to fixed names, so that the source paths compiled into the
# image, a panic's location among them, are the same wherever the
# repository and Cargo's caches lie: the image's size and bytes do not
# depend on the checkout, where a longer path once cost flash that the
# image did not have to spare (bark-fw/README.md, Build and package).
#
# Cargo cannot remap these itself on the stable toolchain (its
# `trim-paths` profile option is unstable in 1.98), and a rustflag in the
# config cannot name the root, which is known only where the build runs.
#
# The standard library's source, which generic code from it compiled
# here names, is mapped to where the toolchain's own compiled library
# names it, `/rustc/<commit>`, so the toolchain's install path drops out
# as well.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
home=${CARGO_HOME:-$HOME/.cargo}
std="$("$1" --print sysroot)/lib/rustlib/src/rust"
commit=$("$1" -vV | sed -n 's/^commit-hash: //p')
exec "$@" --remap-path-prefix="$root=/bark" --remap-path-prefix="$home=/cargo" \
    --remap-path-prefix="$std=/rustc/$commit"
