// What every screen shares: the person, their contacts, the session with
// the node, the messages sealed and opened, and how the app reaches a
// node, by Bluetooth or the fake.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';

import 'alerts.dart';
import 'ble_link.dart';
import 'contacts.dart';
import 'fake_node.dart';
import 'link.dart';
import 'me.dart';
import 'messenger.dart';
import 'phone_notifier.dart';
import 'session.dart';
import 'store.dart';

class AppModel extends ChangeNotifier {
  final Sodium sodium;
  final Me me;
  final Session session;
  final Contacts contacts;
  late final Messenger messenger;
  final DateTime Function() now;
  final Notifier notifier;
  late final Alerts alerts;

  AppModel({
    required this.sodium,
    required this.me,
    required this.session,
    required this.contacts,
    required this.now,
    this.notifier = const NoNotifier(),
  }) {
    messenger = Messenger(sodium, me, contacts, session, now);
    alerts = Alerts(notifier, contacts);
    session.kept = (r) => alerts.kept(r).catchError(_unnotified);
    session.addListener(_nodeAddress);
    session.addListener(_link);
    // The countdowns move without a word from the node.
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      notifyListeners();
    });
  }

  static Future<AppModel> onPhone(Sodium s) async {
    return AppModel(
      sodium: s,
      me: Me(FileStore('me.json'), s, DateTime.now),
      session: Session(store: FileStore('bark.json'), now: DateTime.now),
      contacts: Contacts(FileStore('contacts.json')),
      now: DateTime.now,
      notifier: await PhoneNotifier.start(),
    );
  }

  late final Timer _ticker;

  /// Everything a screen redraws on.
  Listenable get changes => Listenable.merge([this, session, contacts, me]);

  Future<void> load() async {
    await Future.wait([me.load(), session.load(), contacts.load()]);
    for (final s in [me.store, session.store, contacts.store]) {
      if (s.fault != null) session.note(s.fault!);
    }
  }

  /// Whether to connect to the fake node instead of Bluetooth.
  bool fake = false;
  FakeNode? fakeNode;
  final Map<String, FoundNode> found = {};
  bool scanning = false;
  bool connecting = false;
  int? mtu;

  /// Keeps the node's address for the card, and the home's.
  void _nodeAddress() {
    final a = session.address;
    if (a == null) return;
    if (a.toString() != me.nodeAddress?.toString()) me.setNodeAddress(a);
    // The home is the node, not the spot: a fixed, awake node's address is
    // its stored place, which moves only at boot or when it turns fixed,
    // and the card and texts to the old one would miss it. A carried
    // node's address is its live fix, which moves every second, and a card
    // to every contact on each fix would flood the mesh; a sleepy node's is
    // its anchor's, which changes as it picks anchors.
    final s = session.status;
    if (s == null || s.carried || s.sleepy) return;
    if (atHome && a.toString() != me.home.toString()) unawaited(_homeMoved(a));
  }

  Future<void> _homeMoved(Address a) async {
    if (await me.setHome(a)) await messenger.sendCardToAll();
  }

  /// Keeps the app running while a node is connected by Bluetooth, so
  /// that it reads the inbox with the screen off. The fake node runs in
  /// the app and is not a connected device.
  void _link() {
    final n = session.node;
    unawaited(
      alerts
          .link(n != null && !fake ? nodeName(n.id) : null)
          .catchError(_unnotified),
    );
  }

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

  Future<void> connect(FoundNode n) async {
    await stopScan();
    connecting = true;
    notifyListeners();
    try {
      final (link, m) = await BleLink.connect(n.deviceId);
      mtu = m;
      session.note('Connected to ${n.name}, MTU $m');
      await session.attach(link);
    } finally {
      connecting = false;
      notifyListeners();
    }
  }

  Future<void> connectFake() async {
    final f = fakeNode ??= FakeNode(
      now: now,
      sodium: sodium,
      app: me.identity,
      mailbox: me.keys.mailbox,
    );
    mtu = attMtu;
    session.note('Connected to the fake node');
    await session.attach(f.connect());
  }

  @override
  void dispose() {
    _ticker.cancel();
    session.removeListener(_nodeAddress);
    session.removeListener(_link);
    fakeNode?.dispose();
    session.dispose();
    contacts.dispose();
    me.dispose();
    super.dispose();
  }
}
