#!/bin/sh
# Cargo runs this in place of rustc for the firmware and the loader
# (`build.rustc-wrapper` in their `.cargo/config.toml`), with rustc and its
# arguments after it. It runs them with the source paths compiled into
# the image, a panic's location among them, cut to what names the file:
# the crate's directory and the path in it (`bark-proto/src/node.rs`,
# `aes-0.8.4/src/soft/fixslice32.rs`, `core/src/option.rs`). So the image's
# size and bytes do not depend on where the repository, Cargo's caches or
# the toolchain lie, and the image carries no prefix that names none of
# the files: a longer checkout path once cost flash the image did not
# have to spare, and the registry's prefix alone cost 3 KB
# (bark-fw/README.md, Build and package).
#
# Cargo cannot remap these itself on the stable toolchain (its
# `trim-paths` profile option is unstable in 1.98), and a rustflag in the
# config cannot name the root, which is known only where the build runs.
#
# rustc applies the last matching mapping, so the narrower ones come after
# Cargo's home, which still covers whatever else lies in it. The standard
# library's source, which generic code from it compiled here names, and
# the toolchain's compiled library, which names it `/rustc/<commit>`, map
# alike.
set -eu
root=$(cd "$(dirname "$0")/../.." && pwd)
home=${CARGO_HOME:-$HOME/.cargo}
std="$("$1" --print sysroot)/lib/rustlib/src/rust"
commit=$("$1" -vV | sed -n 's/^commit-hash: //p')
set -- "$@" --remap-path-prefix="$home=/cargo"
for index in "$home"/registry/src/*/; do
    [ -d "$index" ] && set -- "$@" --remap-path-prefix="${index%/}="
done
exec "$@" --remap-path-prefix="$root=" \
    --remap-path-prefix="$std/library=" --remap-path-prefix="/rustc/$commit/library="
