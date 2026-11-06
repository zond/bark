//! A ZD25WQ16B in RAM, at the level of the bytes on its bus: it acts on
//! a command when CS# rises, as the chip does, and ignores what the
//! datasheet says it ignores, so that a driver which skips a wait or a
//! Write Enable reads or writes nothing.

use std::vec;
use std::vec::Vec;

use super::*;

/// Microseconds a byte takes on the bus, eight clocks at about 4 MHz.
const BYTE_US: u64 = 2;

pub(crate) struct Fake {
    pub mem: Vec<u8>,
    /// Microseconds since the supply came up.
    pub now: u64,
    /// No chip on the bus: DO is never driven and reads low.
    pub absent: bool,
    asleep: bool,
    /// Commands before this are ignored: tRES1 after a Release.
    ready_at: u64,
    write_enabled: bool,
    busy_until: u64,
    selected: bool,
    command: Vec<u8>,
    /// Commands the chip ignored.
    pub ignored: u32,
    /// Programs and erases done.
    pub programs: u32,
    pub erases: u32,
}

impl Fake {
    /// A chip just powered, erased.
    pub fn new() -> Fake {
        Fake {
            mem: vec![0xff; CAPACITY as usize],
            now: 0,
            absent: false,
            asleep: false,
            ready_at: VSL_US as u64,
            write_enabled: false,
            busy_until: 0,
            selected: false,
            command: Vec::new(),
            ignored: 0,
            programs: 0,
            erases: 0,
        }
    }

    pub fn asleep(&self) -> bool {
        self.asleep
    }

    /// The write under way never ends.
    pub fn stall(&mut self) {
        self.busy_until = u64::MAX;
    }

    fn busy(&self) -> bool {
        self.now < self.busy_until
    }

    /// Whether the chip takes the command under way.
    fn listening(&self) -> bool {
        !self.absent && !self.asleep && self.now >= self.ready_at
    }

    fn address(&self) -> u32 {
        u32::from_be_bytes([0, self.command[1], self.command[2], self.command[3]])
    }

    /// What DO carries during the next byte.
    pub fn out(&self) -> u8 {
        if !self.selected || !self.listening() {
            return 0;
        }
        let n = self.command.len();
        match self.command.first() {
            Some(&READ_STATUS) => u8::from(self.busy()) | u8::from(self.write_enabled) << 1,
            Some(&READ_ID) if !self.busy() && (1..=3).contains(&n) => JEDEC_ID[n - 1],
            Some(&READ) if !self.busy() && n >= 4 => {
                self.mem[(self.address() as usize + n - 4) % CAPACITY as usize]
            }
            _ => 0,
        }
    }

    pub fn select(&mut self) {
        self.selected = true;
        self.command.clear();
    }

    pub fn take(&mut self, byte: u8) {
        self.command.push(byte);
        self.now += BYTE_US;
    }

    pub fn deselect(&mut self) {
        self.selected = false;
        let Some(&op) = self.command.first() else {
            return;
        };
        if self.absent {
            return;
        }
        if self.asleep {
            match op == RELEASE && self.command.len() == 1 {
                true => {
                    self.asleep = false;
                    self.ready_at = self.now + RES1_US as u64;
                }
                false => self.ignored += 1,
            }
            return;
        }
        if self.now < self.ready_at || (self.busy() && op != READ_STATUS) {
            self.ignored += 1;
            return;
        }
        match op {
            WRITE_ENABLE => self.write_enabled = true,
            PAGE_PROGRAM if self.write_enabled && self.command.len() > 4 => {
                let page = self.address() & !(PROGRAM_PAGE - 1);
                let mut at = self.address();
                for &b in &self.command[4..] {
                    self.mem[at as usize] &= b;
                    at = page + (at + 1) % PROGRAM_PAGE;
                }
                self.write_enabled = false;
                self.busy_until = self.now + PROGRAM_TYP_US as u64;
                self.programs += 1;
            }
            SECTOR_ERASE if self.write_enabled && self.command.len() == 4 => {
                let at = (self.address() & !(SECTOR - 1)) as usize;
                self.mem[at..at + SECTOR as usize].fill(0xff);
                self.write_enabled = false;
                self.busy_until = self.now + ERASE_TYP_US as u64;
                self.erases += 1;
            }
            PAGE_PROGRAM | SECTOR_ERASE => self.ignored += 1,
            DEEP_POWER_DOWN => self.asleep = true,
            _ => {}
        }
    }
}

impl Bus for Fake {
    fn select(&mut self) {
        Fake::select(self)
    }

    fn deselect(&mut self) {
        Fake::deselect(self)
    }

    fn transfer(&mut self, out: u8) -> u8 {
        let back = self.out();
        self.take(out);
        back
    }

    fn delay_us(&mut self, us: u32) {
        self.now += u64::from(us);
    }
}

/// A chip on a fake bus, initialised.
pub(crate) fn chip() -> Chip<Fake> {
    let mut c = Chip::new(Fake::new());
    c.init().unwrap();
    c
}

impl Chip<Fake> {
    pub(crate) fn fake(&mut self) -> &mut Fake {
        &mut self.bus
    }
}
