//! The per-node event trace (`bark-sim --trace FILE`): one line per thing a
//! core heard, was told or did, for following a message or a leaf by eye.
//!
//! A line is `<seconds> <node>: <what>`. Packets are decoded; relays are
//! named by node label rather than short name, and a message by its traffic
//! tag (`#n`, the order it was originated in) where the simulator knows it.
//! Channel checks and sweep readings are left out: a relay takes thirteen a
//! second and they would drown everything else.

use bark_proto::wire::{AckName, Packet};
use bark_proto::{Action, AppCommand, AppEvent, Event, Instant, LeafId, MsgId, ShortName, Trace};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;

/// Names for what a trace line mentions.
pub(crate) struct Names {
    /// Node label by index.
    pub labels: Vec<String>,
    /// Relay node by short name.
    pub relays: HashMap<u32, u32>,
    /// Leaf node by leaf ID.
    pub leaves: HashMap<[u8; 4], u32>,
}

impl Names {
    fn relay(&self, s: ShortName) -> String {
        match self.relays.get(&s.0) {
            Some(&n) => self.labels[n as usize].clone(),
            None => format!("{:08x}", s.0),
        }
    }

    fn leaf(&self, l: LeafId) -> String {
        match self.leaves.get(&l.0) {
            Some(&n) => self.labels[n as usize].clone(),
            None if l == LeafId::RELAY => "relay".into(),
            None => format!("leaf id {:02x?}", l.0),
        }
    }

    /// An ACK names a relay or a leaf; try both. Zero is a leaf's DATA,
    /// which names no sender.
    fn party(&self, f: AckName) -> String {
        let s = ShortName(u32::from_be_bytes(f.0));
        if s.0 == 0 {
            "a leaf".into()
        } else if self.relays.contains_key(&s.0) {
            self.relay(s)
        } else {
            self.leaf(LeafId(f.0))
        }
    }
}

/// The trace sink.
pub(crate) struct Tracer {
    out: Box<dyn Write>,
    pub names: Names,
}

/// The traffic tag the simulator puts in the first four payload bytes.
fn payload_tag(p: &[u8]) -> String {
    match p.get(..4) {
        Some(b) => format!("#{}", u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        None => "#?".into(),
    }
}

impl Tracer {
    pub(crate) fn new(out: Box<dyn Write>, names: Names) -> Tracer {
        Tracer { out, names }
    }

    fn line(&mut self, t: Instant, node: u32, what: &str) {
        let s = t.as_micros();
        // A trace is a debugging aid; a full disk must not stop the run.
        let _ = writeln!(
            self.out,
            "{}.{:06} {}: {what}",
            s / 1_000_000,
            s % 1_000_000,
            self.names.labels[node as usize]
        );
    }

    fn msg(&self, id: MsgId, tags: &HashMap<u32, u32>) -> String {
        match tags.get(&id.0) {
            Some(t) => format!("#{t}"),
            None => format!("msg {:08x}", id.0),
        }
    }

    fn packet(&self, bytes: &[u8], tags: &HashMap<u32, u32>) -> String {
        let n = &self.names;
        match Packet::decode(bytes) {
            Ok(Packet::Heartbeat(h)) => format!("HEARTBEAT home {}", h.home.index()),
            Ok(Packet::Ack(a)) => {
                format!(
                    "ACK {} from {} to {}{}",
                    self.msg(a.msg_id, tags),
                    n.party(a.from),
                    n.party(a.to),
                    if a.refused { ", refused" } else { "" }
                )
            }
            Ok(Packet::Attach(a)) => format!(
                "ATTACH {} to {}{}",
                n.leaf(a.leaf),
                n.relay(a.relay),
                if a.reply { ", answering a poll" } else { "" }
            ),
            Ok(Packet::Attached(a)) => format!(
                "ATTACHED {}, next poll in {} ms",
                n.leaf(a.leaf),
                a.next_poll().as_millis()
            ),
            Ok(Packet::Data(d)) => {
                let src = if d.hops == 0 {
                    "a leaf".into()
                } else {
                    n.relay(d.src_hop)
                };
                format!(
                    "DATA {} to {} from {src} (home {}), hops {}, for {}",
                    payload_tag(d.payload),
                    n.relay(d.dst_hop),
                    d.sender_home.index(),
                    d.hops,
                    n.leaf(d.dst_leaf),
                )
            }
            Ok(Packet::Deliver(d)) => format!(
                "DELIVER {} to {}",
                payload_tag(d.payload),
                n.leaf(d.dst_leaf)
            ),
            Err(e) => format!("undecodable {e:?}"),
        }
    }

    /// An event the driver hands a core. Only receptions and application
    /// commands are traced.
    pub(crate) fn event(
        &mut self,
        t: Instant,
        node: u32,
        ev: &Event<'_>,
        tags: &HashMap<u32, u32>,
    ) {
        let what = match ev {
            Event::Received {
                channel,
                rssi_dbm,
                bytes,
                ..
            } => format!(
                "rx ch {} {rssi_dbm} dBm {}",
                channel.index(),
                self.packet(bytes, tags)
            ),
            Event::App(AppCommand::Send {
                dst_leaf, payload, ..
            }) => format!(
                "app send {} to {}",
                payload_tag(payload),
                self.names.leaf(*dst_leaf)
            ),
            _ => return,
        };
        self.line(t, node, &what);
    }

    /// An action a core emitted. Channel checks are not traced.
    pub(crate) fn action(&mut self, t: Instant, node: u32, a: &Action, tags: &HashMap<u32, u32>) {
        let mut what = String::new();
        match a {
            Action::Listen(c) => {
                let _ = write!(what, "listen ch {}", c.index());
            }
            Action::Transmit(f) => {
                let _ = write!(
                    what,
                    "tx ch {} {}",
                    f.channel.index(),
                    self.packet(f.payload(), tags)
                );
            }
            Action::App(AppEvent::Message { len, payload, .. }) => {
                let _ = write!(
                    what,
                    "app message {}",
                    payload_tag(&payload[..*len as usize])
                );
            }
            Action::Trace(tr) => {
                let n = &self.names;
                let _ = match *tr {
                    Trace::Dropped { msg_id, reason } => {
                        write!(what, "dropped {} {reason:?}", self.msg(msg_id, tags))
                    }
                    Trace::Refused { msg_id, reason } => {
                        write!(what, "refused {} {reason:?}", self.msg(msg_id, tags))
                    }
                    Trace::TurnedAway { msg_id, by } => write!(
                        what,
                        "{} turned away by {}",
                        self.msg(msg_id, tags),
                        n.relay(by)
                    ),
                    Trace::HandOverDeferred { msg_id, until } => write!(
                        what,
                        "hand-over of {} deferred to {:.3} s",
                        self.msg(msg_id, tags),
                        until.as_micros() as f64 / 1e6
                    ),
                    Trace::Forwarded { msg_id, to, hops } => write!(
                        what,
                        "forwarded {} to {} at hops {hops}",
                        self.msg(msg_id, tags),
                        n.relay(to)
                    ),
                    Trace::ReturnedToParent { msg_id, to } => write!(
                        what,
                        "returned {} to {}",
                        self.msg(msg_id, tags),
                        n.relay(to)
                    ),
                    Trace::Demoted { neighbour } => write!(what, "demoted {}", n.relay(neighbour)),
                    Trace::Delivered { msg_id, leaf } => write!(
                        what,
                        "delivered {} to {}",
                        self.msg(msg_id, tags),
                        n.leaf(leaf)
                    ),
                    Trace::Queued { msg_id, leaf } => {
                        write!(
                            what,
                            "queued {} for {}",
                            self.msg(msg_id, tags),
                            n.leaf(leaf)
                        )
                    }
                    other => write!(what, "{other:?}"),
                };
            }
            Action::Cad(_) | Action::Rssi(_) => return,
        }
        self.line(t, node, &what);
    }

    /// Something the scenario did to a node.
    pub(crate) fn note(&mut self, t: Instant, node: u32, what: &str) {
        self.line(t, node, what);
    }

    pub(crate) fn flush(&mut self) {
        let _ = self.out.flush();
    }
}
