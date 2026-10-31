//! The radio discipline the jobs share: one excursion at a time
//! (`docs/core-interface.md` §3 rule 1), and frame building.

use crate::event::{Action, Frame, MAX_FRAME};
use crate::radio::{time_on_air, Channel};
use crate::time::Duration;
use crate::wire::Packet;

/// Encode `packet` as a frame. Only for packets whose payload the caller
/// has bounded, which is every packet the MAC sends.
pub(crate) fn frame(packet: &Packet<'_>, channel: Channel, preamble_symbols: u16) -> Frame {
    let mut f = Frame {
        channel,
        preamble_symbols,
        len: 0,
        bytes: [0; MAX_FRAME],
    };
    f.len = packet.encode(&mut f.bytes).unwrap_or(0) as u8;
    f
}

/// Airtime of `f`.
pub(crate) fn airtime(f: &Frame) -> Duration {
    time_on_air(f.len, f.preamble_symbols)
}

/// What the outstanding excursion is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// A listen-before-talk check; found clear, the owner transmits.
    Check,
    /// A transmission.
    Tx,
    /// A signal strength reading.
    Read,
}

/// The one excursion a core may have outstanding, and the job that owns
/// it: every job's use of the radio is a check on a channel and, if it
/// is clear, a transmission, or one reading. The completion event goes
/// back to the owner, which continues from there.
pub(crate) struct Excursion<O: Copy + PartialEq> {
    out: Option<(O, Step)>,
}

impl<O: Copy + PartialEq> Excursion<O> {
    pub(crate) const fn new() -> Self {
        Excursion { out: None }
    }

    pub(crate) fn idle(&self) -> bool {
        self.out.is_none()
    }

    /// Where `owner`'s excursion is, if it has one outstanding.
    pub(crate) fn step_of(&self, owner: O) -> Option<Step> {
        self.out.filter(|(o, _)| *o == owner).map(|(_, s)| s)
    }

    /// Start a listen-before-talk check on `channel` for `owner`.
    pub(crate) fn check(&mut self, owner: O, channel: Channel, emit: &mut dyn FnMut(Action)) {
        self.start(owner, Step::Check, Action::Lbt(channel), emit);
    }

    /// Start a reading of `channel` for `owner`.
    pub(crate) fn read(&mut self, owner: O, channel: Channel, emit: &mut dyn FnMut(Action)) {
        self.start(owner, Step::Read, Action::Rssi(channel), emit);
    }

    /// Transmit `frame` for `owner`.
    pub(crate) fn transmit(&mut self, owner: O, frame: Frame, emit: &mut dyn FnMut(Action)) {
        self.start(owner, Step::Tx, Action::Transmit(frame), emit);
    }

    /// A second excursion while the first is outstanding is a core bug.
    fn start(&mut self, owner: O, step: Step, action: Action, emit: &mut dyn FnMut(Action)) {
        debug_assert!(self.out.is_none(), "second excursion outstanding");
        self.out = Some((owner, step));
        emit(action);
    }

    /// The owner of the excursion that just completed, if one was outstanding.
    pub(crate) fn complete(&mut self) -> Option<O> {
        self.out.take().map(|(o, _)| o)
    }
}
