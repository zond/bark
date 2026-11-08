//! Write the firmware's `memory.x` from the one layout table
//! (`bark-image/src/layout.rs`) and put it on the linker search path; and
//! turn the place given in `BARK_LAT` and `BARK_LON` (degrees) into the
//! position's counts, by the core's own conversion, so that the image
//! holds two integers and no code to parse a number with a fraction; and
//! write the keys an image to stage must be signed by, from
//! `BARK_SIGNING_KEYS`.

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
    fs::write(out.join("keys.rs"), keys()).expect("write keys.rs");
}

/// The signing keys as Rust, a `[Key; SLOTS]`, from `BARK_SIGNING_KEYS`:
/// up to three `id:hex`, comma-separated, each a byte from 1 to 255 that
/// names the key in a signature's slot and its Ed25519 public key. An
/// open build without the variable takes the bench key, `keys/bench.pub`.
/// A protected build without the variable holds no key, so stages no
/// image, and `scripts/uf2.py` refuses to pack it; a protected build
/// refuses the bench key, whose private half is in the repository.
fn keys() -> String {
    use bark_image::sign::{KEY_LEN, SLOTS};
    println!("cargo:rerun-if-env-changed=BARK_SIGNING_KEYS");
    println!("cargo:rerun-if-env-changed=BARK_OPEN");
    println!("cargo:rerun-if-changed=keys/bench.pub");
    let open = env::var("BARK_OPEN").is_ok_and(|v| v == "1");
    let bench = fs::read_to_string("keys/bench.pub").expect("read keys/bench.pub");
    let bench = bench.trim();
    // Empty is unset, as a workflow passes a variable nobody set.
    let list = match env::var("BARK_SIGNING_KEYS") {
        Ok(v) if !v.trim().is_empty() => v,
        _ if open => bench.to_string(),
        _ => {
            println!(
                "cargo:warning=a protected firmware without BARK_SIGNING_KEYS holds \
                 no key and accepts no staged image; scripts/uf2.py will not pack it \
                 (bark-fw/README.md, \"Updates\", Keys)"
            );
            String::new()
        }
    };
    let bench_public = bench.split_once(':').expect("keys/bench.pub is id:hex").1;
    let mut keys = Vec::new();
    for entry in list.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let (id, public) = entry
            .split_once(':')
            .unwrap_or_else(|| panic!("BARK_SIGNING_KEYS: {entry:?} is not id:hex"));
        let id: u8 = id
            .parse()
            .ok()
            .filter(|&id| id != 0)
            .unwrap_or_else(|| panic!("BARK_SIGNING_KEYS: the id {id:?} is not 1 to 255"));
        let public = public.to_ascii_lowercase();
        assert!(
            public.len() == 2 * KEY_LEN && public.bytes().all(|b| b.is_ascii_hexdigit()),
            "BARK_SIGNING_KEYS: key {id} is not {KEY_LEN} bytes of hex"
        );
        assert!(
            open || public != bench_public,
            "BARK_SIGNING_KEYS: a protected build may not hold the bench key, \
             whose private key is in the repository"
        );
        assert!(
            keys.iter().all(|(other, _)| *other != id),
            "BARK_SIGNING_KEYS: the id {id} is given twice"
        );
        keys.push((id, public));
    }
    assert!(
        keys.len() <= SLOTS,
        "BARK_SIGNING_KEYS: at most {SLOTS} keys"
    );
    let mut rust = String::from("[");
    for n in 0..SLOTS {
        match keys.get(n) {
            Some((id, public)) => {
                let bytes: Vec<String> = (0..KEY_LEN)
                    .map(|i| format!("0x{}", &public[2 * i..2 * i + 2]))
                    .collect();
                rust += &format!("Key {{ id: {id}, public: [{}] }}, ", bytes.join(", "));
            }
            None => rust += "Key::NONE, ",
        }
    }
    rust + "]"
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
