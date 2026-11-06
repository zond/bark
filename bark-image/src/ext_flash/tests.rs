//! The driver on a fake chip, and the bit-banged bus on fake pins.

use std::cell::RefCell;
use std::rc::Rc;
use std::vec::Vec;

use super::fake::{chip, Fake};
use super::*;

#[test]
fn a_chip_is_found_and_left_asleep() {
    let mut c = Chip::new(Fake::new());
    assert_eq!(c.init(), Ok(0));
    assert!(c.fake().asleep());
    assert_eq!(c.fake().ignored, 0);
}

#[test]
fn no_chip_fails_the_presence_check() {
    let mut f = Fake::new();
    f.absent = true;
    assert_eq!(Chip::new(f).init(), Err(Error::Absent([0; 3])));
}

#[test]
fn a_chip_left_asleep_by_a_reset_is_woken() {
    let mut c = chip();
    // The processor reset, the chip's supply up throughout.
    let fake = core::mem::replace(c.fake(), Fake::new());
    let mut again = Chip::new(fake);
    assert_eq!(again.init(), Ok(0));
}

#[test]
fn what_is_written_reads_back_across_pages() {
    let mut c = chip();
    let data: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
    c.erase(SECTOR, 2 * SECTOR).unwrap();
    c.write(SECTOR + 100, &data).unwrap();
    let mut back = std::vec![0u8; 1000];
    c.read(SECTOR + 100, &mut back).unwrap();
    assert_eq!(back, data);
    // 1000 bytes from 100 into a page touch five pages.
    assert_eq!(c.fake().programs, 5);
    assert_eq!(c.fake().ignored, 0);
    assert!(c.fake().asleep());
}

#[test]
fn an_erase_clears_its_sector_only() {
    let mut c = chip();
    c.write(SECTOR - 4, &[0; 8]).unwrap();
    c.erase(SECTOR, 2 * SECTOR).unwrap();
    let mut back = [0xaau8; 8];
    c.read(SECTOR - 4, &mut back).unwrap();
    assert_eq!(back, [0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]);
    assert_eq!(c.erase(1, SECTOR), Err(Error::NotAligned));
    assert_eq!(c.erase(0, CAPACITY + SECTOR), Err(Error::OutOfBounds));
}

#[test]
fn a_read_during_an_erase_waits_for_it() {
    let mut c = chip();
    c.write(0, &[1, 2, 3, 4]).unwrap();
    c.start_erase(SECTOR).unwrap();
    assert!(!c.done());
    let started = c.fake().now;
    let mut back = [0u8; 4];
    c.read(0, &mut back).unwrap();
    assert_eq!(back, [1, 2, 3, 4]);
    assert!(c.fake().now - started >= u64::from(ERASE_TYP_US) - 100);
    assert!(c.done());
    assert!(c.fake().asleep());
}

#[test]
fn done_reports_the_end_and_puts_the_chip_to_sleep() {
    let mut c = chip();
    c.start_program(0, &[0x5a; 16]).unwrap();
    assert!(!c.done());
    assert!(!c.fake().asleep());
    c.fake().now += u64::from(PROGRAM_TYP_US);
    assert!(c.done());
    assert!(c.fake().asleep());
    assert_eq!(c.finish(), Ok(()));
}

#[test]
fn a_program_across_a_page_is_refused() {
    let mut c = chip();
    assert_eq!(c.start_program(250, &[0; 8]), Err(Error::OutOfBounds));
    assert_eq!(
        c.start_program(CAPACITY - 4, &[0; 8]),
        Err(Error::OutOfBounds)
    );
}

#[test]
fn a_chip_that_never_ends_a_write_times_out() {
    let mut c = chip();
    c.start_erase(0).unwrap();
    // Stuck: the erase never ends.
    c.fake().stall();
    let started = c.fake().now;
    assert_eq!(c.finish(), Err(Error::Timeout));
    assert!(c.fake().now - started >= u64::from(ERASE_MAX_US));
    // The next use does not wait again for the same write.
    assert!(c.done());
}

/// The bus's four lines and its clock, shared by the fake pins.
struct Wire {
    chip: Fake,
    cs: bool,
    clk: bool,
    di: bool,
    /// The byte the chip shifts out, and its bits shifted so far.
    out: u8,
    bits: u8,
    taken: u8,
}

#[derive(Clone)]
struct Pin(Rc<RefCell<Wire>>, char);

impl embedded_hal::digital::ErrorType for Pin {
    type Error = Infallible;
}

impl OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.set(false);
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Infallible> {
        self.set(true);
        Ok(())
    }
}

impl InputPin for Pin {
    fn is_high(&mut self) -> Result<bool, Infallible> {
        let w = self.0.borrow();
        Ok(!w.cs && w.out & 0x80 != 0)
    }
    fn is_low(&mut self) -> Result<bool, Infallible> {
        self.is_high().map(|h| !h)
    }
}

impl Pin {
    fn set(&self, level: bool) {
        let mut w = self.0.borrow_mut();
        match self.1 {
            'c' if w.cs && !level => {
                w.cs = false;
                w.chip.select();
                w.out = w.chip.out();
                w.bits = 0;
            }
            'c' if !w.cs && level => {
                assert_eq!(w.bits, 0, "CS# rose inside a byte");
                w.cs = true;
                w.chip.deselect();
            }
            'k' if !w.clk && level => {
                w.clk = true;
                if !w.cs {
                    w.taken = w.taken << 1 | u8::from(w.di);
                    w.bits += 1;
                }
            }
            'k' if w.clk && !level => {
                w.clk = false;
                if !w.cs {
                    if w.bits == 8 {
                        let b = w.taken;
                        w.chip.take(b);
                        w.out = w.chip.out();
                        w.bits = 0;
                    } else {
                        w.out <<= 1;
                    }
                }
            }
            'd' => w.di = level,
            _ => {}
        }
    }
}

struct Clock(Rc<RefCell<Wire>>);

impl DelayNs for Clock {
    fn delay_ns(&mut self, ns: u32) {
        self.0.borrow_mut().chip.now += u64::from(ns.div_ceil(1000));
    }
}

#[test]
fn the_bit_banged_bus_speaks_mode_0() {
    let wire = Rc::new(RefCell::new(Wire {
        chip: Fake::new(),
        cs: true,
        clk: false,
        di: false,
        out: 0,
        bits: 0,
        taken: 0,
    }));
    let pin = |which| Pin(wire.clone(), which);
    let mut c = Chip::new(BitBang {
        cs: pin('c'),
        clk: pin('k'),
        di: pin('d'),
        dout: pin('o'),
        delay: Clock(wire.clone()),
    });
    assert_eq!(c.init(), Ok(0));
    let data: Vec<u8> = (0..=255).collect();
    c.erase(0, SECTOR).unwrap();
    c.write(0x80, &data).unwrap();
    let mut back = std::vec![0u8; 256];
    c.read(0x80, &mut back).unwrap();
    assert_eq!(back, data);
    let w = wire.borrow();
    assert_eq!(w.chip.ignored, 0);
    assert!(w.chip.asleep());
    assert_eq!(&w.chip.mem[0x80..0x180], data.as_slice());
}
