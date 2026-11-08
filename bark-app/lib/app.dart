// What every screen shares: the person, their contacts, the session with
// the node, the messages sealed and opened, and how the app reaches a
// node, by Bluetooth or the fake.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';
import 'package:universal_ble/universal_ble.dart';

import 'alerts.dart';
import 'ble_link.dart';
import 'contacts.dart';
import 'fake_node.dart';
import 'link.dart';
import 'me.dart';
import 'messenger.dart';
import 'node_link.dart';
import 'phone_notifier.dart';
import 'releases.dart';
import 'session.dart';
import 'store.dart';
import 'updater.dart';

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

class AppModel extends ChangeNotifier {
  final Sodium sodium;
  final Me me;
  final Session session;
  final Contacts contacts;
  late final Messenger messenger;
  final DateTime Function() now;
  final Notifier notifier;
  late final Alerts alerts;

  /// The latest release, against this app and the connected node.
  final Releases releases;

  AppModel({
    required this.sodium,
    required this.me,
    required this.session,
    required this.contacts,
    required this.now,
    this.notifier = const NoNotifier(),
    Releases? releases,
  }) : releases = releases ?? Releases(MemoryStore('release'), now: now) {
    this.releases.noted = session.note;
    messenger = Messenger(sodium, me, contacts, session, now);
    alerts = Alerts(notifier, contacts);
    session.kept = (r) =>
        alerts.kept(r, session.partsOf(r)).catchError(_unnotified);
    session.addListener(_nodeAddress);
    session.addListener(_link);
    session.addListener(_search);
    addListener(_link);
    addListener(_search);
    // The countdowns move without a word from the node, and the check for
    // a release is due a day after the last.
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      notifyListeners();
      unawaited(this.releases.check());
    });
  }

  static Future<AppModel> onPhone(Sodium s) async {
    return AppModel(
      sodium: s,
      me: Me(FileStore('me.json'), s, DateTime.now),
      session: Session(store: FileStore('bark.json'), now: DateTime.now)
        // debugPrint, unlike dart:developer's log, reaches the Android log
        // in release builds too, under the tag flutter.
        ..noted = (l) => debugPrint('bark ${logTime(l.at)} ${l.text}'),
      contacts: Contacts(FileStore('contacts.json')),
      now: DateTime.now,
      notifier: await PhoneNotifier.start(),
      releases: Releases(
        FileStore('release.json'),
        now: DateTime.now,
        fetch: fetchLatest,
        app: appVersion,
      ),
    );
  }

  late final Timer _ticker;

  /// The firmware update of the connected node.
  late final Updater updater = Updater(
    session,
    () => mtu ?? defaultAttMtu,
    release: () => releases.latest,
  );

  /// Everything a screen redraws on.
  Listenable get changes =>
      Listenable.merge([this, session, contacts, me, updater, releases]);

  Future<void> load() async {
    await Future.wait([
      me.load(),
      session.load(),
      contacts.load(),
      releases.load(),
    ]);
    for (final s in [me.store, session.store, contacts.store, releases.store]) {
      if (s.fault != null) session.note(s.fault!);
    }
  }

  /// Whether to connect to the fake node instead of Bluetooth.
  bool fake = false;
  FakeNode? fakeNode;
  final Map<String, FoundNode> found = {};
  bool scanning = false;
  int? mtu;

  /// Where a connection stands until the link is attached.
  Linking linking = Linking.none;

  /// A connection is being made: no other may start.
  bool get connecting => linking != Linking.none;

  /// The device of the link connected or being connected: a found
  /// node's ID, or [fakeDevice].
  String? linkDevice;
  static const fakeDevice = 'fake';

  /// After a connect found the node no longer this phone's: forgets the
  /// old bond and connects again, which pairs.
  Future<void> Function()? pairAgain;

  /// Keeps the node's address for the card, and the home's.
  void _nodeAddress() {
    final a = session.address;
    if (a == null) return;
    if (a.toString() != me.nodeAddress?.toString()) me.setNodeAddress(a);
    // The home is the node, not the spot: a fixed, awake node's address is
    // its stored place, which moves only at boot or when it turns fixed,
    // and the card and texts to the old one would miss it. A carried
    // node's address is its live fix, which moves every second, and would
    // change the card as often; a sleepy node's is its anchor's, which
    // changes as it picks anchors. The changed card waits for the person to
    // send it.
    final s = session.status;
    if (s == null || s.carried || s.sleepy) return;
    if (atHome && a.toString() != me.home.toString()) unawaited(me.setHome(a));
  }

  /// The card changed and has not gone to the contacts. Without contacts
  /// there is no one to send it to.
  bool get cardUnsent => me.cardUnsent && contacts.list.isNotEmpty;

  /// Why the card changed, for the line that offers to send it.
  String get cardChange => me.home == null
      ? 'Your card changed: you have no home now'
      : 'Your card changed: your home node has a new address';

  /// Keeps the app running while a node is connected by Bluetooth, so
  /// that it reads the inbox with the screen off, and while it searches
  /// for the paired node, so that the texts waiting go out as soon as it
  /// is in reach. The fake node runs in the app and is not a connected
  /// device.
  ///
  /// The app also starts with the phone and looks for the paired node,
  /// unless it is no longer this phone's or the person stopped looking:
  /// a phone that held a link when it was switched off takes it up again
  /// without anyone opening the app.
  void _link() {
    final n = session.node;
    final paired = searchFor;
    final Held? h = n != null && !fake
        ? (
            title: 'Connected to ${nodeName(n.id)}',
            body: 'Texts arrive while the screen is off',
          )
        : paired != null
        ? _looking(paired, bluetooth: bluetoothOn)
        : null;
    unawaited(alerts.hold(h).catchError(_unnotified));
    final p = session.pairedWith;
    final boot = p == null || session.pairingLost || session.stopped
        ? null
        : _looking(p, bluetooth: true);
    unawaited(alerts.atBoot(boot).catchError(_unnotified));
  }

  static Held _looking(String node, {required bool bluetooth}) => (
    title: 'Looking for $node',
    body: bluetooth
        ? 'Texts waiting go out once it is in reach'
        : 'Bluetooth is off',
  );

  void _unnotified(Object e) => session.note('Notifying failed: $e');

  /// Asks, once, to notify; `why` shows the person why first and says
  /// whether to go on.
  Future<void> askToNotify(Future<bool> Function() why) async {
    if (me.askedToNotify) return;
    await me.setAskedToNotify();
    if (await notifier.allowed()) return;
    if (await why()) await notifier.ask();
  }

  /// The connected node is the person's home.
  bool get atHome {
    final n = session.node;
    return n != null && messenger.isHome(n.id);
  }

  /// The connected node holds a mailbox key and is not the person's home:
  /// a former home of theirs, or someone else's.
  bool get holdsOtherKey => !atHome && (session.nodeHome?.set ?? false);

  /// Fetches to the node's address; returns what to tell the person. Only
  /// the person's word fetches, as fetched mail crosses the city twice
  /// (roadmap D128).
  Future<String> fetchNow() async {
    if (me.home == null) return 'No home node to fetch from';
    if (!session.connected) {
      return 'Not connected to a node: a fetch is sent from another node '
          'to your home, and the mail comes back to that node';
    }
    if (atHome) {
      return 'Connected to your home node: your mail comes from its inbox '
          'here; a fetch is sent from another node';
    }
    if (session.fetchWaiting != null) {
      return 'Still waiting for your home node to answer the last fetch';
    }
    final here = session.address;
    if (here == null) {
      return 'This node has no address yet, so your mail could not come '
          'back to it';
    }
    session.fetchNews = null;
    await messenger.fetch(here);
    return 'Fetching your mail';
  }

  static const _oldHomeKeeps =
      'Your old home node still holds your mailbox key until you connect '
      'to it and press "Clear its mailbox key" there.';

  /// Makes the connected node, at `a`, the person's home: it is given the
  /// mailbox key and is the home only if it then says it holds one. The
  /// new card goes to every contact if its address changed. Returns what
  /// to tell the person.
  Future<String> makeHome(Address a) async {
    final n = session.node;
    if (n == null) return 'Not connected to a node';
    if (n.version < homeVersion) {
      return 'This node\'s firmware has no mailbox (link version '
          '${n.version}): update it to make it a home. Nothing changed.';
    }
    final h = await session.writeHome(me.keys.mailbox.extractBytes());
    if (!h.set) {
      return 'The node did not keep your mailbox key, so it cannot be your '
          'home. Nothing changed.';
    }
    final away = me.home != null && !atHome;
    if (await me.setHome(a)) await messenger.sendCardToAll();
    return away
        ? 'This node is your home. $_oldHomeKeeps'
        : 'This node is your home';
  }

  /// No home node. Connected to the home, it forgets the mailbox key;
  /// elsewhere the home keeps it, and the person is told.
  Future<String> noHome() async {
    if (me.home == null) return 'No home node';
    final at = atHome;
    if (at && (await session.writeHome(null)).set) {
      return 'The node still says it holds a mailbox key, so it is still '
          'your home. Nothing changed.';
    }
    if (await me.setHome(null)) await messenger.sendCardToAll();
    return at ? 'No home node' : 'No home node. $_oldHomeKeeps';
  }

  /// Takes the mailbox key from a connected node that is not the home.
  Future<String> clearKey() async {
    final h = await session.writeHome(null);
    return h.set
        ? 'The node still says it holds a mailbox key'
        : 'The node no longer holds a mailbox key';
  }

  void setFake(bool v) {
    fake = v;
    if (!v) {
      fakeNode?.dispose();
      fakeNode = null;
    }
    notifyListeners();
  }

  Future<void> scan() async {
    found.clear();
    scanning = true;
    notifyListeners();
    await _searchStopped();
    try {
      await startScan((n) {
        found[n.deviceId] = n;
        notifyListeners();
      });
    } catch (e) {
      scanning = false;
      notifyListeners();
      rethrow;
    }
  }

  Future<void> stopScan() async {
    await stopBleScan();
    scanning = false;
    notifyListeners();
  }

  /// Whether `device` is the one connected.
  bool connectedTo(String device) => session.connected && linkDevice == device;

  /// The person connects to `n`, after closing the link to another
  /// node; `again` after they chose to pair again, which forgets the
  /// phone's old bond and finds the node in a scan first ([seen]). It
  /// ends the search for the paired node. Nothing while a connection is
  /// being made or `n` is connected already: a second connect to the
  /// same node would close the link the first made.
  Future<void> connect(FoundNode n, {bool again = false}) async {
    if (connecting || connectedTo(n.deviceId)) return;
    await _connect(n, again: again);
  }

  Future<void> _connect(FoundNode n, {bool again = false}) async {
    final s = session;
    await _open(n.deviceId, () async {
      await _searchStopped();
      await stopScan();
      if (again) {
        await BleLink.forget(n.deviceId);
        if (!await seen(n.deviceId)) throw NotSeen(n.name);
      }
      return BleLink.connect(
        n.deviceId,
        name: n.name,
        hadBond: !again && s.pairedWith == n.name && !s.pairingLost,
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

  Future<void> connectFake() async {
    if (connecting || connectedTo(fakeDevice)) return;
    final f = fakeNode ??= FakeNode(
      now: now,
      sodium: sodium,
      app: me.identity,
      mailbox: me.keys.mailbox,
    );
    await _open(
      fakeDevice,
      () async => (f.connect(), attMtu),
      again: () async {
        f.pair();
        await connectFake();
      },
    );
  }

  /// Connects by `open` and attaches the link, which ends a stop of the
  /// search the person made; the node holds this phone's bond then. A
  /// node that no longer does is kept as lost, with `again` to pair with
  /// it anew, and [NotOurs] goes on to the caller.
  Future<void> _open(
    String device,
    Future<(NodeLink, int)> Function() open, {
    required Future<void> Function() again,
  }) async {
    _phase(Linking.connecting);
    pairAgain = null;
    try {
      await session.setStopped(false);
      if (session.connected) await session.disconnect();
      linkDevice = device;
      final (link, m) = await open();
      _phase(Linking.attaching);
      mtu = m;
      session.note('Connected, MTU $m');
      if (await session.attach(link)) {
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

  /// The person stops the search for the paired node, until they connect.
  Future<void> stopLooking() => session.setStopped(true);

  /// Whether the search has started, which only the app on a phone does:
  /// a test starts it to drive it.
  bool _searchStarted = false;

  /// Bluetooth is on, by Android's word.
  bool bluetoothOn = false;
  StreamSubscription<AvailabilityState>? _availability;

  /// The search runs: a round scans, or waits for the next.
  bool _searching = false;

  /// The round under way, which ends once its scan has stopped.
  Future<void>? _round;

  /// Completed to end the search's scan or wait at once.
  Completer<void> _wake = Completer<void>();

  /// The last failure the search noted, so that a failure every round
  /// is told once.
  String? _searchFailed;

  /// The node the app searches for while it is not connected: the paired
  /// node, unless it is no longer this phone's (then the person decides,
  /// with Pair again), the person stopped the search, or the fake node
  /// is in use.
  String? get searchFor {
    final p = session.pairedWith;
    if (!_searchStarted || fake || session.stopped || session.connected) {
      return null;
    }
    if (p == null || session.pairingLost) return null;
    return p;
  }

  /// The node the search is scanning for now: not while Bluetooth is
  /// off, a connection is being made, or the person scans.
  String? get lookingFor =>
      bluetoothOn && !connecting && !scanning ? searchFor : null;

  /// Starts the search for the paired node, which then runs whenever it
  /// should: at start, and after a link drops.
  void startSearch() {
    if (_searchStarted) return;
    _searchStarted = true;
    _availability = UniversalBle.availabilityStream.listen((a) {
      bluetoothOn = a == AvailabilityState.poweredOn;
      notifyListeners();
    });
    notifyListeners();
  }

  /// Starts the search's rounds when it should run, and ends a scan or
  /// wait when it should not.
  void _search() {
    if (lookingFor == null) {
      if (!_wake.isCompleted) _wake.complete();
      return;
    }
    if (_searching) return;
    _searching = true;
    _wake = Completer<void>();
    unawaited(_rounds());
  }

  Future<void> _rounds() async {
    try {
      while (true) {
        final name = _awake();
        if (name == null) break;
        final start = now();
        final round = _look(name);
        _round = round;
        final n = await round;
        _round = null;
        if (n != null && lookingFor == name) {
          await _connectFound(n);
        }
        if (_awake() == null) break;
        final left = lookRound - now().difference(start);
        if (left > Duration.zero) {
          final t = Timer(left, () {
            if (!_wake.isCompleted) _wake.complete();
          });
          await _wake.future;
          t.cancel();
        }
      }
    } finally {
      _searching = false;
    }
  }

  /// [lookingFor], with a new [_wake] when the search goes on: the last
  /// may have ended a scan or wait for a change that has passed.
  String? _awake() {
    final name = lookingFor;
    if (name != null && _wake.isCompleted) _wake = Completer<void>();
    return name;
  }

  Future<FoundNode?> _look(String name) async {
    try {
      final n = await look(name, stop: _wake.future);
      _searchFailed = null;
      return n;
    } catch (e) {
      final f = 'Looking for $name failed: $e';
      if (f != _searchFailed) session.note(f);
      _searchFailed = f;
      return null;
    }
  }

  /// Connects to the paired node the search found, by the person's path.
  /// A node no longer this phone's ends the search, as [searchFor] says;
  /// another failure waits for the next round.
  Future<void> _connectFound(FoundNode n) async {
    session.note('Found ${n.name}, connecting');
    try {
      await _connect(n);
    } on NotOurs {
      return;
    } catch (e) {
      session.note('Connecting to ${n.name} failed: $e');
    }
  }

  /// Ends the search's scan and waits until it has stopped, so that a
  /// scan or connect of the person's does not meet it.
  Future<void> _searchStopped() async {
    if (!_wake.isCompleted) _wake.complete();
    await _round;
  }

  @override
  void dispose() {
    _ticker.cancel();
    _availability?.cancel();
    _searchStarted = false;
    if (!_wake.isCompleted) _wake.complete();
    session.removeListener(_nodeAddress);
    session.removeListener(_link);
    session.removeListener(_search);
    fakeNode?.dispose();
    updater.dispose();
    releases.dispose();
    session.dispose();
    contacts.dispose();
    me.dispose();
    super.dispose();
  }
}
