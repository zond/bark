//! The tool's own state, in a directory of its own: the nodes it is
//! paired with, the person's identity, their contacts, and how far it has
//! read each node's ring as a guest. The bond itself is BlueZ's.
//!
//! Each file's first line names it and its layout, such as `paired 1`:
//! - `paired`: a line for each node, its ID and its Bluetooth address;
//! - `me`: the person ([`Me::save`]); it holds the seed every key comes
//!   from, so it is written readable by its owner alone;
//! - `contacts`: a line for each contact ([`Contact::save`]);
//! - `rings`: a line for each node read as a guest, its ID and the number
//!   of the last item taken (spec "Public nodes").

use std::path::{Path, PathBuf};

use bark_proto::NodeId;

use crate::link::{id_of_name, id_text, Error, Result};
use crate::person::{random, Contact, Me};
use crate::phone::Select;

/// The layout of each file; a later one counts on.
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
    pub me: Option<Me>,
    pub contacts: Vec<Contact>,
    /// The number of the last ring item taken as a guest, by node.
    pub rings: Vec<(NodeId, u32)>,
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

fn node_of(hex: &str) -> Option<NodeId> {
    id_of_name(&format!("{}{hex}", bark_link::NAME_PREFIX))
}

/// The lines of the file `name` in `dir` after its layout line; none when
/// it does not exist yet.
fn read(dir: &Path, name: &str) -> Result<Vec<String>> {
    let path = dir.join(name);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(fail("reading", &path, e)),
    };
    let mut lines = text.lines();
    if lines.next() != Some(&format!("{name} {LAYOUT}")) {
        return Err(fail("reading", &path, "not a layout this tool reads"));
    }
    Ok(lines.filter(|l| !l.is_empty()).map(String::from).collect())
}

/// Writes `body` as the file `name`, aside and renamed, so a crash leaves
/// the old file whole; `private` ones readable by their owner alone.
fn write(dir: &Path, name: &str, body: &str, private: bool) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = dir.join(name);
    let new = dir.join(format!("{name}.new"));
    let mode = if private { 0o600 } else { 0o644 };
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(&new)
        .map_err(|e| fail("writing", &new, e))?;
    f.write_all(format!("{name} {LAYOUT}\n{body}").as_bytes())
        .map_err(|e| fail("writing", &new, e))?;
    std::fs::rename(&new, &path).map_err(|e| fail("writing", &path, e))
}

impl State {
    pub fn in_memory() -> State {
        State::default()
    }

    /// The state in `dir`; empty where there is none yet.
    pub fn load(dir: &Path) -> Result<State> {
        let bad = |name: &str, line: &str| fail("reading", &dir.join(name), format!("{line:?}"));
        let mut state = State {
            dir: Some(dir.to_path_buf()),
            ..State::default()
        };
        for line in read(dir, "paired")? {
            let parsed = line.split_once(' ').and_then(|(id, address)| {
                Some(Paired {
                    id: node_of(id)?,
                    address: address.to_string(),
                })
            });
            state
                .paired
                .push(parsed.ok_or_else(|| bad("paired", &line))?);
        }
        let me = read(dir, "me")?;
        if !me.is_empty() {
            let text = me.join("\n");
            state.me = Some(Me::load(&text).ok_or_else(|| bad("me", "a field"))?);
        }
        for line in read(dir, "contacts")? {
            state
                .contacts
                .push(Contact::load(&line).ok_or_else(|| bad("contacts", &line))?);
        }
        for line in read(dir, "rings")? {
            let parsed = line
                .split_once(' ')
                .and_then(|(id, n)| Some((node_of(id)?, n.parse().ok()?)));
            state.rings.push(parsed.ok_or_else(|| bad("rings", &line))?);
        }
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        std::fs::create_dir_all(dir).map_err(|e| fail("making", dir, e))?;
        let paired: String = self
            .paired
            .iter()
            .map(|p| format!("{} {}\n", id_text(p.id), p.address))
            .collect();
        write(dir, "paired", &paired, false)?;
        if let Some(me) = &self.me {
            write(dir, "me", &me.save(), true)?;
        }
        let contacts: String = self.contacts.iter().map(|c| c.save() + "\n").collect();
        write(dir, "contacts", &contacts, false)?;
        let rings: String = self
            .rings
            .iter()
            .map(|(id, n)| format!("{} {n}\n", id_text(*id)))
            .collect();
        write(dir, "rings", &rings, false)
    }

    /// The person, made with a fresh identity the first time.
    pub fn me(&mut self) -> Result<&mut Me> {
        if self.me.is_none() {
            self.me = Some(Me::new(random()?));
            self.save()?;
        }
        Ok(self.me.as_mut().expect("made above"))
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

    /// The contact `name` names: its name, ignoring case, or the start of
    /// its identity in hex.
    pub fn contact(&self, name: &str) -> Result<usize> {
        let by_name: Vec<usize> = (0..self.contacts.len())
            .filter(|&i| self.contacts[i].name.eq_ignore_ascii_case(name))
            .collect();
        let found = if by_name.is_empty() {
            let lower = name.to_ascii_lowercase();
            (0..self.contacts.len())
                .filter(|&i| {
                    !lower.is_empty()
                        && crate::person::hex(&self.contacts[i].identity().0).starts_with(&lower)
                })
                .collect()
        } else {
            by_name
        };
        match found[..] {
            [i] => Ok(i),
            [] => Err(Error::Other(format!(
                "no contact {name:?}: add one (bark contact add)"
            ))),
            _ => Err(Error::Other(format!(
                "{name:?} names more than one contact: give more of the identity"
            ))),
        }
    }

    /// The number of the last ring item taken at `node` as a guest.
    pub fn ring(&self, node: NodeId) -> u32 {
        self.rings.iter().find(|r| r.0 == node).map_or(0, |r| r.1)
    }

    pub fn set_ring(&mut self, node: NodeId, after: u32) {
        self.rings.retain(|r| r.0 != node);
        self.rings.push((node, after));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_lasts_across_a_load() {
        let dir = std::env::temp_dir().join(format!("bark-cli-state-{}", std::process::id()));
        let mut s = State::load(&dir).unwrap();
        assert!(s.paired.is_empty() && s.me.is_none());
        s.add(Paired {
            id: NodeId::from_u64(0xbae6_e6cb_f4f5),
            address: "E2:49:1C:03:EF:88".into(),
        });
        let card = s.me().unwrap().card();
        s.contacts.push(Contact::of_card("Pal", &card).unwrap());
        s.set_ring(NodeId::from_u64(7), 12);
        s.save().unwrap();
        let s = State::load(&dir).unwrap();
        assert_eq!(s.paired.len(), 1);
        assert_eq!(
            s.pick(&"f4f5".parse().unwrap()).unwrap().address,
            "E2:49:1C:03:EF:88"
        );
        assert!(s.me.is_some());
        assert_eq!(s.contact("pal").unwrap(), 0);
        assert_eq!(s.ring(NodeId::from_u64(7)), 12);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("me"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
