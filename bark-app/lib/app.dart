// What every screen shares: the session with the node, the contacts,
// and how the app reaches a node, by Bluetooth or the fake.

import 'dart:async';

import 'package:flutter/foundation.dart';

import 'ble_link.dart';
import 'contacts.dart';
import 'fake_node.dart';
import 'link.dart';
import 'session.dart';
import 'store.dart';

class AppModel extends ChangeNotifier {
  final Session session;
  final Contacts contacts;

  AppModel({required this.session, required this.contacts}) {
    session.addListener(_keepMyAddress);
    // The countdowns move without a word from the node.
    _ticker = Timer.periodic(const Duration(seconds: 1), (_) {
      notifyListeners();
    });
  }

  factory AppModel.onPhone() => AppModel(
    session: Session(store: FileStore('bark.json'), now: DateTime.now),
    contacts: Contacts(FileStore('contacts.json')),
  );

  late final Timer _ticker;

  /// Everything a screen redraws on.
  Listenable get changes => Listenable.merge([this, session, contacts]);

  Future<void> load() async {
    await Future.wait([session.load(), contacts.load()]);
  }

  /// Whether to connect to the fake node instead of Bluetooth.
  bool fake = false;
  FakeNode? fakeNode;
  final Map<String, FoundNode> found = {};
  bool scanning = false;
  bool connecting = false;
  int? mtu;

  void _keepMyAddress() {
    final a = session.address;
    if (a != null) contacts.setMyAddress(a);
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
      session.senderOf = (_) => null;
      await session.attach(link);
    } finally {
      connecting = false;
      notifyListeners();
    }
  }

  Future<void> connectFake() async {
    final f = fakeNode ??= FakeNode(now: DateTime.now);
    mtu = attMtu;
    session.note('Connected to the fake node');
    session.senderOf = f.senderOf;
    await session.attach(f.connect());
  }

  @override
  void dispose() {
    _ticker.cancel();
    session.removeListener(_keepMyAddress);
    fakeNode?.dispose();
    session.dispose();
    contacts.dispose();
    super.dispose();
  }
}
