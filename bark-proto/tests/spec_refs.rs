//! Every citation of `docs/protocol.md` names a heading that exists.
//!
//! The spec cites its own sections by markdown link (`[owner](#owner)`); the
//! rest of the repository cites them by heading name (`spec "Owner"`, or a
//! list `spec "Owner", "Delivery queue"`). Section numbers are not used, so
//! that moving a section breaks no reference; a renamed heading fails here.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const SPEC: &str = "docs/protocol.md";
const SKIP_DIRS: &[&str] = &[".git", "target", "vendor", ".claude", ".notes"];
const SEPARATORS: &[&str] = &[", and ", ", ", " and ", " to ", " or "];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The anchor a markdown renderer gives a heading.
fn anchor(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn headings(spec: &str) -> Vec<String> {
    spec.lines()
        .filter(|l| l.starts_with("## ") || l.starts_with("### ") || l.starts_with("#### "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .collect()
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory") {
        let path = entry.expect("directory entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name) {
                files(&path, out);
            }
        } else if name.ends_with(".rs") || name.ends_with(".md") || name.ends_with(".toml") {
            out.push(path);
        }
    }
}

/// The names cited after each `spec "`, with the list that may follow it.
fn cited_names(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut names = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("spec \"") {
        let mut at = from + i + "spec ".len();
        loop {
            let rest = &text[at + 1..];
            let Some(end) = rest.find('"') else { break };
            names.push(rest[..end].to_string());
            at += 1 + end + 1;
            match continuation(&text[at..]) {
                Some(skip) => at += skip,
                None => break,
            }
        }
        from = at;
    }
    names
}

/// How far a list of cited names continues to its next quoted name, which
/// may sit on the next line of a comment.
fn continuation(next: &str) -> Option<usize> {
    let sep = SEPARATORS.iter().find(|s| next.starts_with(s.trim_end()))?;
    let mut rest = &next[sep.trim_end().len()..];
    let blank = rest.len() - rest.trim_start().len();
    if blank == 0 {
        return None;
    }
    let broke = rest[..blank].contains('\n');
    rest = rest.trim_start();
    if broke {
        for marker in ["//!", "///", "//", "#"] {
            if let Some(r) = rest.strip_prefix(marker) {
                rest = r.trim_start();
                break;
            }
        }
    }
    rest.starts_with('"').then(|| next.len() - rest.len())
}

/// Section numbers a reader would take for the spec's.
fn numbered_citations(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for prefix in ["spec §", "design §", "protocol §", "spec's §", "design's §"] {
        for (i, _) in text.match_indices(prefix) {
            let before = text[..i].chars().next_back();
            if !matches!(before, Some(c) if c.is_alphanumeric() || c == '-') {
                found.push(text[i..].chars().take(prefix.len() + 6).collect());
            }
        }
    }
    found
}

#[test]
fn spec_headings_are_unique() {
    let spec = fs::read_to_string(root().join(SPEC)).expect("spec");
    let mut seen = HashSet::new();
    for h in headings(&spec) {
        assert!(seen.insert(anchor(&h)), "heading {h:?} is not unique");
    }
}

#[test]
fn spec_links_resolve() {
    let spec = fs::read_to_string(root().join(SPEC)).expect("spec");
    let anchors: HashSet<String> = headings(&spec).iter().map(|h| anchor(h)).collect();
    for (i, _) in spec.match_indices("](#") {
        let rest = &spec[i + 3..];
        let target = &rest[..rest.find(')').expect("closed link")];
        assert!(
            anchors.contains(target),
            "{SPEC}: no heading for link #{target}"
        );
    }
    // A section number left in the spec is a datasheet's.
    for (i, _) in spec.match_indices('§') {
        let before: String = spec[..i]
            .chars()
            .rev()
            .take(40)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert!(
            before.contains("datasheet"),
            "{SPEC}: numbered reference after {before:?}"
        );
    }
}

/// The citations in `text` that name no heading of the spec.
fn stale(rel: &str, text: &str, names: &HashSet<String>) -> Vec<String> {
    let anchors: HashSet<String> = names.iter().map(|h| anchor(h)).collect();
    let mut bad = Vec::new();
    for name in cited_names(text) {
        if !names.contains(&name) {
            bad.push(format!("{rel}: spec \"{name}\""));
        }
    }
    for (i, _) in text.match_indices("protocol.md#") {
        let rest = &text[i + "protocol.md#".len()..];
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(rest.len());
        if !anchors.contains(&rest[..end]) {
            bad.push(format!("{rel}: protocol.md#{}", &rest[..end]));
        }
    }
    for n in numbered_citations(text) {
        bad.push(format!("{rel}: numbered citation {n:?}"));
    }
    bad
}

fn spec_names() -> HashSet<String> {
    let spec = fs::read_to_string(root().join(SPEC)).expect("spec");
    headings(&spec).into_iter().collect()
}

#[test]
fn repository_citations_resolve() {
    let root = root();
    let names = spec_names();
    let mut paths = Vec::new();
    files(&root, &mut paths);
    let mut bad = Vec::new();
    for path in paths {
        let rel = path
            .strip_prefix(&root)
            .expect("under root")
            .to_string_lossy()
            .to_string();
        if rel == SPEC || rel.ends_with("tests/spec_refs.rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        bad.extend(stale(&rel, &text, &names));
    }
    assert!(bad.is_empty(), "stale spec citations:\n{}", bad.join("\n"));
}

#[test]
fn citations_parse() {
    let names = cited_names(
        "as in spec \"Owner\", \"Delivery queue\" and \"Hop count\"; spec \"Nope\" step 6,\n\
         /// (spec \"Delivery\",\n/// \"Bounds\") and \"not cited\"",
    );
    assert_eq!(
        names,
        [
            "Owner",
            "Delivery queue",
            "Hop count",
            "Nope",
            "Delivery",
            "Bounds"
        ]
    );
    assert_eq!(anchor("Positions on the wire"), "positions-on-the-wire");
    assert_eq!(anchor("Hand-over"), "hand-over");
}

#[test]
fn a_stale_citation_fails() {
    let names = spec_names();
    assert!(stale("x", "(spec \"Owner\", \"Hand-over\")", &names).is_empty());
    assert_eq!(stale("x", "(spec \"Owner\", \"Owners\")", &names).len(), 1);
    assert_eq!(
        stale("x", "see protocol.md#no-such-heading", &names).len(),
        1
    );
    assert_eq!(stale("x", "as design §9.3 says", &names).len(), 1);
}
