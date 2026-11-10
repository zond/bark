// Making and ending links: the person's taps, the search's connects and
// a guest's errand, each attached to the session.

part of '../app.dart';

/// The steps of a connection before its link is attached.
enum Linking {
  none,

  /// Connecting, and asking for the MTU.
  connecting,

  /// Android asks the person for the code on the node's screen.
  pairing,

  /// The bond is made; the app goes on to connect.
  paired,

  /// Connected; the app reads the node.
  attaching,
}

mixin _Connect on _AppCore {
  /// Where a connection stands until the link is attached.
  Linking linking = Linking.none;

  /// A connection is being made: no other may start, and the search
  /// does not scan. A tap's covers the whole of its connect, from the
  /// close of the old link to the new link up, as the search would take
  /// another node in the moments between ([_byTap]).
  @override
  bool get connecting => linking != Linking.none || _tapHeld;

  /// The device of the link connected or being connected, and the MTU
  /// it agreed.
  @override
  String? linkDevice;
  int? mtu;

  /// The person taps a node they are paired with in Settings: the app
  /// connects to it as its owner, as a tap on it in a scan does. A node
  /// heard within the span the search takes to call a node out of reach
  /// ([ownGone] rounds) is taken as heard; another is listened for first,
  /// one round's scan, and [NotSeen] goes on if it is not heard.
  Future<void> connectPaired(String name) async {
    if (_tapHeld) return;
    await _byTap(() async {
      var n = heardLately(name);
      if (n == null) {
        await stopScan();
        final heard = await listen(until: (f) => f.name == name);
        _heard(heard.values);
        n = heard.values.where((f) => f.name == name).firstOrNull;
      }
      if (n == null) throw NotSeen(shownName(name));
      if (connectedTo(n.deviceId) && !session.guest) return;
      await _connect(n);
    });
  }

  /// Whether `device` is the one connected.
  bool connectedTo(String device) => session.connected && linkDevice == device;

  /// The person connects to `n`, after closing the link to another
  /// node, or a guest's to `n`, and pairs with it if the phone has not,
  /// removing any bond Android holds with it first; `again` after they
  /// chose to pair again, which finds the node in a scan first ([seen]),
  /// as Android may have dropped the bond the node refused, and with it
  /// what it knew of the address. It ends the search for the paired node.
  /// Nothing while another connect of the person's is under way, or once
  /// `n` is connected as the phone's: a second connect to the same node
  /// would close the link the first made. A connect that ends with no
  /// link says why: by what it throws, or, where app and node do not serve
  /// each other, by the session's notice.
  @override
  Future<void> connect(FoundNode n, {bool again = false}) async {
    if (_tapHeld || (connectedTo(n.deviceId) && !session.guest)) return;
    await _byTap(() async {
      if (connectedTo(n.deviceId) && !session.guest) return;
      await _connect(n, again: again);
    });
  }

  /// The person connects to `n`, a public node, as a guest: without
  /// pairing, so that it stays its owner's. It ends a search round, as
  /// [connect] does, and the link lasts until the node ends it, after a
  /// guest's 30 s, or the person leaves, not one errand (roadmap D179).
  Future<void> connectGuest(FoundNode n) async {
    if (_tapHeld || connectedTo(n.deviceId)) return;
    await _byTap(() => _connectGuest(n, errand: false));
  }

  /// Runs the connect `f` the person asked for, holding [connecting]
  /// until it ends, so that the search neither scans nor connects
  /// meanwhile: a round started when the tap closed the old link took
  /// the home before the tap's own connect was made (bench, 2026-11-10).
  /// A connect of the search's already under way ends first: the tap
  /// waits for it rather than doing nothing.
  Future<void> _byTap(Future<void> Function() f) async {
    _tapHeld = true;
    notifyListeners();
    try {
      await _searchStopped();
      await _linkingEnded();
      await f();
    } finally {
      _tapHeld = false;
      notifyListeners();
    }
  }

  /// Completes once no connection is being set up ([linking]).
  Future<void> _linkingEnded() async {
    if (linking == Linking.none) return;
    final done = Completer<void>();
    void check() {
      if (linking == Linking.none && !done.isCompleted) done.complete();
    }

    addListener(check);
    await done.future;
    removeListener(check);
  }

  /// A connect the person asked for is under way ([_byTap]).
  bool _tapHeld = false;

  /// A guest's errand at `n`: connect without pairing, write the tag and
  /// read the ring's items for the person ([Session.attach]), send what
  /// waits while Ready comes, and leave, as the node shares its one guest
  /// link among every guest in reach (spec "Phone link", Public); not an
  /// `errand` but the person's tap, the link stays until it ends. A bond
  /// Android holds with the node goes first ([_forgetBond]): a guest's
  /// link is never made over one; where Android keeps it, [BondKept].
  Future<void> _connectGuest(FoundNode n, {bool errand = true}) async {
    if (!await _forgetBond(n.name, n.deviceId)) throw BondKept(n.name);
    // A connect of the person's that came meanwhile goes first.
    if (errand && _tapHeld) return;
    final r = nodes.of(n.name);
    if (r.unseen && !await seen(n.deviceId)) throw NotSeen(shownName(n.name));
    r.unseen = false;
    await _open(n.deviceId, n.name, () async {
      await _searchStopped();
      await stopScan();
      return BleLink.connect(n.deviceId, name: n.name, guest: true);
    }, guest: true);
    if (!session.connected || !session.guest) return;
    // Linked, it is in reach now.
    nodes.heard(n, now());
    // The search's errand node is the one the phone uses as a guest
    // between errands; a link the person made by a tap is theirs alone.
    nodes.guest = errand ? n.name : null;
    r.asGuest = (
      address: session.address,
      homeable: canBeHome(session.status),
      room: session.node?.room,
    );
    if (session.guestRead) r.ringReadAt = now();
    if (!errand) return;
    final link = session.link;
    await _sendQueuedFetch();
    await _sent();
    // Only the errand's own link: a tap may have replaced it meanwhile.
    if (session.connected && session.link == link) await session.disconnect();
  }

  /// Completes once nothing waits to be offered to the node, or the link
  /// has ended: the node ends a guest's after 30 s, or at once when
  /// another guest has it, and the next round tries again.
  Future<void> _sent() async {
    final done = Completer<void>();
    void check() {
      if (!done.isCompleted && (!session.connected || !session.toSend)) {
        done.complete();
      }
    }

    session.addListener(check);
    check();
    await done.future;
    session.removeListener(check);
  }

  /// The link the phone has, if any.
  @override
  LinkNow? get _linkNow {
    final n = session.connected ? session.node : null;
    if (n == null) return null;
    return (node: nodeName(n.id), guest: session.guest, byTap: _linkByTap);
  }

  /// The link up was made by the person's tap, not by the search.
  bool _linkByTap = false;

  Future<void> _connect(FoundNode n, {bool again = false}) async {
    await _open(n.deviceId, n.name, () async {
      await _searchStopped();
      await stopScan();
      final ours = nodes[n.name]?.own ?? false;
      // A node not the phone's is paired with anew, and never over a bond
      // Android holds with it: Android pairs with its old keys while the
      // node runs a new pairing, and the key check fails whatever code is
      // typed; a link over it, to a node that no longer knows it, the node
      // ends before a pairing can start (bench, 2026-11-10). So the bond
      // goes first, as on Forget; a node that still held it takes the new
      // pairing in its place (spec "Phone link", Pairing).
      if (!ours && !await _forgetBond(n.name, n.deviceId)) {
        throw BondKept(n.name);
      }
      final r = nodes.of(n.name);
      if (r.unseen || again) {
        if (!await seen(n.deviceId)) throw NotSeen(shownName(n.name));
        r.unseen = false;
      }
      return BleLink.connect(
        n.deviceId,
        name: n.name,
        hadBond: ours,
        pairing: () {
          _phase(Linking.pairing);
          session.note('Pairing with ${n.name}');
        },
        paired: () {
          _phase(Linking.paired);
          session.note('Paired with ${n.name}');
        },
      );
    }, again: () => connect(n, again: true));
  }

  /// Connects by `open` and attaches the link, which ends a stop of the
  /// search the person made. Unless the link is a `guest`'s, the node
  /// holds this phone's bond then and is kept as the paired node; one
  /// that no longer does is kept as lost, with `again` to pair with it
  /// anew, and [NotOurs] goes on to the caller. An owner's link to `name`
  /// that ended while it was read is not taken as the node holding the
  /// bond: [LinkEnded] goes on.
  Future<void> _open(
    String device,
    String name,
    Future<(NodeLink, int)> Function() open, {
    Future<void> Function()? again,
    bool guest = false,
  }) async {
    _phase(Linking.connecting);
    pairAgain = null;
    try {
      await session.setStopped(false);
      // The phone leaves a node that drains by its own choice: the link's
      // end is no reboot, and nothing is waited for.
      if (_installsNow) install = null;
      if (session.connected) await session.disconnect();
      linkDevice = device;
      final (link, m) = await open();
      _phase(Linking.attaching);
      mtu = m;
      session.note('Connected, MTU $m');
      _linkByTap = _tapHeld;
      if (await session.attach(link, guest: guest) && !guest) {
        if (!session.connected) throw LinkEnded(name);
        _roundsAfresh();
        await session.setPaired(nodeName(session.node!.id));
      }
    } on NotOurs catch (e) {
      await session.setPaired(e.node, lost: true);
      pairAgain = again;
      rethrow;
    } finally {
      _phase(Linking.none);
    }
  }

  void _phase(Linking l) {
    linking = l;
    notifyListeners();
  }

  /// The person closes the link: the app does not look for the node
  /// again until they connect.
  Future<void> disconnect() async {
    final stop = stopLooking();
    await session.disconnect();
    await stop;
  }

  /// Connects to the public node the search found, as a guest; a failure
  /// waits for the next round, which takes the strongest again. Nothing
  /// while a connect of the person's is under way ([_byTap]).
  @override
  Future<void> _connectPublic(FoundNode n) async {
    if (connecting) return;
    session.note(
      'Found public ${n.name}, ${n.rssi ?? '?'} dBm, connecting '
      '${session.toSend ? 'to send' : 'for mail'}',
    );
    try {
      await _connectGuest(n);
    } catch (e) {
      session.note('Connecting to ${n.name} failed: $e');
    }
  }

  /// Connects to the paired node the search found, by the person's path.
  /// A node no longer this phone's leaves the paired nodes the search
  /// looks for ([pairedNodes]); another failure waits for the next round.
  /// Nothing while a connect of the person's is under way ([_byTap]).
  @override
  Future<void> _connectFound(FoundNode n) async {
    if (connecting) return;
    session.note('Found ${n.name}, connecting');
    try {
      await _connect(n);
    } on NotOurs {
      return;
    } catch (e) {
      session.note('Connecting to ${n.name} failed: $e');
    }
  }
}
