//! The store on two pages of flash in RAM, and power cuts at every word
//! written and every erase of its appends and its turns.

extern crate std;

use std::cell::RefCell;
use std::rc::Rc;
use std::vec;
use std::vec::Vec;

use bark_link::phones::{Keys, Name};
use bark_proto::{Channel, Position};
use embedded_storage_async::nor_flash::{
    ErrorType, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};

use super::*;

/// The power went: every later operation fails until the next boot.
#[derive(Debug)]
struct Cut;

impl NorFlashError for Cut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

/// What a cut leaves of the operation it lands in.
#[derive(Clone, Copy, Debug)]
struct Leaves {
    /// A word cut while written has half the bits it was to clear cleared;
    /// otherwise none.
    half_word: bool,
    /// A page cut while erased has its second half erased, its head left
    /// as it was; otherwise its first half, the head with it.
    head_survives: bool,
}

/// Both ways a cut may leave a word and a page.
const LEAVES: [Leaves; 2] = [
    Leaves {
        half_word: true,
        head_survives: true,
    },
    Leaves {
        half_word: false,
        head_survives: false,
    },
];

/// The two pages in RAM, as the nRF52840's NVMC writes them (Product
/// Specification, NVMC): an erase sets a page to `0xFF`, a write clears
/// bits a word at a time, in address order.
struct Chip {
    bytes: Vec<u8>,
    /// Operations, each word written and each page erased, before the one
    /// the power is cut in; none for a supply that holds.
    budget: Option<usize>,
    leaves: Leaves,
    dead: bool,
    /// Operations done.
    ops: usize,
    /// Erases of each page.
    erases: [usize; 2],
}

#[derive(Clone)]
struct Ram(Rc<RefCell<Chip>>);

impl Ram {
    fn new(leaves: Leaves) -> Ram {
        Ram(Rc::new(RefCell::new(Chip {
            bytes: vec![0xff; STORE.len as usize],
            budget: None,
            leaves,
            dead: false,
            ops: 0,
            erases: [0; 2],
        })))
    }

    /// Whether the next operation is the one the power is cut in.
    fn cut_now(&self) -> Result<bool, Cut> {
        let mut c = self.0.borrow_mut();
        if c.dead {
            return Err(Cut);
        }
        c.ops += 1;
        match &mut c.budget {
            Some(0) => {
                c.dead = true;
                Ok(true)
            }
            Some(n) => {
                *n -= 1;
                Ok(false)
            }
            None => Ok(false),
        }
    }

    fn power(&self, budget: Option<usize>) {
        let mut c = self.0.borrow_mut();
        c.budget = budget;
        c.dead = false;
    }

    fn dead(&self) -> bool {
        self.0.borrow().dead
    }

    /// The next boot's read of the pages.
    fn boot(&self) -> (Store<Ram>, Kept) {
        let (log, kept) = scan(|at, out| {
            let c = self.0.borrow();
            out.copy_from_slice(&c.bytes[at as usize..at as usize + out.len()]);
            true
        });
        (Store::new(self.clone(), log, &kept), kept)
    }
}

impl ErrorType for Ram {
    type Error = Cut;
}

impl ReadNorFlash for Ram {
    const READ_SIZE: usize = 1;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        let c = self.0.borrow();
        bytes.copy_from_slice(&c.bytes[offset as usize..offset as usize + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        STORE.len as usize
    }
}

impl NorFlash for Ram {
    const WRITE_SIZE: usize = 4;
    const ERASE_SIZE: usize = PAGE as usize;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Cut> {
        assert!(from.is_multiple_of(PAGE) && to == from + PAGE);
        let cut = self.cut_now()?;
        let mut c = self.0.borrow_mut();
        c.erases[(from / PAGE) as usize] += 1;
        let (at, half) = (from as usize, PAGE as usize / 2);
        let erased = match (cut, c.leaves.head_survives) {
            (false, _) => at..at + 2 * half,
            (true, true) => at + half..at + 2 * half,
            (true, false) => at..at + half,
        };
        c.bytes[erased].fill(0xff);
        if cut {
            return Err(Cut);
        }
        Ok(())
    }

    async fn write(&mut self, offset: u32, data: &[u8]) -> Result<(), Cut> {
        assert!(offset.is_multiple_of(4) && data.len().is_multiple_of(4));
        for (k, word) in data.chunks(4).enumerate() {
            let cut = self.cut_now()?;
            let mut c = self.0.borrow_mut();
            let at = offset as usize + 4 * k;
            let half = c.leaves.half_word;
            for (cell, &b) in c.bytes[at..at + 4].iter_mut().zip(word) {
                *cell &= match (cut, half) {
                    (false, _) => b,
                    (true, true) => b | 0x55,
                    (true, false) => 0xff,
                };
            }
            if cut {
                return Err(Cut);
            }
        }
        Ok(())
    }
}

/// The value of a future that does not wait: the flash in RAM never does.
fn now<T>(f: impl core::future::Future<Output = T>) -> T {
    let mut f = core::pin::pin!(f);
    let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(v) => v,
        core::task::Poll::Pending => panic!("the flash in RAM waited"),
    }
}

fn keys(n: u8) -> Keys {
    Keys {
        kind: 1,
        addr: [n, 2, 3, 4, 5, 0xc6],
        irk: Some([n; 16]),
        ltk: [n.wrapping_add(1); 16],
        seen: 1,
        service_changed: true,
    }
}

fn stored(n: u32) -> Stored {
    Stored {
        home: Channel::new((n % 13) as u8).unwrap(),
        place: Position::from_degrees(50.0 + f64::from(n) * 1e-4, 30.5),
        carried: n % 2 == 1,
        map_point: false,
        anchor: None,
        relay: true,
        battery_scale: Some(n as u16),
        drain: None,
    }
}

/// What the node holds, as the store should give it back.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    stored: Option<Stored>,
    offset_min: Option<i16>,
    records: [Option<Record>; RECORDS],
    settings: phones::Settings,
}

fn held(stored: Option<Stored>, offset_min: Option<i16>, p: &Phones) -> Held {
    Held {
        stored,
        offset_min,
        records: p.records,
        settings: p.settings(),
    }
}

fn read_back(kept: &Kept) -> Held {
    held(kept.stored, kept.offset_min, &kept.phones.phones())
}

/// A node's RAM: what the store is handed.
struct Node {
    stored: Option<Stored>,
    offset_min: Option<i16>,
    phones: Phones,
}

impl Node {
    fn held(&self) -> Held {
        held(self.stored, self.offset_min, &self.phones)
    }
}

/// A change to what the node holds, each written as one entry.
#[derive(Clone, Copy, Debug)]
enum Step {
    Stored(u32),
    Offset(i16),
    /// A person's progress: their record's progress entry.
    Progress(usize, u32),
    /// A person seen on a day: their record whole.
    Seen(usize, u16),
    Private(bool),
}

/// Six phones paired, written by the first turn.
fn node() -> Node {
    let mut phones = Phones::EMPTY;
    for n in 0..6 {
        let i = phones.pair(keys(n), 0).unwrap().index();
        phones.set_recipient(i, n, Name::new(b"Ola").unwrap());
    }
    phones.take_dirty();
    Node {
        stored: None,
        offset_min: None,
        phones,
    }
}

/// Enough changes for six turns, of every kind.
fn steps() -> Vec<Step> {
    (0..400u32)
        .map(|n| match n % 7 {
            0 => Step::Stored(n),
            1 => Step::Offset(n as i16 - 200),
            2..=4 => Step::Progress((n % 6) as usize, n),
            5 => Step::Seen((n % 6) as usize, n as u16 + 1),
            _ => Step::Private(n % 2 == 0),
        })
        .collect()
}

/// Take `step` in the node's RAM and hand the store its entry, as the
/// node and `bark-fw`'s store task do.
fn take(store: &mut Store<Ram>, node: &mut Node, step: Step) -> Result<(), Cut> {
    match step {
        Step::Stored(n) => {
            node.stored = Some(stored(n));
            return now(store.keep(stored(n), &node.phones));
        }
        Step::Offset(o) => {
            node.offset_min = Some(o);
            return now(store.keep_offset(o, &node.phones));
        }
        Step::Progress(i, n) => node.phones.progress(i, n, n + 1),
        Step::Seen(i, day) => node.phones.seen(i, day),
        Step::Private(p) => node.phones.set_private(p),
    }
    let d = node.phones.take_dirty();
    let p = &node.phones;
    let entry: Vec<u8> = match (d.records, d.progress, d.settings) {
        (0, 0, false) => return Ok(()),
        (r, 0, false) if r.count_ones() == 1 => {
            let i = r.trailing_zeros() as usize;
            Latest::record(p, i).unwrap().to_vec()
        }
        (0, r, false) if r.count_ones() == 1 => {
            let i = r.trailing_zeros() as usize;
            p.progress_of(i).unwrap().encode().to_vec()
        }
        (0, 0, true) => Latest::settings(p).to_vec(),
        _ => panic!("a step writes one entry"),
    };
    now(store.write(&entry, p))
}

/// Run `steps` from the first, each written, until the power is cut:
/// the step it was cut in, none if every step was written.
fn run(store: &mut Store<Ram>, node: &mut Node, steps: &[Step]) -> Option<usize> {
    for (j, &s) in steps.iter().enumerate() {
        if take(store, node, s).is_err() {
            return Some(j);
        }
    }
    None
}

/// What the node holds after each step: `after[j]` after the first `j`.
fn after(steps: &[Step]) -> Vec<Held> {
    let ram = Ram::new(LEAVES[0]);
    let (mut store, _) = ram.boot();
    let mut node = node();
    let mut held = vec![read_back(&ram.boot().1)];
    for &s in steps {
        take(&mut store, &mut node, s).unwrap();
        held.push(node.held());
    }
    held
}

/// Cut the power at each operation of `steps` in turn; at the next boot
/// the store holds what the node held before the step cut, or after it,
/// and the steps after it, written from there, are all held at the boot
/// after.
fn cut_everywhere(leaves: Leaves) -> usize {
    let steps = steps();
    let held = after(&steps);
    for k in 0.. {
        let ram = Ram::new(leaves);
        ram.power(Some(k));
        let (mut store, _) = ram.boot();
        let mut node = node();
        let Some(j) = run(&mut store, &mut node, &steps) else {
            return k;
        };
        assert!(ram.dead(), "only a cut fails a write");
        ram.power(None);
        let (mut store, kept) = ram.boot();
        let back = read_back(&kept);
        assert!(
            back == held[j] || back == held[j + 1],
            "cut at {k}, in step {j} {:?}",
            steps[j]
        );
        let mut node = Node {
            stored: kept.stored,
            offset_min: kept.offset_min,
            phones: kept.phones.phones(),
        };
        assert_eq!(run(&mut store, &mut node, &steps[j + 1..]), None);
        assert_eq!(read_back(&ram.boot().1), node.held(), "cut at {k}");
    }
    unreachable!()
}

#[test]
fn a_power_cut_at_any_word_or_erase_loses_nothing_kept_before_it() {
    let steps = steps();
    let ram = Ram::new(LEAVES[0]);
    let (mut store, _) = ram.boot();
    assert_eq!(run(&mut store, &mut node(), &steps), None);
    let (ops, erases) = {
        let c = ram.0.borrow();
        (c.ops, c.erases)
    };
    // Six turns, three to each page, among 5 838 words and erases.
    assert_eq!((erases, ops), ([3, 3], 5838));
    for leaves in LEAVES {
        assert_eq!(cut_everywhere(leaves), ops, "{leaves:?}");
    }
}

#[test]
fn the_pages_take_turns() {
    let ram = Ram::new(LEAVES[0]);
    let (mut store, _) = ram.boot();
    let mut node = node();
    for n in 0..2000 {
        take(&mut store, &mut node, Step::Stored(n)).unwrap();
        let [a, b] = ram.0.borrow().erases;
        assert!(a == b || a == b + 1, "erases {a} and {b}");
    }
    assert_eq!(read_back(&ram.boot().1), node.held());
    // A turn writes the head, the state, the settings and six records, 27
    // slots: the first write turns, and then every 102nd, after 101
    // appends fill the page.
    let [a, b] = ram.0.borrow().erases;
    assert_eq!(a + b, 1 + (2000 - 1) / 102);
}

#[test]
fn the_later_valid_page_holds() {
    use Head::{Erased, Invalid, Valid};
    assert_eq!(choose([Erased, Erased]), None);
    assert_eq!(choose([Invalid, Erased]), None);
    assert_eq!(choose([Valid(4), Erased]), Some((0, 4)));
    assert_eq!(choose([Invalid, Valid(4)]), Some((1, 4)));
    assert_eq!(choose([Valid(4), Valid(5)]), Some((1, 5)));
    assert_eq!(choose([Valid(6), Valid(5)]), Some((0, 6)));
    // The count wraps.
    assert_eq!(choose([Valid(u16::MAX), Valid(0)]), Some((1, 0)));
    assert_eq!(choose([Valid(0), Valid(u16::MAX)]), Some((0, 0)));
    assert_eq!(head(&head_entry(7)), Valid(7));
    let mut cut = head_entry(7);
    cut[4..8].fill(0xff);
    assert_eq!(head(&cut), Invalid);
}

/// A board that kept layout 2, one page at the second page's place, is
/// read as holding nothing, and says so; its first write turns to the
/// other page.
#[test]
fn a_page_of_layout_2_is_discarded() {
    let ram = Ram::new(LEAVES[0]);
    {
        let mut c = ram.0.borrow_mut();
        let old = PAGE as usize;
        c.bytes[old..old + SLOT_LEN].copy_from_slice(&word_entry([HEAD, 2, 0, 0]));
        c.bytes[old + SLOT_LEN..old + 2 * SLOT_LEN].copy_from_slice(&stored(1).record());
    }
    let (mut store, kept) = ram.boot();
    assert!(kept.discarded);
    assert_eq!(kept.stored, None);
    let mut node = node();
    take(&mut store, &mut node, Step::Stored(2)).unwrap();
    let kept = ram.boot().1;
    assert!(!kept.discarded);
    assert_eq!(read_back(&kept), node.held());
    assert_eq!(ram.0.borrow().erases, [1, 0]);
}
