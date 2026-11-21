// How the phone reaches a node over Bluetooth: Android's leave to use
// it, the person's scan, the links the app makes and attaches to the
// session, the search that chooses them (`decision.dart`), and pairing
// with Android's bonds, which go for every node that is not the
// person's (roadmap D194, D203). One object, as the search connects,
// a connect pairs, and pairing again connects.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:universal_ble/universal_ble.dart';

import 'ble_link.dart';
import 'decision.dart';
import 'installs.dart';
import 'link.dart';
import 'me.dart';
import 'node_link.dart';
import 'nodes.dart';
import 'session.dart';

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

class Links extends ChangeNotifier {
  final Session session;
  final Me me;
  final Installs installs;
  final DateTime Function() now;

  /// A firmware update is under way on the link ([Updater.underWay]):
  /// closing it would cut it.
  final bool Function() updating;

  /// Sends the fetch the person asked for while no node was linked
  /// ([Mailbox.sendQueuedFetch]), which a guest's errand waits for.
  final Future<void> Function() sendQueuedFetch;

  Links({
    required this.session,
    required this.me,
    required this.installs,
    required this.now,
    required this.updating,
    required this.sendQueuedFetch,
    Future<bool> Function()? bluetoothAllowed,
    Future<void> Function()? requestBluetooth,
  }) : _bluetoothAllowed = bluetoothAllowed ?? UniversalBle.hasPermissions,
       _requestBluetooth = requestBluetooth ?? _askAndroid {
    // The home is the search's first node, and an install is waited for.
    for (final n in [session, me, installs, this]) {
      n.addListener(_search);
    }
  }

  /// What the phone knows of each node.
  NodeStore get nodes => session.nodes;

  @override
  void dispose() {
    _scanTimer?.cancel();
    _availability?.cancel();
    _searchStarted = false;
    if (!_wake.isCompleted) _wake.complete();
    for (final n in [session, me, installs]) {
      n.removeListener(_search);
    }
    super.dispose();
  }

  // Bluetooth, as Android allows it.

  /// Asks, once, to use Bluetooth, as the app starts: without it there is
  /// no node to send through. `why` shows the person why first and says
  /// whether to go on.
  Future<void> askForBluetooth(Future<bool> Function() why) async {
    if (me.askedBluetooth || !_searchStarted) return;
    await checkBluetooth();
    if (bluetoothAllowed) return;
    await me.setAskedBluetooth();
    if (!await why()) return;
    try {
      await _requestBluetooth();
    } finally {
      await checkBluetooth();
    }
  }

  final Future<void> Function() _requestBluetooth;
  static Future<void> _askAndroid() async {
    await UniversalBle.requestPermissions();
  }

  /// Whether Android lets the app use Bluetooth, as last checked: a fresh
  /// install has not been allowed yet, and its search would only fail.
  bool bluetoothAllowed = false;
  final Future<bool> Function() _bluetoothAllowed;

  /// Checks whether Bluetooth is allowed: at start, when the app comes to
  /// the front and when Bluetooth's state changes, as the person may have
  /// allowed it in Android's settings meanwhile.
  Future<void> checkBluetooth() async {
    bool a;
    try {
      a = await _bluetoothAllowed();
    } catch (_) {
      a = false;
    }
    if (a == bluetoothAllowed) return;
    bluetoothAllowed = a;
    notifyListeners();
  }

  // The person's scan.

  /// The nodes the person's scan found, by device, and whether it runs.
  final Map<String, FoundNode> found = {};
  bool scanning = false;

  /// The last scan found nothing: it was stopped with none found, or has
  /// run [seenWait], ten of a node's advertising intervals, without one.
  bool get noneFound => found.isEmpty && (_scanEnded || _scanLong);
  bool _scanEnded = false;
  bool _scanLong = false;
  Timer? _scanTimer;

  /// Scans for nodes, after asking for Bluetooth if it is not allowed
  /// yet: the person's tap is where the app asks, and the search runs
  /// from then on.
  Future<void> scan() async {
    if (!bluetoothAllowed) {
      try {
        await _requestBluetooth();
      } finally {
        await checkBluetooth();
      }
    }
    found.clear();
    scanning = true;
    _scanEnded = false;
    _scanLong = false;
    _scanTimer?.cancel();
    _scanTimer = Timer(seenWait, () {
      _scanLong = scanning;
      notifyListeners();
    });
    notifyListeners();
    await _searchStopped();
    try {
      await startScan((n) {
        found[n.deviceId] = n;
        _heard([n]);
        unawaited(_forgetBonds([n]));
        notifyListeners();
      });
    } catch (e) {
      scanning = false;
      _scanTimer?.cancel();
      notifyListeners();
      rethrow;
    }
  }

  Future<void> stopScan() async {
    await stopBleScan();
    _scanTimer?.cancel();
    _scanEnded = scanning || _scanEnded;
    scanning = false;
    notifyListeners();
  }

  // Links.

  /// Where a connection stands until the link is attached.
  Linking linking = Linking.none;

  /// A connection is being made: no other may start, and the search
  /// does not scan. A tap's covers the whole of its connect, from the
  /// close of the old link to the new link up, as the search would take
  /// another node in the moments between ([_byTap]).
  bool get connecting => linking != Linking.none || _tapHeld;

  /// The device of the link connected or being connected, and the MTU
  /// it agreed.
  String? linkDevice;
  int? mtu;

  /// The person taps a node they are paired with in Settings: the app
  /// connects to it as a paired phone, as a tap on it in a scan does. A node
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
  Future<void> connect(FoundNode n, {bool again = false}) async {
    if (_tapHeld || (connectedTo(n.deviceId) && !session.guest)) return;
    await _byTap(() async {
      if (connectedTo(n.deviceId) && !session.guest) return;
      await _connect(n, again: again);
    });
  }

  /// The person connects to `n`, a public node, as a guest: without
  /// pairing, so that it stays its paired phones'. It ends a search round, as
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
    await sendQueuedFetch();
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
  /// anew, and [NotOurs] goes on to the caller. A paired phone's link to `name`
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
    // The person's tap tries a node the search passes over; a refusal
    // marks it again.
    if (_tapHeld) nodes.of(name).unserved = false;
    try {
      await session.setStopped(false);
      installs.leaving();
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

  // Pairing and Android's bonds.

  /// After a connect found the node no longer this phone's: forgets the
  /// old bond and connects again, which pairs.
  Future<void> Function()? pairAgain;

  /// A node whose bond Android kept ([NodeRecord.bondKept]), for the line
  /// that tells the person.
  String? get bondKept =>
      nodes.records.where((r) => r.bondKept).firstOrNull?.name;

  /// Clears [NodeRecord.bondKept] of each node whose bond Android no
  /// longer lists: the person has forgotten it in the phone's settings.
  Future<void> recheckKept() async {
    for (final r in [...nodes.records.where((r) => r.bondKept)]) {
      final device = r.heard?.deviceId;
      if (device == null || await BleLink.bonded(device)) continue;
      r.bondKept = false;
      session.note('Pairing: the phone no longer holds a bond with ${r.name}');
      notifyListeners();
    }
  }

  /// Has Android remove its bond with the node `name` at `device`, if it
  /// lists one, as the app makes no link over a bond to a node it does
  /// not count as its own: the node would take the link as its owner's,
  /// or end it (spec "Phone link", Public). True once none is left; where
  /// Android keeps it, false, and the node's record says so
  /// ([NodeRecord.bondKept]).
  Future<bool> _forgetBond(String name, String device) async {
    if (!await BleLink.bonded(device)) return true;
    final r = nodes.of(name);
    if (await BleLink.forget(device)) {
      r.unseen = true;
      session.note('Pairing: the phone forgot its bond with $name');
      notifyListeners();
      return true;
    }
    if (!r.bondKept) {
      session.note('Pairing: the phone kept its bond with $name');
    }
    r.bondKept = true;
    notifyListeners();
    return false;
  }

  /// Removes the bond Android holds with each node of `heard` that the app
  /// does not count as its own, as it hears it, so that the node is a
  /// public node like any other. A node of an identity file whose bond
  /// Android holds is the phone's own still: the file came back to the
  /// phone that made the pairing. If the node forgot it since, the first
  /// link says so.
  Future<void> _forgetBonds(Iterable<FoundNode> heard) async {
    for (final n in heard) {
      if (n.name == n.deviceId) continue;
      final r = nodes.of(n.name);
      if (r.moved) {
        r.moved = false;
        if (await BleLink.bonded(n.deviceId)) {
          await session.setPaired(n.name);
          continue;
        }
        await session.save(state: true);
      }
      if (r.own || r.bondKept) continue;
      await _forgetBond(n.name, n.deviceId);
    }
  }

  /// The owner gives the connected node up: the node forgets this phone's
  /// bond, then the phone forgets its own, as Android would encrypt every
  /// later link to the node with it, a guest's too, and the node refuses
  /// it (spec "Phone link", Public). Returns whether the phone's bond is
  /// gone; where Android keeps it, the person forgets the node in the
  /// phone's Bluetooth settings, and [bondKept] says so meanwhile.
  Future<bool> unpair() async {
    _unpairing = true;
    notifyListeners();
    try {
      return await _unpair();
    } finally {
      _unpairing = false;
      notifyListeners();
    }
  }

  /// An unpairing is under way: the search waits, as the node ends the
  /// link while the phone still holds the bond, and a connect with it
  /// then would find the node no longer this phone's.
  bool _unpairing = false;

  Future<bool> _unpair() async {
    final device = linkDevice;
    final n = session.node;
    await session.unpair();
    if (device == null || n == null) return true;
    final name = nodeName(n.id);
    _roundsAfresh();
    return _forgetBond(name, device);
  }

  /// The node `name` as last heard, if heard within the span the search
  /// takes to call a node out of reach ([ownGone] rounds): in reach.
  FoundNode? heardLately(String name) {
    final r = nodes[name];
    final at = r?.heardAt;
    return at != null && now().difference(at) < lookRound * ownGone
        ? r!.heard
        : null;
  }

  /// Whether `name`, a paired node no longer this phone's, may be paired
  /// with again now ([pairLost]): heard lately.
  bool lostInReach(String name) => heardLately(name) != null;

  /// The person pairs again with `name`, a paired node that no longer
  /// knows this phone, by the code on its screen, as **Pair** on its row
  /// in a scan does ([connect]).
  Future<void> pairLost(String name) async {
    final n = heardLately(name);
    if (n == null) throw NotSeen(shownName(name));
    await connect(n);
  }

  /// Forgets `name`, a paired node no longer this phone's: the app counts
  /// it paired no more, and has Android remove its bond with the node, as
  /// [unpair] does, where it has heard the node in this run (roadmap
  /// D173). Returns true once no bond is left, false where Android kept
  /// it ([bondKept] then says so), and null where the app has not heard
  /// the node: it removes the bond once it does ([_forgetBonds]), or the
  /// person forgets the node in the phone's Bluetooth settings. The home
  /// stays, as the node keeps the mailbox key.
  Future<bool?> forget(String name) async {
    await session.forget(name);
    final device = nodes[name]?.heard?.deviceId;
    if (device == null) return null;
    return _forgetBond(name, device);
  }

  // The search.

  /// Whether the search has started, which only the app on a phone does:
  /// a test starts it to drive it.
  bool _searchStarted = false;

  /// Bluetooth is on, by Android's word.
  bool bluetoothOn = false;
  StreamSubscription<AvailabilityState>? _availability;

  /// The search's loop runs.
  bool _searching = false;

  /// The scan under way, which ends once it has stopped.
  Future<void>? _round;

  /// Completed to end the search's scan or wait at once.
  Completer<void> _wake = Completer<void>();

  /// What the search did when last asked ([modeOf]); a change wakes it.
  Mode _mode = Mode.idle;

  /// The search's rounds in a row that have reached none of the paired
  /// nodes.
  int _missed = 0;

  /// When the search last started a scan, a round's or a look-around's.
  DateTime? _scanned;

  /// The link the search last saw, and when the phone last lost one it
  /// held between rounds ([Situation.droppedAt]).
  LinkNow? _had;
  DateTime? _dropped;

  /// What the last whole round heard.
  List<FoundNode> _lastRound = const [];

  /// The last failure the search noted, so that a failure every round
  /// is told once.
  String? _searchFailed;

  /// The nodes this phone is paired with that still hold its bond; one
  /// that no longer does is the person's to decide on, with Pair again.
  Set<String> get pairedNodes => nodes.own;

  /// The home's node, by name, if one is set.
  String? get homeName {
    final h = me.home;
    return h == null ? null : nodeName(h.id);
  }

  /// The paired node the search looks for first: the home when the phone
  /// is paired with it, else the one connected last; of those it may use
  /// ([NodeRecord.usable]).
  String? get firstPaired {
    final p = nodes.usable;
    final h = homeName;
    return h != null && p.contains(h) ? h : p.lastOrNull;
  }

  /// The phone's situation, on which the search decides ([decide]), with
  /// what a scan just `heard`.
  Situation situation({List<FoundNode>? heard}) => Situation(
    now: now(),
    home: homeName,
    nodes: nodes.records,
    link: _linkNow,
    linking: connecting || _unpairing,
    radio: bluetoothAllowed && bluetoothOn && !scanning,
    stopped: session.stopped,
    awaited: installs.awaited,
    missed: _missed,
    busy: !settled,
    toSend: session.toSend,
    scannedAt: _scanned,
    droppedAt: _dropped,
    heard: heard,
  );

  /// Starts the search, which then runs whenever it should: at start, and
  /// after a link drops.
  void startSearch() {
    if (_searchStarted) return;
    _searchStarted = true;
    _availability = UniversalBle.availabilityStream.listen((a) {
      bluetoothOn = a == AvailabilityState.poweredOn;
      notifyListeners();
      unawaited(checkBluetooth());
    });
    unawaited(checkBluetooth());
    notifyListeners();
  }

  /// Runs the search when it should, and ends a scan or wait under way
  /// when what it does changes: a link that drops starts a round, and one
  /// that comes up ends the round's wait.
  void _search() {
    if (!_searchStarted) return;
    final l = _linkNow;
    final had = _had;
    // An errand's link is the round's own, and its end no news.
    if (l == null && had != null && (!had.guest || had.byTap)) {
      _dropped = now();
    }
    _had = l;
    final m = modeOf(situation());
    if (m != _mode) {
      _mode = m;
      if (!_wake.isCompleted) _wake.complete();
    }
    if (m == Mode.idle || _searching) return;
    _searching = true;
    unawaited(_run());
  }

  /// The search: decides, carries the decision out, and decides again on
  /// what came of it, until there is nothing to do.
  Future<void> _run() async {
    try {
      List<FoundNode>? heard;
      while (_searchStarted) {
        if (_wake.isCompleted) _wake = Completer<void>();
        var s = situation(heard: heard);
        var d = decide(s);
        if (d case GiveUp(:final node)) {
          installs.giveUp(node);
          s = situation(heard: heard);
          d = decide(s);
        }
        heard = null;
        switch (d) {
          case Idle():
            // A scan cut short by a change heard too little to judge: the
            // next round comes as soon as the search runs again.
            if (s.heard != null) _scanned = null;
            return;
          case Wait(:final wait):
            await _sleep(wait);
          case Listen(:final until):
            heard = await _listenRound(until);
          case Connect(:final node):
            await _connectFound(node);
          case Errand(:final node):
            await _connectPublic(node);
          case MoveHome(:final node):
            session.note(
              'Heard ${node.name}, ${node.rssi ?? '?'} dBm: moving to it',
            );
            await _connectFound(node);
          case GiveUp():
            throw StateError('decided to give up twice');
        }
        if (!_searchStarted) return;
        if (s.heard case final round?) _roundDone(round, s);
      }
    } finally {
      _searching = false;
    }
  }

  /// Waits `d`, or until [_wake] completes.
  Future<void> _sleep(Duration d) async {
    final t = Timer(d, () {
      if (!_wake.isCompleted) _wake.complete();
    });
    await _wake.future;
    t.cancel();
  }

  /// A scan of one round, ended early once `until` is heard well; then
  /// Android's bond with each node heard that the app does not count as
  /// its own goes ([_forgetBonds]), before anything is done with them.
  Future<List<FoundNode>> _listenRound(String? until) async {
    _scanned = now();
    final round = _listen(
      until == null ? null : (n) => n.name == until && n.usable,
    );
    _round = round;
    final heard = await round;
    _round = null;
    await recheckKept();
    await _forgetBonds(heard.values);
    return heard.values.toList();
  }

  /// What the round that heard `heard`, decided on in `s`, leaves behind:
  /// a miss of the paired nodes where it left the phone on none of them
  /// ([countsMiss]), and the public node in use no longer, once a round
  /// does not hear it.
  void _roundDone(List<FoundNode> heard, Situation s) {
    _lastRound = heard;
    final g = nodes.guest;
    if (g != null && !heard.any((h) => h.name == g)) nodes.guest = null;
    if (!countsMiss(s) || (session.connected && !session.guest)) return;
    _missed++;
    if (_missed == ownGone) {
      session.note(
        'Not reached ${s.own.join(', ')} in $ownGone rounds: out of reach, '
        'using public nodes as a guest',
      );
    }
    notifyListeners();
  }

  void _heard(Iterable<FoundNode> heard) {
    for (final n in heard) {
      nodes.heard(n, now());
    }
  }

  Future<Map<String, FoundNode>> _listen(
    bool Function(FoundNode)? until,
  ) async {
    try {
      final heard = await listen(until: until, stop: _wake.future);
      _heard(heard.values);
      _searchFailed = null;
      // Which nodes are in reach moved ([heardLately]).
      notifyListeners();
      return heard;
    } catch (e) {
      final f = 'Looking for nodes failed: $e';
      if (f != _searchFailed) session.note(f);
      _searchFailed = f;
      return const {};
    }
  }

  /// The phone reached a node of its own, or gave one up: the rounds
  /// that missed the paired nodes, and the public node used meanwhile,
  /// bear on what comes next no more.
  void _roundsAfresh() {
    if (ownAway) session.note('Back on a paired node');
    _missed = 0;
    nodes.guest = null;
  }

  /// Ends the search's scan and waits until it has stopped, so that a
  /// scan or connect of the person's does not meet it.
  Future<void> _searchStopped() async {
    if (!_wake.isCompleted) _wake.complete();
    await _round;
  }

  // What the screens say of the search.

  /// Whether the search runs, as the person reads it: not once they
  /// stopped it, nor on a node of theirs, nor on a guest's link they made
  /// by a tap ([tappedGuest]), nor while unpairing.
  bool get searchOn {
    if (!_searchStarted || session.stopped || _unpairing) return false;
    final l = _linkNow;
    return l == null || (l.guest && !l.byTap);
  }

  /// The paired node the search looks for first, while it runs
  /// ([firstPaired]); null without one, when it looks for public nodes
  /// alone.
  String? get searchFor => searchOn ? firstPaired : null;

  /// Whether the search scans now: not before Bluetooth is allowed, nor
  /// while it is off, a connection is being made, or the person scans.
  bool get lookingNow =>
      searchOn && bluetoothAllowed && bluetoothOn && !connecting && !scanning;

  /// The phone, on a paired node, moves to the home once it hears it
  /// ([homeAbove]).
  bool get movesHome => _searchStarted && homeAbove(situation()) != null;

  /// The last round heard a node of the phone's own, too weak to take
  /// ([ownPassedOver]), and the phone uses a public node meanwhile.
  bool get ownWeak =>
      !(session.connected && !session.guest) &&
      ownPassedOver(situation(heard: _lastRound));

  /// Whether the link in use may be closed with nothing lost: nothing in
  /// flight on it ([Session.inFlight]) and no firmware update under way
  /// ([Updater.underWay]).
  bool get settled => !session.inFlight && !updating();

  /// The paired node the search is scanning for first now.
  String? get lookingFor => lookingNow ? firstPaired : null;

  /// The paired nodes are out of reach: [ownGone] rounds in a row have
  /// reached none the search may use, and the phone uses public nodes as a
  /// guest meanwhile.
  bool get ownAway => nodes.usable.isNotEmpty && _missed >= ownGone;

  /// The public node the phone uses as a guest, between its errands too:
  /// the node of the last, while the search runs; or the node of a guest's
  /// link the person made by a tap, while it is up.
  String? get guestNode {
    final l = _linkNow;
    if (l != null && l.guest && l.byTap) return l.node;
    return searchOn ? nodes.guest : null;
  }

  /// The link up is a guest's the person made by a tap, which the node
  /// ends after a guest's 30 s (spec "Phone link", Public).
  bool get tappedGuest {
    final l = _linkNow;
    return l != null && l.guest && l.byTap;
  }

  /// The record of [guestNode], once an errand there has read it, for
  /// making it the home or pairing with it between errands.
  NodeRecord? get guestHere {
    final g = guestNode;
    final r = g == null ? null : nodes[g];
    return r?.asGuest != null && r?.heard != null ? r : null;
  }

  /// The person stops the search for the paired node, until they connect.
  Future<void> stopLooking() => session.setStopped(true);
}
