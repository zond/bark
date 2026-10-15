//! The radio discipline the roles share: one excursion at a time
//! (`docs/core-interface.md` §3 rule 1), and frame building.

use crate::event::{Action, Frame, MAX_FRAME};
use crate::radio::{time_on_air, Channel, MAX_ERP_DBM};
use crate::time::Duration;
use crate::wire::Packet;

/// Encode `packet` as a frame. Only for packets whose payload the caller
/// has bounded, which is every packet the MAC sends. The power is the
/// ERP limit until the role sets it, just before transmitting.
pub(crate) fn frame(packet: &Packet<'_>, channel: Channel, preamble_symbols: u16) -> Frame {
    let mut f = Frame {
        channel,
        preamble_symbols,
        power_dbm: MAX_ERP_DBM,
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

/// The one excursion a core may have outstanding, and which of its jobs
/// started it, so the completion event goes back to that job.
pub(crate) struct Radio<O: Copy> {
    out: Option<O>,
}

impl<O: Copy> Radio<O> {
    pub(crate) const fn new() -> Self {
        Radio { out: None }
    }

    pub(crate) fn idle(&self) -> bool {
        self.out.is_none()
    }

    /// Start an excursion for `owner`. A second one while the first is
    /// outstanding is a core bug.
    pub(crate) fn start(&mut self, owner: O, action: Action, emit: &mut dyn FnMut(Action)) {
        debug_assert!(self.out.is_none(), "second excursion outstanding");
        debug_assert!(matches!(
            action,
            Action::Cad(_) | Action::Rssi(_) | Action::Transmit(_)
        ));
        self.out = Some(owner);
        emit(action);
    }

    /// The owner of the excursion that just completed, if one was outstanding.
    pub(crate) fn complete(&mut self) -> Option<O> {
        self.out.take()
    }
}
