//! The reboot into an image gathered over the mesh and staged
//! (spec "Updates over the mesh", the install): after each step of its
//! driver the node's loop looks at the moment (`bark_image::policy`),
//! drains its core while a swap may come, and reboots once the moment is
//! quiet. This is the loop's own state for it, and the plan for a step.

use bark_image::policy::{drains, quiet, Links, Moment};

/// What the node is doing as a step ends, but for how long it has waited.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    /// What its phone links are doing.
    pub links: Links,
    /// Its core may reboot (`Relay::drained`).
    pub drained: bool,
    /// Its mailbox has writes queued.
    pub writing: bool,
    /// The phone's update holds the staged slot.
    pub phone_update: bool,
    /// The power allows a swap.
    pub swap_allowed: bool,
}

/// What a step is to do about the install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The version the status says the node drains to install.
    pub installing: Option<u32>,
    /// With an image staged, whether the core is to drain or to take
    /// messages again.
    pub drain: Option<bool>,
    /// Reboot into the staged image now.
    pub reboot: bool,
}

/// The plan for a step that ends at moment `m`, `staged` the version
/// whose swap is asked for.
pub fn plan(staged: Option<u32>, m: Moment) -> Plan {
    Plan {
        installing: staged.filter(|_| drains(m)),
        drain: staged.map(|_| drains(m)),
        reboot: staged.is_some() && quiet(m),
    }
}

/// The loop's state for the install, kept from step to step: since when
/// the core has been drained while the node drains, and the drain last
/// asked of the core and not yet seen there. `T` is the loop's instant.
#[derive(Clone, Copy, Debug)]
pub struct Install<T> {
    since: Option<T>,
    asked: Option<bool>,
}

impl<T: Copy> Default for Install<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> Install<T> {
    /// At boot: nothing waited for, nothing asked.
    pub const fn new() -> Self {
        Install {
            since: None,
            asked: None,
        }
    }

    /// The moment at `now`, with how long the node has waited since its
    /// core was drained, in ms as `elapsed_ms(now, since)` counts them.
    /// A moment that does not drain, or a core not drained, starts the
    /// wait again.
    pub fn moment(&mut self, seen: Seen, now: T, elapsed_ms: impl Fn(T, T) -> u64) -> Moment {
        let mut m = Moment {
            links: seen.links,
            drained: seen.drained,
            writing: seen.writing,
            phone_update: seen.phone_update,
            swap_allowed: seen.swap_allowed,
            waited_ms: 0,
        };
        self.since = (m.drained && drains(m)).then(|| self.since.unwrap_or(now));
        m.waited_ms = self.since.map_or(0, |t| elapsed_ms(now, t));
        m
    }

    /// The drain to ask the core for, if `want` is not what it does
    /// (`draining`) and was not asked already; once it does, nothing is
    /// left asked.
    pub fn ask(&mut self, want: bool, draining: bool) -> Option<bool> {
        if want == draining {
            self.asked = None;
            return None;
        }
        (self.asked != Some(want)).then_some(want)
    }

    /// The ask [`Install::ask`] gave reached the core's requests.
    pub fn asked(&mut self, want: bool) {
        self.asked = Some(want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bark_image::policy::{EXCHANGE_WAIT_MS, WRITE_WAIT_MS};

    fn seen() -> Seen {
        Seen {
            links: Links::None,
            drained: true,
            writing: false,
            phone_update: false,
            swap_allowed: true,
        }
    }

    fn ms(now: u64, since: u64) -> u64 {
        now - since
    }

    #[test]
    fn the_plan_follows_the_moment() {
        let m = |s: Seen, waited_ms| {
            let mut i = Install::<u64>::new();
            let mut m = i.moment(s, 0, ms);
            m.waited_ms = waited_ms;
            m
        };
        let quiet = m(seen(), 0);
        let mid_way = m(
            Seen {
                links: Links::MidWay,
                ..seen()
            },
            0,
        );
        let low = m(
            Seen {
                swap_allowed: false,
                ..seen()
            },
            0,
        );
        let phone = m(
            Seen {
                phone_update: true,
                ..seen()
            },
            0,
        );
        let held = m(
            Seen {
                drained: false,
                ..seen()
            },
            0,
        );
        // (staged, moment) -> (installing, drain, reboot)
        let table = [
            (None, quiet, None, None, false),
            (None, low, None, None, false),
            (Some(9), quiet, Some(9), Some(true), true),
            (Some(9), mid_way, Some(9), Some(true), false),
            (Some(9), held, Some(9), Some(true), false),
            (Some(9), low, None, Some(false), false),
            (Some(9), phone, None, Some(false), false),
        ];
        for (staged, moment, installing, drain, reboot) in table {
            assert_eq!(
                plan(staged, moment),
                Plan {
                    installing,
                    drain,
                    reboot
                },
                "{staged:?} {moment:?}"
            );
        }
    }

    #[test]
    fn the_wait_counts_from_the_first_drained_moment() {
        let mut i = Install::new();
        let busy = Seen {
            links: Links::MidWay,
            ..seen()
        };
        assert_eq!(i.moment(busy, 100, ms).waited_ms, 0);
        assert_eq!(i.moment(busy, 150, ms).waited_ms, 50);
        let m = i.moment(busy, 100 + EXCHANGE_WAIT_MS, ms);
        assert!(bark_image::policy::quiet(m));
    }

    #[test]
    fn a_core_not_drained_or_a_moment_that_does_not_drain_starts_the_wait_again() {
        for stop in [
            Seen {
                drained: false,
                ..seen()
            },
            Seen {
                swap_allowed: false,
                ..seen()
            },
            Seen {
                phone_update: true,
                ..seen()
            },
        ] {
            let mut i = Install::new();
            let writing = Seen {
                writing: true,
                ..seen()
            };
            i.moment(writing, 0, ms);
            assert_eq!(i.moment(stop, 10, ms).waited_ms, 0);
            assert_eq!(i.moment(writing, 20, ms).waited_ms, 0);
            assert_eq!(
                i.moment(writing, 20 + WRITE_WAIT_MS, ms).waited_ms,
                WRITE_WAIT_MS
            );
        }
    }

    #[test]
    fn a_drain_is_asked_once_until_the_core_does_it() {
        let mut i = Install::<u64>::new();
        // (want, draining, sent) -> ask
        let table = [
            // Not sent: the core's requests were full; asked again.
            (true, false, false, Some(true)),
            (true, false, true, Some(true)),
            (true, false, false, None),
            (true, true, false, None),
            (false, true, true, Some(false)),
            (false, true, false, None),
            (true, true, false, None),
            (false, false, false, None),
            (true, false, false, Some(true)),
        ];
        for (n, (want, draining, sent, ask)) in table.into_iter().enumerate() {
            assert_eq!(i.ask(want, draining), ask, "row {n}");
            if sent {
                i.asked(want);
            }
        }
    }
}
