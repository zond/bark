//! Write the firmware's `memory.x` from the one layout table
//! (`bark-image/src/layout.rs`) and put it on the linker search path; and
//! turn the place given in `BARK_LAT` and `BARK_LON` (degrees) into the
//! position's counts, by the core's own conversion, so that the image
//! holds two integers and no code to parse a number with a fraction; and
//! write the keys an image to stage must be signed by, from `keys/`.

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

/// The signing keys as Rust, a `[Key; SLOTS]`: the project's,
/// `keys/project.pub`, one `id:hex` a line, each id a byte from 1 to 255
/// that names the key in a signature's slot and each key its Ed25519
/// public key, so that any node takes the project's releases whichever
/// build it runs; and in an open build the bench key, `keys/bench.pub`,
/// as well. A fork with keys of its own gives them in `BARK_SIGNING_KEYS`,
/// `id:hex` comma-separated, in place of the project's. A protected build
/// refuses the bench key, whose private half is in the repository.
fn keys() -> String {
    use bark_image::sign::{KEY_LEN, SLOTS};
    println!("cargo:rerun-if-env-changed=BARK_SIGNING_KEYS");
    println!("cargo:rerun-if-env-changed=BARK_OPEN");
    println!("cargo:rerun-if-changed=keys/project.pub");
    println!("cargo:rerun-if-changed=keys/bench.pub");
    let open = env::var("BARK_OPEN").is_ok_and(|v| v == "1");
    let read = |path: &str| fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let bench = read("keys/bench.pub");
    let bench = bench.trim();
    // Empty is unset, as a workflow passes a variable nobody set.
    let (from, mut list) = match env::var("BARK_SIGNING_KEYS") {
        Ok(v) if !v.trim().is_empty() => ("BARK_SIGNING_KEYS", v),
        _ => (
            "keys/project.pub",
            read("keys/project.pub").replace('\n', ","),
        ),
    };
    if open {
        list = format!("{list},{bench}");
    }
    let bench_public = bench.split_once(':').expect("keys/bench.pub is id:hex").1;
    let mut keys = Vec::new();
    for entry in list.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        let (id, public) = entry
            .split_once(':')
            .unwrap_or_else(|| panic!("{from}: {entry:?} is not id:hex"));
        let id: u8 = id
            .parse()
            .ok()
            .filter(|&id| id != 0)
            .unwrap_or_else(|| panic!("{from}: the id {id:?} is not 1 to 255"));
        let public = public.to_ascii_lowercase();
        assert!(
            public.len() == 2 * KEY_LEN && public.bytes().all(|b| b.is_ascii_hexdigit()),
            "{from}: key {id} is not {KEY_LEN} bytes of hex"
        );
        assert!(
            open || public != bench_public,
            "{from}: a protected build may not hold the bench key, \
             whose private key is in the repository"
        );
        assert!(
            keys.iter().all(|(other, _)| *other != id),
            "{from} and the bench key: the id {id} is given twice"
        );
        keys.push((id, public));
    }
    assert!(
        !keys.is_empty(),
        "{from} holds no key, so the firmware would stage no image"
    );
    assert!(
        keys.len() <= SLOTS,
        "{from} and the bench key: at most {SLOTS} keys"
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
