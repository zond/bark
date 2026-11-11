// Which node the phone uses, and when it listens for one: one pure
// function from what the phone knows now to what the search does next
// (`bark-app/README.md`, "Screens"; roadmap D163, D171). The search carries the
// decision out and feeds what came of it back; nothing else decides.

import 'ble_link.dart';
import 'node_link.dart' show swapAway;
import 'nodes.dart';

/// How often the app reads a public node's ring for the person's mail
/// while it has nothing to send, short of the paired node: a minute, the
/// span the app takes for a person on foot to stay within a relay's reach
/// (`bark-app/README.md`, "Where a text goes"), which a sender's app takes as how
/// long a reply should go to where the person wrote from; a reply that
/// came there is thus read within about as long as it was aimed there.
/// Four of the search's rounds of 15 s, so it costs the phone no scan
/// more within Android's five in 30 s. The node pays for it in its one
/// guest link: an errand of a few seconds, say 5, a minute takes a
/// twelfth of the link, so a few phones in reach seldom meet there, and
/// one that does is closed and tries next round. Mail waits in the ring
/// meanwhile, so a longer interval delays it and loses nothing.
const Duration guestCheck = Duration(minutes: 1);

/// How often the phone, on a paired node that is not the home, scans for
/// the home, and moves to it once heard (roadmap D172): [guestCheck]'s
/// minute, the span the spec takes for a person on foot to stay within a
/// relay's reach, so a home that comes into reach is taken within about
/// as long as the person stays near it. Not every round of [lookRound]: on
/// a standing link a scan serves only this, and a scan of [lookScan] a
/// minute listens some 7 % of the time, against 27 % every 15 s, for a
/// home reached at most 45 s sooner. It takes at most one of Android's
/// five scans in 30 s.
const Duration lookAround = guestCheck;

/// The search's rounds in a row that must reach none of the paired nodes
/// before the app calls them out of reach and goes on errands at a public
/// node: two, 30 s at a round every [lookRound]. A round's scan of four
/// advertising intervals hears a node in reach with the screen on, but
/// with the screen off Android listens less and one round may miss it
/// ([lookScan]), so one miss is no proof. A round that hears one moves the
/// phone back at once, as a paired node serves the person whole. The two
/// differ on purpose: at the edge of a paired node's reach, heard in some
/// rounds and not others, the phone stays with it rather than going back
/// and forth, and its texts and the replies to them do not go by a public
/// node it then leaves (`bark-app/README.md`, "Screens";
/// roadmap D163, D166).
const int ownGone = 2;

/// The node heard strongest of `nodes`, the likeliest to hold a link.
FoundNode? _strongest(Iterable<FoundNode> nodes) {
  FoundNode? best;
  for (final n in nodes) {
    if ((n.rssi ?? noRssi) > (best?.rssi ?? noRssi) || best == null) {
      best = n;
    }
  }
  return best;
}

/// The node to use of those `heard`, in this order: the `home` when it is
/// one of the `paired` nodes, as it serves the person whole and keeps
/// their mail; else the paired node heard strongest; else the public node
/// heard strongest; else none. Never one of `passed`, of any kind
/// ([Situation.passed]).
///
/// The order holds among the nodes heard at a signal a link holds at
/// ([FoundNode.usable]): a node heard weak is passed over for one further
/// down heard better, as a link to it would not hold. With none usable,
/// the node heard strongest, of whatever kind, as signal alone then says
/// which link is likeliest to hold (roadmap D171).
///
/// While the phone has a link to the node `on`, the choice is only a node
/// higher in the order than it, heard usable: the home over another paired
/// node, a paired node over a public one. Never one beside it, as two of
/// one place in the order heard by turns at like signals would move the
/// phone back and forth; and never a weak one over the link it has
/// (roadmap D172).
FoundNode? chooseNode(
  Iterable<FoundNode> heard, {
  String? home,
  Set<String> paired = const {},
  Set<String> passed = const {},
  String? on,
}) {
  int place(String name) => name == home && paired.contains(name)
      ? 0
      : paired.contains(name)
      ? 1
      : 2;
  // A node the phone may use at all, whatever its signal.
  bool may(FoundNode n) =>
      !passed.contains(n.name) && (paired.contains(n.name) || n.public);
  final good = heard.where((n) => n.usable && may(n));
  final own = good.where((n) => paired.contains(n.name));
  final FoundNode? n;
  if (good.isEmpty) {
    n = on == null ? _strongest(heard.where(may)) : null;
  } else if (own.isEmpty) {
    n = _strongest(good);
  } else {
    n = own.where((n) => place(n.name) == 0).firstOrNull ?? _strongest(own);
  }
  if (n == null || on == null) return n;
  return place(n.name) < place(on) ? n : null;
}

/// The link the phone has: to the node `node`, a guest's or the owner's,
/// made by the person's tap or by the search.
typedef LinkNow = ({String node, bool guest, bool byTap});

/// A paired node that said it would install a version and ended its link
/// for the reboot at `gone` (spec "Updates over the mesh", the install;
/// roadmap D182).
typedef Awaited = ({String node, DateTime gone});

/// What the phone knows when the search decides.
class Situation {
  final DateTime now;

  /// The person's home node, by name, if they have one.
  final String? home;

  /// Every node the phone knows of, by name.
  final Map<String, NodeRecord> nodes;

  /// The link the phone has, if any.
  final LinkNow? link;

  /// A connection is being made or undone: the person's connect, a
  /// pairing or an unpairing. The search keeps out of its way.
  final bool linking;

  /// The search may scan: Bluetooth is allowed and on, and the person is
  /// not scanning in Settings.
  final bool radio;

  /// The person stopped the search, until they connect.
  final bool stopped;

  /// A paired node away to install a version, while the phone waits for
  /// it.
  final Awaited? awaited;

  /// The search's rounds in a row that have reached none of the paired
  /// nodes.
  final int missed;

  /// Something on the link would be cut were it closed now: a message
  /// written and not yet answered, one read and not yet confirmed, a
  /// fetch awaiting its answer, or a firmware update under way.
  final bool busy;

  /// Messages wait on the phone to be sent.
  final bool toSend;

  /// When the search last started a scan.
  final DateTime? scannedAt;

  /// When the phone last lost a link it held between rounds: an owner's,
  /// or a guest's the person made.
  final DateTime? droppedAt;

  /// What the scan that just ended heard, each node at its best signal;
  /// null when no scan has just ended.
  final List<FoundNode>? heard;

  Situation({
    required this.now,
    this.home,
    Iterable<NodeRecord> nodes = const [],
    this.link,
    this.linking = false,
    this.radio = true,
    this.stopped = false,
    this.awaited,
    this.missed = 0,
    this.busy = false,
    this.toSend = false,
    this.scannedAt,
    this.droppedAt,
    this.heard,
  }) : nodes = {for (final r in nodes) r.name: r};

  /// The paired nodes the search may use: they still hold the phone's
  /// bond, and they and the app serve each other as far as it knows
  /// ([NodeRecord.usable]). A paired node that does not is as one out of
  /// reach: the phone goes on to another, or to a public node, at once.
  late final Set<String> own = {
    for (final r in nodes.values)
      if (r.usable) r.name,
  };

  /// The nodes the phone does not use: those whose bond Android keeps
  /// though they are not the phone's, as a link encrypted with it the
  /// node ends (spec "Phone link", Public), and those that refused it over
  /// the link version ([NodeRecord.unserved]), as they would again every
  /// round.
  late final Set<String> passed = {
    for (final r in nodes.values)
      if (r.bondKept || r.unserved) r.name,
  };
}

/// What the search does: whether it runs, and how.
enum Mode {
  /// It does not run: the person stopped it, a connection is being made,
  /// Bluetooth cannot scan, or the link up is the person's or the home's.
  idle,

  /// Rounds every [lookRound], with no link up or during an errand.
  rounds,

  /// A scan for the home every [lookAround], from another paired node.
  lookAround,
}

/// The home, while the phone is on another node of its own by the
/// search's link: the one node above it in the order, which a look-around
/// listens for (roadmap D172). None on a link the person made by a tap,
/// which is theirs until it drops.
String? homeAbove(Situation s) {
  final l = s.link;
  final h = s.home;
  if (l == null || l.guest || l.byTap || h == null) return null;
  return s.own.contains(h) && l.node != h ? h : null;
}

Mode modeOf(Situation s) {
  if (s.linking || s.stopped || !s.radio) return Mode.idle;
  final l = s.link;
  if (l == null) return Mode.rounds;
  // A guest's errand is part of a round; a guest's link the person made
  // lasts until the node ends it (roadmap D179).
  if (l.guest) return l.byTap ? Mode.idle : Mode.rounds;
  return homeAbove(s) != null ? Mode.lookAround : Mode.idle;
}

/// Whether the phone still waits for the node [Situation.awaited]: through
/// [swapAway] and the first round after it, while the search looks for
/// that node alone. The second of the [ownGone] rounds that call it out of
/// reach is an ordinary round (roadmap D182, D202). Not once the node,
/// back, refused the phone over the link version: it is in reach, and of
/// no use.
bool waitsForInstall(Situation s) {
  final a = s.awaited;
  if (a == null || s.passed.contains(a.node)) return false;
  return s.now.difference(a.gone) < swapAway || s.missed < ownGone - 1;
}

/// Whether a round decided on in `s` that leaves the phone on none of its
/// own nodes counts as a miss of them: not while a node away to install
/// swaps its image in, as it is known to be away (roadmap D182).
bool countsMiss(Situation s) {
  final a = s.awaited;
  if (s.own.isEmpty) return false;
  return a == null || s.now.difference(a.gone) >= swapAway;
}

/// Whether the phone's own nodes were heard in [Situation.heard] and
/// passed over, too weak beside a node heard well.
bool ownPassedOver(Situation s) {
  final heard = s.heard ?? const [];
  if (!heard.any((h) => s.own.contains(h.name))) return false;
  final n = chooseNode(heard, home: s.home, paired: s.own, passed: s.passed);
  return n == null || !s.own.contains(n.name);
}

/// What the search does next.
sealed class Decision {
  const Decision();
}

/// Nothing until the situation changes.
final class Idle extends Decision {
  const Idle();
  @override
  String toString() => 'Idle';
}

/// Nothing for `wait`, or until the situation changes.
final class Wait extends Decision {
  final Duration wait;
  const Wait(this.wait);
  @override
  String toString() => 'Wait(${wait.inSeconds} s)';
}

/// A scan of one round, ended early once `until` is heard well: no node
/// heard after it would go before it.
final class Listen extends Decision {
  final String? until;
  const Listen(this.until);
  @override
  String toString() => 'Listen(${until ?? 'all'})';
}

/// A link to `node` as its owner.
final class Connect extends Decision {
  final FoundNode node;
  const Connect(this.node);
  @override
  String toString() => 'Connect(${node.name})';
}

/// A guest's errand at `node`: connect, read its ring for the person,
/// send what waits, and leave (spec "Phone link", Public).
final class Errand extends Decision {
  final FoundNode node;
  const Errand(this.node);
  @override
  String toString() => 'Errand(${node.name})';
}

/// From the paired node the phone is on to the home, `node`.
final class MoveHome extends Decision {
  final FoundNode node;
  const MoveHome(this.node);
  @override
  String toString() => 'MoveHome(${node.name})';
}

/// The wait for the installing node `node` is over ([waitsForInstall]):
/// it is out of reach, or back and passed over, as any node, and the
/// round goes on as an ordinary one.
final class GiveUp extends Decision {
  final String node;
  const GiveUp(this.node);
  @override
  String toString() => 'GiveUp($node)';
}

/// What the search does next in the situation `s`.
Decision decide(Situation s) => switch (modeOf(s)) {
  Mode.idle => const Idle(),
  Mode.lookAround => _lookAround(s),
  Mode.rounds => _round(s),
};

/// The wait until a scan of every `period` is due again, or the scan; at
/// once after a link was lost since the last, as the phone is then on no
/// node, and texts waiting go out as soon as one is in reach.
Decision _next(Situation s, Duration period, String? until) {
  final at = s.scannedAt;
  final dropped = s.droppedAt;
  if (at == null || (dropped != null && dropped.isAfter(at))) {
    return Listen(until);
  }
  final left = period - s.now.difference(at);
  return left > Duration.zero ? Wait(left) : Listen(until);
}

/// On another paired node: the home, once heard well, when nothing on the
/// link would be cut; else the next minute (roadmap D172).
Decision _lookAround(Situation s) {
  final home = homeAbove(s)!;
  final heard = s.heard;
  if (heard == null) return _next(s, lookAround, home);
  final n = chooseNode(
    heard,
    home: s.home,
    paired: s.own,
    passed: s.passed,
    on: s.link!.node,
  );
  if (n == null || s.busy) return _next(s, lookAround, home);
  return MoveHome(n);
}

/// A round: with no link, the node [chooseNode] takes of those heard; the
/// public node only while the paired nodes are out of reach or heard too
/// weakly, and when its errand is due; while a paired node installs, that
/// node alone.
Decision _round(Situation s) {
  if (s.link != null) return const Idle();
  final awaited = s.awaited;
  if (awaited != null && !waitsForInstall(s)) return GiveUp(awaited.node);
  final heard = s.heard;
  if (heard == null) return _next(s, lookRound, awaited?.node ?? _stopAt(s));
  if (awaited != null) {
    final n = heard.where((h) => h.name == awaited.node).firstOrNull;
    return n != null ? Connect(n) : _next(s, lookRound, null);
  }
  final n = chooseNode(heard, home: s.home, paired: s.own, passed: s.passed);
  if (n == null) return _next(s, lookRound, null);
  if (s.own.contains(n.name)) return Connect(n);
  // This round counts as a miss of the paired nodes, as it takes none.
  final away = s.own.isEmpty || s.missed + 1 >= ownGone;
  // A paired node heard weak was not missed: the public node goes before
  // it at once, as no later round is needed to tell it is there.
  final weak = heard.any((h) => s.own.contains(h.name));
  if ((away || weak) && _errandDue(s, n.name)) return Errand(n);
  return _next(s, lookRound, null);
}

/// What ends a round's scan early, as no node heard after it would go
/// before it: the home, when it is the phone's, else its one paired node;
/// nothing for several, of which the strongest goes first.
String? _stopAt(Situation s) {
  final h = s.home;
  if (h != null && s.own.contains(h)) return h;
  return s.own.length == 1 ? s.own.single : null;
}

/// Whether a public node is due a guest's errand: something waits to be
/// sent, or its ring has not been read for [guestCheck].
bool _errandDue(Situation s, String node) {
  if (s.toSend) return true;
  final at = s.nodes[node]?.ringReadAt;
  return at == null || s.now.difference(at) >= guestCheck;
}
