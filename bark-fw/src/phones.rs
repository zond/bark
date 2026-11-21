//! The node's phones (spec "Phone link", Pairing, Phones; roadmap D168):
//! a record for each person paired with it, at most eight, as
//! `bark_link::phones` decides on them, kept here for the phone link
//! (`ble.rs`), the mailbox (`mailbox.rs`) and the screen, and written to
//! flash by the store (`store.rs`) after each change.
//!
//! Every change goes through [`with`], which then hands the store what
//! changed, the mailbox each person as their record says them, the
//! screen its phone line, and the phone link what Phones lists, so that
//! none disagrees for longer than the call.

use core::cell::RefCell;

use bark_app_layer::mailbox::Person;
use bark_link::phones::Phones;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;

static PHONES: Mutex<CriticalSectionRawMutex, RefCell<Phones>> =
    Mutex::new(RefCell::new(Phones::EMPTY));

/// The records as the store's page held them at boot, once, before the
/// phone link and the mailbox start.
pub fn init(phones: Phones) {
    PHONES.lock(|p| *p.borrow_mut() = phones);
    log::info!(
        "phones: {} paired, {} kept, room {}, public {}",
        phones.paired(),
        phones.records.iter().flatten().count(),
        phones.room(),
        phones.public()
    );
    // The mailbox takes its people as it opens.
    crate::display::phone_at_boot(line());
}

/// Read the records.
pub fn read<R>(f: impl FnOnce(&Phones) -> R) -> R {
    PHONES.lock(|p| f(&p.borrow()))
}

/// Change the records; what changed is written to flash, and the mailbox,
/// the screen and the linked phones follow.
pub fn with<R>(f: impl FnOnce(&mut Phones) -> R) -> R {
    let r = PHONES.lock(|p| f(&mut p.borrow_mut()));
    changed();
    r
}

/// What changed since the store last asked, for it to write
/// (`store.rs`); a change while it writes asks again.
pub fn take_dirty() -> bark_link::phones::Dirty {
    PHONES.lock(|p| p.borrow_mut().take_dirty())
}

/// A fetch the mailbox took for record `i`: the record keeps it and the
/// day, and its command runs on the records, the mailbox given its
/// outcome for the summary (`bark_app_layer::command::obey`). The two are
/// changed together, the records' lock inside the mailbox's, so that
/// neither sees the other half done; the rest follow after.
pub fn fetched(i: usize, command: Option<bark_app_layer::command::Command>) {
    let (day, linked) = (today(), crate::ble::linked_records());
    crate::mailbox::with(|m| {
        PHONES.lock(|p| {
            bark_app_layer::command::obey(m, &mut p.borrow_mut(), i, command, day, linked)
        })
    });
    changed();
    if command.is_some() {
        // A record made or freed: the phone link follows.
        crate::ble::records_changed();
    }
}

/// Hand the store, the mailbox, the screen and the phone link what the
/// records say now.
fn changed() {
    crate::store::keep_phones();
    to_mailbox();
    crate::display::phone(line());
    crate::ble::phones_moved();
}

/// Hand the mailbox each person as their record says them; at its
/// opening, and at each change.
pub fn to_mailbox() {
    let people = read(|p| p.records.map(|r| r.as_ref().map(Person::of)));
    crate::mailbox::with(|m| {
        for (i, p) in people.into_iter().enumerate() {
            m.set_person(i, p);
        }
    });
}

/// The mailbox moved person `i` on: their record keeps it.
pub fn progress(i: usize) {
    let Some(p) = crate::mailbox::with(|m| m.person(i)).flatten() else {
        return;
    };
    with(|ph| ph.progress(i, p.read, p.last_fetch));
}

/// The screen's phone line when no code shows
/// (spec "Screen and buttons").
pub fn line() -> bark_screen::Phone {
    use bark_screen::Phone;
    read(|p| match (p.paired(), p.public()) {
        (0, _) => Phone::Unpaired,
        (1, true) => Phone::Public,
        (1, false) => Phone::Private,
        (n, public) => Phone::Phones {
            count: n as u8,
            private: !public,
        },
    })
}

/// Today, counted from 1 on 2026-01-01 UTC as `sent`'s epoch
/// (`bark_app_layer::EPOCH_UNIX_S`), or 0 while the node has no time.
pub fn today() -> u16 {
    const DAY_S: u64 = 24 * 60 * 60;
    crate::utc::time().utc_s.map_or(0, |s| {
        let days = s.saturating_sub(bark_app_layer::EPOCH_UNIX_S) / DAY_S;
        (days + 1).min(u64::from(u16::MAX)) as u16
    })
}
