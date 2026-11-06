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
  await UniversalBle.startScan(
    scanFilter: ScanFilter(withServices: [Uuids.service]),
    // The node advertises with legacy advertising PDUs (spec "Phone link").
    platformConfig: PlatformConfig(android: AndroidOptions(legacy: true)),
  );
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

class BleLink implements NodeLink {
  final String deviceId;
  final _closed = Completer<void>();
  StreamSubscription<bool>? _connection;

  BleLink._(this.deviceId);

  /// Connects, asks for the MTU of [attMtu], and pairs if the phone has
  /// no bond with the node, telling `pairing` first, as Android then asks
  /// the person for the code on the node's screen, and `paired` once the
  /// bond is made; returns the MTU
  /// agreed. Throws [NotOurs] for a node, `name`, that no longer holds
  /// the phone's bond, or whose bond the phone no longer has though it
  /// `hadBond`, as Android may drop a bond whose key the node refused;
  /// and [NotPaired] for a pairing that did not complete.
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
    await UniversalBle.discoverServices(deviceId);
    final mtu = await UniversalBle.requestMtu(deviceId, attMtu);
    try {
      if (await UniversalBle.isPaired(deviceId) ?? false) {
        await link._probe(name);
      } else if (hadBond) {
        throw NotOurs(name);
      } else {
        pairing?.call();
        await UniversalBle.pair(deviceId);
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
  /// this waits for that, by [_forgetStep], at most [_forgetWait].
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
  Stream<Status> get statusNotified =>
      _notified(Uuids.status).map(Status.decode);

  @override
  Stream<GivenBack?> get givenBackNotified =>
      _notified(Uuids.givenBack).map(GivenBack.decode);

  @override
  Future<void> get closed => _closed.future;

  @override
  Future<void> close() async {
    await _connection?.cancel();
    try {
      await UniversalBle.disconnect(deviceId);
    } finally {
      if (!_closed.isCompleted) _closed.complete();
    }
  }
}
