//! Write the firmware's `memory.x` from the one layout table
//! (`bark-image/src/layout.rs`) and put it on the linker search path.

use std::{env, fs, path::PathBuf};

use bark_image::layout::{write_memory_x, Image};

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let mut script = String::new();
    write_memory_x(&mut script, Image::Firmware).expect("writing to a String");
    fs::write(out.join("memory.x"), script).expect("write memory.x");
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=../bark-image/src/layout.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
