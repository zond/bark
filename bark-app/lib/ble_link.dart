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

class BleLink implements NodeLink {
  final String deviceId;
  final _closed = Completer<void>();
  StreamSubscription<bool>? _connection;

  BleLink._(this.deviceId);

  /// Connects and asks for the MTU of [attMtu]; returns the MTU agreed.
  static Future<(BleLink, int)> connect(String deviceId) async {
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
    return (link, mtu);
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
