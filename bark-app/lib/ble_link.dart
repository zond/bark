// The phone link over Bluetooth LE.

import 'dart:async';
import 'dart:typed_data';

import 'package:universal_ble/universal_ble.dart';

import 'link.dart';
import 'node_link.dart';

/// A node seen in a scan.
class FoundNode {
  final String deviceId;
  final String name;
  final int? rssi;
  const FoundNode(this.deviceId, this.name, this.rssi);
}

/// The ATT transaction timeout (Bluetooth Core Specification 5.0, Vol 3
/// Part F §3.3.3). The node answers Send only once it has the message,
/// so a write may wait longer than the plugin's default.
const Duration attTimeout = Duration(seconds: 30);

/// Scans for nodes by the service UUID; each result goes to `found`.
Future<void> startScan(void Function(FoundNode) found) async {
  UniversalBle.onScanResult = (d) {
    final name = d.name ?? '';
    found(FoundNode(d.deviceId, name.isEmpty ? d.deviceId : name, d.rssi));
  };
  await _scanForNodes();
}

Future<void> _scanForNodes({AndroidScanMode? mode}) => UniversalBle.startScan(
  scanFilter: ScanFilter(withServices: [Uuids.service]),
  // The node advertises with legacy advertising PDUs (spec "Phone link").
  platformConfig: PlatformConfig(
    android: AndroidOptions(legacy: true, scanMode: mode),
  ),
);

/// How long a scan looks for one node: ten of its advertising intervals
/// of 1022.5 ms (spec "Phone link", Finding the node), so that a phone
/// that misses some of its advertisements still sees it.
const Duration seenWait = Duration(milliseconds: 10 * 1022 + 5);

/// Scans until the node `deviceId` is seen, at most `wait`; whether it
/// was.
///
/// Android connects to an address as public unless a scan result or a
/// bond has told it the address is random, and the node's is random
/// static (spec "Phone link"; roadmap D135): with neither, Android looks
/// for the wrong address and its connect times out after 30 s (bench,
/// 2026-11-06, Pair again). The Bluetooth plugin has no connect that
/// takes the address's type, so a node is seen in a scan before a
/// connect that has no bond to go by.
Future<bool> seen(String deviceId, {Duration wait = seenWait}) async {
  final id = deviceId.toUpperCase();
  return await _scanUntil((d) => d.deviceId.toUpperCase() == id, wait) != null;
}

/// How long a round of the search for the paired node scans: four of
/// its advertising intervals of 1022.5 ms (spec "Phone link", Finding the
/// node). The scan listens without a break ([AndroidScanMode.lowLatency]),
/// so with the screen on it hears nearly every advertisement in reach;
/// the extra intervals cover a scan's start and an advertisement lost.
/// With the screen off Android listens less, and a later round makes up
/// for a miss.
const Duration lookScan = Duration(microseconds: 4 * 1022500);

/// How often a round of the search starts. Android refuses an app's
/// sixth scan started within 30 s, silently (its Bluetooth service,
/// since Android 7); a round every 15 s takes two of the five and leaves
/// three for the person's own Scan and Pair again.
const Duration lookRound = Duration(seconds: 15);

/// Scans for the node called `name` for [lookScan], or until `stop`
/// completes; the node, or null if it was not seen.
Future<FoundNode?> look(String name, {Future<void>? stop}) async {
  final d = await _scanUntil(
    (d) => d.name == name,
    lookScan,
    mode: AndroidScanMode.lowLatency,
    stop: stop,
  );
  return d == null ? null : FoundNode(d.deviceId, name, d.rssi);
}

/// Scans until a device `match`es, at most `wait` or until `stop`
/// completes; the device, or null.
Future<BleDevice?> _scanUntil(
  bool Function(BleDevice) match,
  Duration wait, {
  AndroidScanMode? mode,
  Future<void>? stop,
}) async {
  final found = Completer<BleDevice?>();
  void end(BleDevice? d) {
    if (!found.isCompleted) found.complete(d);
  }

  final sub = UniversalBle.scanStream.listen((d) {
    if (match(d)) end(d);
  });
  final timer = Timer(wait, () => end(null));
  stop?.then((_) => end(null));
  try {
    await _scanForNodes(mode: mode);
    return await found.future;
  } finally {
    timer.cancel();
    unawaited(sub.cancel());
    await UniversalBle.stopScan();
  }
}

/// How long the app waits for a pairing to end. The plugin's commands
/// time out after 10 s by default, and a person typing the code takes
/// longer (17 and 20 s on the bench, 2026-11-06), so the pairing gets its
/// own bound. Its true ends are Android's: the bond made, or none when
/// the person cancels or the pairing fails, at the latest when the
/// Security Manager's 30 s timer between its messages runs out (Bluetooth
/// Core Specification 5.0, Vol 3 Part H §3.4), so the bond state comes
/// within some 30 s of the code being shown. Four times that only keeps
/// the app from waiting for ever on a report that never comes.
const Duration pairWait = Duration(seconds: 4 * 30);

/// The node `node` was not seen in a scan.
class NotSeen implements Exception {
  final String node;
  const NotSeen(this.node);
  @override
  String toString() =>
      '$node was not seen: move nearer to it and connect again from the '
      'scan';
}

Future<void> stopBleScan() async {
  UniversalBle.onScanResult = null;
  await UniversalBle.stopScan();
}

/// The ATT error in a failed write or read, as Android reports it.
int? _attCode(Object e) =>
    e is UniversalBleException ? int.tryParse('${e.details}') : null;

/// How a bonded phone's read fails when the node holds no bond for it:
/// insufficient authentication 0x05 or insufficient encryption 0x0F
/// (Bluetooth Core Specification 5.0, Vol 3 Part F §3.4.1.1), or Android's
/// own 0x89, its encryption with the phone's key failed.
const _notBonded = {0x05, 0x0F, 0x89};

/// Whether a bonded phone's first read failed because the node no longer
/// holds its bond: an error of [_notBonded], or the link closed under it.
bool bondRefused(Object e, {required bool closed}) {
  final code = _attCode(e);
  return closed || (code != null && _notBonded.contains(code));
}

/// The Generic Attribute service and its Service Changed characteristic
/// (Bluetooth Core Specification 5.0, Vol 3 Part G §7.1).
final _gattService = BleUuidParser.string('1801');
final _serviceChanged = BleUuidParser.string('2a05');

/// Whether `services` hold Service Changed, by which the node tells a
/// bonded phone that its table changed (spec "Phone link", Changes to
/// the table).
bool hasServiceChanged(List<BleService> services) => services.any(
  (s) =>
      s.uuid == _gattService &&
      s.characteristics.any((c) => c.uuid == _serviceChanged),
);

/// Discovers the node's services. Android keeps a bonded node's table
/// from one connection to the next, and one kept without Service Changed
/// is never told of a change, so its handles may name other attributes
/// now: a node's table from before link version 5, which Android kept on
/// through the update that added Service Changed and moved every handle
/// of the service. Such a table is dropped and discovered again; for a
/// node still on an older version that costs a discovery at each
/// connection.
Future<void> discover(String deviceId) async {
  final services = await UniversalBle.discoverServices(deviceId);
  if (hasServiceChanged(services)) return;
  await UniversalBle.discoverServices(
    deviceId,
    platformConfig: DiscoverServicesPlatformConfig(
      android: AndroidDiscoverServicesOptions(clearGattCache: true),
    ),
  );
}

class BleLink implements NodeLink {
  final String deviceId;
  final _closed = Completer<void>();
  StreamSubscription<bool>? _connection;

  BleLink._(this.deviceId);

  /// Connects, asks for the MTU of [attMtu], and pairs if the phone has
  /// no bond with the node, telling `pairing` first, as Android then asks
  /// the person for the code on the node's screen, and `paired` once the
  /// bond is made; returns the MTU agreed.
  ///
  /// Throws [NotOurs] for a node, `name`, that no longer holds the
  /// phone's bond: one that ends the link it took before the app has
  /// read it while the phone holds a bond for it, as the node does when
  /// the phone's key is not its bond's ([setUpRefused]); one whose first
  /// read it refuses ([_probe]); or one whose bond the phone no longer
  /// has though it `hadBond`, as Android may drop a bond whose key the
  /// node refused. Throws [NotPaired] for a pairing that did not
  /// complete.
  static Future<(BleLink, int)> connect(
    String deviceId, {
    required String name,
    bool hadBond = false,
    void Function()? pairing,
    void Function()? paired,
  }) async {
    final link = BleLink._(deviceId);
    await UniversalBle.connect(
      deviceId,
      // The node takes one connection; one left open by a killed app
      // would keep the owner out until the supervision timeout.
      platformConfig: ConnectionPlatformConfig(
        android: AndroidConnectionOptions(closeGattOnDetach: true),
      ),
    );
    link._connection = UniversalBle.connectionStream(deviceId).listen((up) {
      if (!up && !link._closed.isCompleted) link._closed.complete();
    });
    final int mtu;
    try {
      await discover(deviceId);
      mtu = await UniversalBle.requestMtu(deviceId, attMtu);
    } catch (e) {
      await link.close();
      if (await setUpRefused(deviceId, hadBond: hadBond)) throw NotOurs(name);
      rethrow;
    }
    try {
      if (await UniversalBle.isPaired(deviceId) ?? false) {
        await link._probe(name);
      } else if (hadBond) {
        throw NotOurs(name);
      } else {
        pairing?.call();
        await UniversalBle.pair(deviceId, timeout: pairWait);
        paired?.call();
      }
    } on PairingException {
      await link.close();
      throw const NotPaired();
    } catch (_) {
      await link.close();
      rethrow;
    }
    return (link, mtu);
  }

  /// Forgets the phone's bond with the node, so that the next connect
  /// pairs anew. Android removes a bond a moment after it is asked to;
  /// this waits for that, by [_forgetStep], at most [_forgetWait]. With
  /// the bond goes what Android knew of the address's type: the node is
  /// to be [seen] again before the connect.
  static Future<void> forget(String deviceId) async {
    await UniversalBle.unpair(deviceId);
    final end = DateTime.now().add(_forgetWait);
    while (await UniversalBle.isPaired(deviceId) ?? false) {
      if (DateTime.now().isAfter(end)) break;
      await Future<void>.delayed(_forgetStep);
    }
  }

  /// Far longer than the bond's removal takes, and short beside the
  /// person's wait for the pairing dialog.
  static const _forgetWait = Duration(seconds: 2);
  static const _forgetStep = Duration(milliseconds: 100);

  /// Whether a link that came up and failed before the app read the node
  /// failed for the phone's bond: the phone holds one for the node, by
  /// Android's word or `hadBond`. A bonded Android phone encrypts the link
  /// with its key as soon as it connects; a node that holds another bond
  /// ends the link at once, authentication failure, which Android reports
  /// as the connection's status 5 and fails the MTU request or the
  /// discovery under way with 133, its catch-all GATT error (bench,
  /// 2026-11-06). A link that never came up fails in
  /// [UniversalBle.connect] instead and is a plain failure to connect.
  static Future<bool> setUpRefused(
    String deviceId, {
    required bool hadBond,
  }) async {
    if (hadBond) return true;
    try {
      return await UniversalBle.isPaired(deviceId) ?? false;
    } catch (_) {
      return false;
    }
  }

  /// A bonded phone's first read, which shows whether the node still
  /// holds its bond.
  Future<void> _probe(String name) async {
    try {
      await _read(Uuids.node);
    } catch (e) {
      if (bondRefused(e, closed: _closed.isCompleted)) throw NotOurs(name);
      rethrow;
    }
  }

  Future<Uint8List> _read(String c) =>
      UniversalBle.read(deviceId, Uuids.service, c, timeout: attTimeout);

  Future<void> _write(String c, Uint8List v) async {
    try {
      await UniversalBle.write(
        deviceId,
        Uuids.service,
        c,
        v,
        timeout: attTimeout,
      );
    } catch (e) {
      final code = _attCode(e);
      if (code != null) throw AttException(code);
      rethrow;
    }
  }

  Stream<Uint8List> _notified(String c) =>
      UniversalBle.characteristicValueStream(deviceId, c);

  @override
  Future<NodeInfo> readNode() async => NodeInfo.decode(await _read(Uuids.node));

  @override
  Future<void> subscribe() async {
    await UniversalBle.subscribeNotifications(
      deviceId,
      Uuids.service,
      Uuids.status,
    );
    await UniversalBle.subscribeNotifications(
      deviceId,
      Uuids.service,
      Uuids.givenBack,
    );
  }

  @override
  Future<Status> readStatus() async => Status.decode(await _read(Uuids.status));

  @override
  Future<GivenBack?> readGivenBack() async =>
      GivenBack.decode(await _read(Uuids.givenBack));

  @override
  Future<InboxMessage?> readInbox() async =>
      InboxMessage.decode(await _read(Uuids.inbox));

  @override
  Future<void> confirm(int id) => _write(Uuids.inbox, encodeConfirm(id));

  @override
  Future<void> send(Uint8List value) => _write(Uuids.send, value);

  @override
  Future<void> setMapPoint(Position? p) =>
      _write(Uuids.mapPoint, encodeMapPoint(p));

  @override
  Future<void> setClock(int utcS, int offsetMin) =>
      _write(Uuids.clock, encodeClock(utcS, offsetMin));

  @override
  Future<NodeTime> readClock() async =>
      NodeTime.decode(await _read(Uuids.clock));

  @override
  Future<void> setHome(List<int>? key) => _write(Uuids.home, encodeHome(key));

  @override
  Future<NodeHome> readHome() async => NodeHome.decode(await _read(Uuids.home));

  @override
  Future<void> setNeighboursFrom(int slot) =>
      _write(Uuids.neighbours, encodeNeighboursFrom(slot));

  @override
  Future<NeighbourPage> readNeighbours() async =>
      NeighbourPage.decode(await _read(Uuids.neighbours));

  @override
  Future<UpdateProgress> readUpdate() async =>
      UpdateProgress.decode(await _read(Uuids.update));

  /// A begin is longer than one write at the node's MTU, and Android
  /// writes it as a long write (spec "Phone link", MTU).
  @override
  Future<void> writeUpdate(Uint8List value) => _write(Uuids.update, value);

  @override
  Future<void> writePiece(Uint8List value) => UniversalBle.write(
    deviceId,
    Uuids.service,
    Uuids.image,
    value,
    withoutResponse: true,
    timeout: attTimeout,
  );

  /// Android's high priority, an interval of 11.25 to 15 ms (spec
  /// "Phone link", Updates); other platforms choose their own.
  @override
  Future<void> hurry() async {
    try {
      await UniversalBle.requestConnectionPriority(
        deviceId,
        BleConnectionPriority.highPerformance,
      );
    } on UniversalBleException {
      // Not supported here: the transfer takes longer.
    }
  }

  @override
  Stream<Status> get statusNotified =>
      _notified(Uuids.status).map(Status.decode);

  @override
  Stream<GivenBack?> get givenBackNotified =>
      _notified(Uuids.givenBack).map(GivenBack.decode);

  @override
  Future<void> get closed => _closed.future;

  @override
  Future<void> close() async {
    unawaited(_connection?.cancel());
    try {
      await UniversalBle.disconnect(deviceId);
    } finally {
      if (!_closed.isCompleted) _closed.complete();
    }
  }
}
