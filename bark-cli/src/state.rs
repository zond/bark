//! The tool's own state, in a directory of its own: the nodes it is
//! paired with. The bond itself is BlueZ's.
//!
//! `paired` holds a first line naming its layout, `paired 1`, then a line
//! for each node: its ID and its Bluetooth address.

use std::path::{Path, PathBuf};

use bark_proto::NodeId;

use crate::link::{id_of_name, id_text, Error, Result};
use crate::phone::Select;

/// The layout of `paired`; a later one counts on.
const LAYOUT: u32 = 1;

/// A node this tool is paired with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paired {
    pub id: NodeId,
    pub address: String,
}

#[derive(Debug, Default)]
pub struct State {
    /// None for a state kept in memory alone, as the tests keep it.
    dir: Option<PathBuf>,
    pub paired: Vec<Paired>,
}

/// `$XDG_DATA_HOME/bark`, else `~/.local/share/bark` (the XDG Base
/// Directory Specification's default).
pub fn default_dir() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("bark"))
}

fn fail(what: &str, path: &Path, e: impl std::fmt::Display) -> Error {
    Error::Other(format!("{what} {}: {e}", path.display()))
}

impl State {
    pub fn in_memory() -> State {
        State::default()
    }

    /// The state in `dir`; empty where there is none yet.
    pub fn load(dir: &Path) -> Result<State> {
        let path = dir.join("paired");
        let mut state = State {
            dir: Some(dir.to_path_buf()),
            paired: Vec::new(),
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(state),
            Err(e) => return Err(fail("reading", &path, e)),
        };
        let mut lines = text.lines();
        if lines.next() != Some(&format!("paired {LAYOUT}")) {
            return Err(fail("reading", &path, "not a layout this tool reads"));
        }
        for line in lines.filter(|l| !l.is_empty()) {
            let parsed = line.split_once(' ').and_then(|(id, address)| {
                Some(Paired {
                    id: id_of_name(&format!("{}{id}", bark_link::NAME_PREFIX))?,
                    address: address.to_string(),
                })
            });
            state
                .paired
                .push(parsed.ok_or_else(|| fail("reading", &path, format!("{line:?}")))?);
        }
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        std::fs::create_dir_all(dir).map_err(|e| fail("making", dir, e))?;
        let mut text = format!("paired {LAYOUT}\n");
        for p in &self.paired {
            text += &format!("{} {}\n", id_text(p.id), p.address);
        }
        // Written aside and renamed, so a crash leaves the old file whole.
        let path = dir.join("paired");
        let new = dir.join("paired.new");
        std::fs::write(&new, text).map_err(|e| fail("writing", &new, e))?;
        std::fs::rename(&new, &path).map_err(|e| fail("writing", &path, e))
    }

    /// The one paired node `sel` names.
    pub fn pick(&self, sel: &Select) -> Result<Paired> {
        let mut matching: Vec<&Paired> = self.paired.iter().filter(|p| sel.matches(p.id)).collect();
        match matching.len() {
            0 => Err(Error::Other(format!(
                "not paired with a node {sel}: pair first (bark pair {sel})"
            ))),
            1 => Ok(matching.remove(0).clone()),
            _ => Err(Error::Other(format!(
                "{sel} names more than one paired node: give more of the ID"
            ))),
        }
    }

    pub fn add(&mut self, p: Paired) {
        self.remove(p.id);
        self.paired.push(p);
    }

    pub fn remove(&mut self, id: NodeId) {
        self.paired.retain(|p| p.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paired_nodes_last_across_a_load() {
        let dir = std::env::temp_dir().join(format!("bark-cli-state-{}", std::process::id()));
        let mut s = State::load(&dir).unwrap();
        assert!(s.paired.is_empty());
        s.add(Paired {
            id: NodeId::from_u64(0xbae6_e6cb_f4f5),
            address: "E2:49:1C:03:EF:88".into(),
        });
        s.save().unwrap();
        let s = State::load(&dir).unwrap();
        assert_eq!(s.paired.len(), 1);
        assert_eq!(
            s.pick(&"f4f5".parse().unwrap()).unwrap().address,
            "E2:49:1C:03:EF:88"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
