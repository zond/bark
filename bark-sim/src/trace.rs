//! The per-node event trace (`bark-sim --trace FILE`): one line per thing a
//! core heard, was told or did, for following a message or a leaf by eye.
//!
//! A line is `<seconds> <node>: <what>`. Packets are decoded; nodes are
//! named by node label rather than ID, and a message by its traffic
//! tag (`#n`, the order it was originated in) where the simulator knows it.
//! Channel checks and sweep readings are left out: a relay takes thirteen a
//! second and they would drown everything else.

use bark_proto::wire::Packet;
use bark_proto::{Action, AppCommand, AppEvent, Event, Instant, MsgId, NodeId, Trace};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;

/// Names for what a trace line mentions.
pub(crate) struct Names {
    /// Node label by index.
    pub labels: Vec<String>,
    /// Node by ID.
    pub ids: HashMap<[u8; 4], u32>,
}

impl Names {
    /// The node `id` names.
    fn node(&self, id: NodeId) -> String {
        match self.ids.get(&id.0) {
            Some(&n) => self.labels[n as usize].clone(),
            None => format!("id {:02x?}", id.0),
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
            Ok(Packet::Heartbeat(h)) => format!(
                "HEARTBEAT{} home {}",
                if h.sleepy { " (sleepy)" } else { "" },
                h.home.index()
            ),
            Ok(Packet::Ack(a)) => {
                format!(
                    "ACK {} from {} to {}{}",
                    self.msg(a.msg_id, tags),
                    n.node(a.from),
                    n.node(a.to),
                    if a.refused { ", refused" } else { "" }
                )
            }
            Ok(Packet::Data(d)) => {
                let src = n.node(d.src_hop);
                format!(
                    "DATA {} to {} from {src} (home {}), hops {}, for {}",
                    payload_tag(d.payload),
                    n.node(d.dst_hop),
                    d.sender_home.index(),
                    d.hops,
                    n.node(d.dst_id),
                )
            }
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
                dst_id, payload, ..
            }) => format!(
                "app send {} to {}",
                payload_tag(payload),
                self.names.node(*dst_id)
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
            Action::Sleep(until) => {
                let _ = write!(what, "sleep to {:.3} s", until.as_micros() as f64 / 1e6);
            }
            Action::Sniff(until) => {
                let _ = write!(what, "sniff to {:.3} s", until.as_micros() as f64 / 1e6);
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
                        n.node(by)
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
                        n.node(to)
                    ),
                    Trace::ReturnedToParent { msg_id, to } => write!(
                        what,
                        "returned {} to {}",
                        self.msg(msg_id, tags),
                        n.node(to)
                    ),
                    Trace::Demoted { neighbour } => write!(what, "demoted {}", n.node(neighbour)),
                    Trace::Delivered { msg_id, node } => write!(
                        what,
                        "delivered {} to {}",
                        self.msg(msg_id, tags),
                        n.node(node)
                    ),
                    Trace::Queued { msg_id, node } => {
                        write!(
                            what,
                            "queued {} for {}",
                            self.msg(msg_id, tags),
                            n.node(node)
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
