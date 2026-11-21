//! A firmware update of a paired node from an image file, as the app sends
//! one (spec "Updates from the phone"): begin with the header, the image
//! in pieces with a read of Update after each window, commit, reboot.

use std::time::Duration;

use bark_image::{Header, HEADER_LEN};
use bark_link::update::{Command, Piece, Progress, Refusal, State as UpdateState};
use bark_link::Clock;
use sha2::{Digest, Sha256};

use crate::link::{id_text, Char, Error, Link, Result, Transport};
use crate::phone::{self, Select};
use crate::state::State;

/// Pieces written between two reads of Update: the node's host queues 8
/// ATT PDUs for a link, so seven pieces and the read fit, and the read,
/// answered once the pieces before it are written, is the flow control
/// (spec "Updates from the phone").
const WINDOW: usize = 7;

/// Windows in a row that move the bytes held not at all before the tool
/// gives up, as the app does: a node that takes no piece of 7 × 20.
const STALLED_WINDOWS: usize = 20;

/// How long the tool waits for the link's end after a reboot whose answer
/// was lost: the longest supervision timeout, 32 s (Vol 6 Part B §4.5.2),
/// as the app does.
const REBOOT_END: Duration = Duration::from_secs(32);

/// An image file: its header, then its bytes (`bark-fw/README.md`,
/// "Updates"), as a release carries it, `bark-fw-<version>.img`.
pub struct Image {
    pub header: [u8; HEADER_LEN],
    pub fields: Header,
    pub bytes: Vec<u8>,
}

impl Image {
    /// The image in `file`, if it is one: its header's magic, format and
    /// length, and its bytes' hash, the header's.
    pub fn parse(file: &[u8]) -> Result<Image> {
        let bad = |why: &str| Error::Other(format!("not a firmware image: {why}"));
        let (head, bytes) = file
            .split_first_chunk::<HEADER_LEN>()
            .ok_or_else(|| bad("too short"))?;
        let fields = Header::decode(head).map_err(|e| bad(&format!("{e:?}")))?;
        if fields.length as usize != bytes.len() {
            return Err(bad("its length is not the header's"));
        }
        if Sha256::digest(bytes)[..] != fields.hash {
            return Err(bad("its bytes do not hash to the header's"));
        }
        Ok(Image {
            header: *head,
            fields,
            bytes: bytes.to_vec(),
        })
    }

    /// Whether the node, by `p`, is staging or has staged this image.
    fn held_by(&self, p: &Progress) -> bool {
        p.state != UpdateState::Idle && p.hash == self.fields.hash
    }
}

/// Why the node refused, for the person; the app's words.
fn why(r: Option<Refusal>) -> &'static str {
    match r {
        Some(Refusal::Header) => "the file is not firmware this node reads",
        Some(Refusal::Version) => "the firmware is not newer than the version the node runs",
        Some(Refusal::Busy) => "the node holds checked new firmware already: reboot it first",
        Some(Refusal::Incomplete) => "the node does not hold every byte of the firmware",
        Some(Refusal::Hash) => "the bytes the node holds are not the firmware's; send it again",
        Some(Refusal::Battery) => {
            "the node is on a low battery; it keeps the firmware, to go on once it is on USB or charged"
        }
        Some(Refusal::Flash) => "the node's flash memory failed",
        Some(Refusal::Signature) => {
            "the firmware is not signed by a key the node holds: a file built here is signed with `image.py sign`, a release's is signed already"
        }
        None => "it did not say why",
    }
}

fn progress<L: Link>(l: &mut L) -> Result<Progress> {
    Progress::decode(&l.read(Char::Update)?)
        .map_err(|_| Error::Other("the node's Update value is malformed".into()))
}

/// Writes a begin, a commit or a reboot; a refusal, 0x81, says why from
/// Update.
fn command<L: Link>(l: &mut L, c: Command) -> Result<()> {
    let mut b = [0; Command::MAX_LEN];
    let len = c.encode(&mut b);
    match l.write(Char::Update, &b[..len]) {
        Err(Error::Att(bark_link::att::REFUSED)) => {
            let r = progress(l).ok().and_then(|p| p.refused);
            Err(Error::Other(format!("the node refused: {}", why(r))))
        }
        r => r,
    }
}

/// Updates the paired node `sel` names to `image`, from what it holds
/// already if that is this image's, and asks it to reboot into it; each
/// step goes to `say` as it happens. As the app, an image not newer than
/// the node runs is refused before anything is sent.
pub fn update<T: Transport>(
    t: &mut T,
    state: &mut State,
    sel: &Select,
    image: &Image,
    clock: Clock,
    say: &mut dyn FnMut(String),
) -> Result<()> {
    let (n, mut l) = phone::paired_link(t, state, sel, clock)?;
    let version = image.fields.version;
    if version <= n.image {
        return Err(Error::Other(format!(
            "the image is version {version} and the node runs {}: it takes only a newer one",
            n.image
        )));
    }
    let mut p = progress(&mut l)?;
    if image.held_by(&p) {
        say(format!("resumed {} of {} bytes", p.held, image.bytes.len()));
    } else {
        command(&mut l, Command::Begin(&image.header))?;
        say(format!(
            "began version {version}, {} bytes",
            image.bytes.len()
        ));
        p = progress(&mut l)?;
    }
    let piece = (bark_link::value_room(l.mtu()) - Piece::HEADER).min(Piece::MAX_BYTES);
    let len = image.bytes.len();
    let mut stalled = 0;
    let mut told = 0;
    while image.held_by(&p) && p.state == UpdateState::Receiving {
        let from = p.held as usize;
        let mut at = from;
        for _ in 0..WINDOW {
            if at >= len {
                break;
            }
            let end = (at + piece).min(len);
            let mut b = [0; Piece::MAX_LEN];
            let n = Piece {
                offset: at as u32,
                bytes: &image.bytes[at..end],
            }
            .encode(&mut b);
            l.write_command(Char::Image, &b[..n])?;
            at = end;
        }
        p = progress(&mut l)?;
        stalled = if p.held as usize > from {
            0
        } else {
            stalled + 1
        };
        if stalled >= STALLED_WINDOWS {
            return Err(Error::Other("the node takes no pieces".into()));
        }
        // A line at each tenth of the image.
        let tenth = p.held as usize * 10 / len;
        if tenth > told {
            told = tenth;
            say(format!("sent {} of {len} bytes", p.held));
        }
    }
    match (image.held_by(&p), p.state) {
        (true, UpdateState::Staged) => {
            command(&mut l, Command::Commit)?;
            say(format!("committed version {version}: the node checked it"));
        }
        (true, UpdateState::Committed) => say(format!("version {version} is committed already")),
        _ => {
            return Err(Error::Other(format!(
                "the node lost the image: {}",
                why(p.refused)
            )))
        }
    }
    match command(&mut l, Command::Reboot) {
        Ok(()) => {}
        // The node answers, then ends the link, and the end may overtake
        // the answer.
        Err(e @ Error::Other(_)) => return Err(e),
        Err(e) => {
            if !phone::ends(&mut l, REBOOT_END) {
                return Err(e);
            }
        }
    }
    say(format!(
        "rebooting into version {version}: the node is away about a minute; bark node {} shows it back",
        id_text(n.id)
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeNode, FakeRadio};
    use crate::phone::SCAN_TIME;
    use bark_proto::NodeId;

    const CLOCK: Clock = Clock {
        utc_s: 1_795_000_000,
        offset_min: 0,
    };

    fn file(version: u32, len: usize) -> Vec<u8> {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
        let h = Header {
            version,
            length: len as u32,
            hash: Sha256::digest(&bytes).into(),
            table: [0; 32],
        };
        let mut f = h.encode().to_vec();
        f.extend_from_slice(&bytes);
        f
    }

    fn paired() -> (FakeRadio, State) {
        let mut radio = FakeRadio::new(vec![FakeNode::new(NodeId::from_u64(0x3112)).image(46)]);
        let mut state = State::in_memory();
        phone::pair(
            &mut radio,
            &mut state,
            &"3112".parse().unwrap(),
            SCAN_TIME,
            Some(123_456),
            CLOCK,
        )
        .unwrap();
        (radio, state)
    }

    fn run(radio: &mut FakeRadio, state: &mut State, image: &Image) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        update(
            radio,
            state,
            &"3112".parse().unwrap(),
            image,
            CLOCK,
            &mut |l| lines.push(l),
        )?;
        Ok(lines)
    }

    #[test]
    fn an_image_goes_in_pieces_and_the_node_reboots_into_it() {
        let (mut radio, mut state) = paired();
        let image = Image::parse(&file(47, 10_000)).unwrap();
        let lines = run(&mut radio, &mut state, &image).unwrap();
        assert!(lines[0].starts_with("began version 47"), "{lines:?}");
        assert!(lines
            .last()
            .unwrap()
            .starts_with("rebooting into version 47"));
        assert_eq!(radio.nodes[0].borrow().node.image, 47);
    }

    #[test]
    fn a_lost_piece_is_sent_again_from_what_the_node_holds() {
        let (mut radio, mut state) = paired();
        radio.nodes[0].borrow_mut().drops = 3;
        let image = Image::parse(&file(47, 5_000)).unwrap();
        run(&mut radio, &mut state, &image).unwrap();
        assert_eq!(radio.nodes[0].borrow().node.image, 47);
    }

    #[test]
    fn an_image_not_newer_is_refused_before_it_is_sent() {
        let (mut radio, mut state) = paired();
        let image = Image::parse(&file(46, 1_000)).unwrap();
        let e = run(&mut radio, &mut state, &image).unwrap_err();
        assert!(e.to_string().contains("only a newer one"), "{e}");
        assert_eq!(radio.nodes[0].borrow().update.state, UpdateState::Idle);
    }

    #[test]
    fn a_file_that_is_not_an_image_is_refused() {
        let mut f = file(47, 1_000);
        assert!(Image::parse(&f[..100]).is_err());
        let last = f.len() - 1;
        f[last] ^= 1;
        let e = Image::parse(&f).err().unwrap();
        assert!(e.to_string().contains("hash"), "{e}");
    }
}
