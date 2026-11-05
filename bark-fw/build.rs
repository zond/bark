//! Write the firmware's `memory.x` from the one layout table
//! (`bark-image/src/layout.rs`) and put it on the linker search path; and
//! turn the place given in `BARK_LAT` and `BARK_LON` (degrees) into the
//! position's counts, by the core's own conversion, so that the image
//! holds two integers and no code to parse a number with a fraction.

use std::{env, fs, path::PathBuf};

use bark_image::layout::{write_memory_x, Image};
use bark_proto::Position;

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let mut script = String::new();
    write_memory_x(&mut script, Image::Firmware).expect("writing to a String");
    fs::write(out.join("memory.x"), script).expect("write memory.x");
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=../bark-image/src/layout.rs");
    println!("cargo:rerun-if-changed=build.rs");
    fs::write(out.join("place.rs"), place()).expect("write place.rs");
}

/// The built place as Rust: `Some` position or `None`. A variable set to
/// something that is not a number of degrees fails the build.
fn place() -> String {
    println!("cargo:rerun-if-env-changed=BARK_LAT");
    println!("cargo:rerun-if-env-changed=BARK_LON");
    let deg = |name: &str| {
        env::var(name).ok().map(|v| {
            v.trim()
                .parse::<f64>()
                .unwrap_or_else(|e| panic!("{name}={v:?} is not degrees: {e}"))
        })
    };
    match (deg("BARK_LAT"), deg("BARK_LON")) {
        (Some(lat), Some(lon)) => {
            let p = Position::from_degrees(lat, lon);
            format!("Some(Position {{ lat: {}, lon: {} }})", p.lat, p.lon)
        }
        (None, None) => "None".into(),
        _ => panic!("BARK_LAT and BARK_LON are set together or not at all"),
    }
}
