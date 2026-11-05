// What every screen shares: the person, their contacts, the session with
// the node, the messages sealed and opened, and how the app reaches a
// node, by Bluetooth or the fake.

import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:sodium/sodium.dart';

import 'ble_link.dart';
import 'contacts.dart';
import 'fake_node.dart';
import 'link.dart';
import 'me.dart';
import 'messenger.dart';
import 'session.dart';
import 'store.dart';

class AppModel extends ChangeNotifier {
  final Sodium sodium;
  final Me me;
  final Session session;
  final Contacts contacts;
  late final Messenger messenger;
  final DateTime Function() now;

  AppModel({
    required this.sodium,
    required this.me,
    required this.session,
    required this.contacts,
    required this.now,
  }) {
    messenger = Messenger(sodium, me, contacts, session, now);
    session.addListener(_nodeAddress);
    // The countdowns move without a word from the node.
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      notifyListeners();
    });
  }

  factory AppModel.onPhone(Sodium s) {
    return AppModel(
      sodium: s,
      me: Me(FileStore('me.json'), s, DateTime.now),
      session: Session(store: FileStore('bark.json'), now: DateTime.now),
      contacts: Contacts(FileStore('contacts.json')),
      now: DateTime.now,
    );
  }

  late final Timer _ticker;
  Timer? _settle;

  /// How long the node's address must hold before the app fetches its
  /// mail to it: the delivery TTL, which the network keeps a dark node's
  /// mail for (spec "Nodes", "Mailbox").
  static const settle = Duration(seconds: 600);

  /// Everything a screen redraws on.
  Listenable get changes => Listenable.merge([this, session, contacts, me]);

  Future<void> load() async {
    await Future.wait([me.load(), session.load(), contacts.load()]);
  }

  /// Whether to connect to the fake node instead of Bluetooth.
  bool fake = false;
  FakeNode? fakeNode;
  final Map<String, FoundNode> found = {};
  bool scanning = false;
  bool connecting = false;
  int? mtu;

  /// Keeps the node's address for the card, and fetches the person's
  /// mail once a new address away from home has held for [settle].
  void _nodeAddress() {
    final a = session.address;
    if (a == null || a.toString() == me.nodeAddress?.toString()) return;
    me.setNodeAddress(a);
    _settle?.cancel();
    if (!needsFetch(a)) return;
    _settle = Timer(settle, () {
      if (session.address?.toString() == a.toString() && needsFetch(a)) {
        messenger.fetch(a);
      }
    });
  }

  /// A home is set, the node is elsewhere, and its mail has not been
  /// fetched to here.
  bool needsFetch(Address here) {
    final home = me.home;
    return home != null &&
        home.toString() != here.toString() &&
        me.fetchedTo?.toString() != here.toString();
  }

  /// Fetches now, at the person's word, to the node's address.
  Future<bool> fetchNow() async {
    final here = session.address;
    if (here == null || me.home == null) return false;
    await messenger.fetch(here);
    return true;
  }

  /// Makes the connected node the person's home, or clears it, and
  /// sends the new card to every contact if its address changed.
  Future<void> setHome(Address? a) async {
    if (await me.setHome(a)) await messenger.sendCardToAll();
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
    final f = fakeNode ??= FakeNode(now: now, sodium: sodium, app: me.identity);
    mtu = attMtu;
    session.note('Connected to the fake node');
    await session.attach(f.connect());
  }

  @override
  void dispose() {
    _ticker.cancel();
    _settle?.cancel();
    session.removeListener(_nodeAddress);
    fakeNode?.dispose();
    session.dispose();
    contacts.dispose();
    me.dispose();
    super.dispose();
  }
}
