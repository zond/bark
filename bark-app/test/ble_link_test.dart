// The Bluetooth link's set-up against a platform that plays the node:
// a node that ends the link before the app has read it is no longer
// this phone's when the phone holds a bond for it, and a plain failure
// when it does not; and a node Android can reach only once a scan has
// seen it.

import 'dart:async';
import 'dart:typed_data';

import 'package:bark_app/ble_link.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/node_link.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:universal_ble/universal_ble.dart';

/// Connects, then fails the MTU request with 133 and drops the link, as
/// a bonded Android phone saw it from a node that held another bond.
class RefusingNode extends UniversalBlePlatform {
  /// Whether Android lists a bond with the node.
  final bool bonded;
  RefusingNode({required this.bonded});

  @override
  Future<void> connect(
    String deviceId, {
    Duration? connectionTimeout,
    bool autoConnect = false,
    ConnectionPlatformConfig? platformConfig,
  }) async => updateConnection(deviceId, true);

  @override
  Future<void> disconnect(String deviceId) async =>
      updateConnection(deviceId, false);

  @override
  Future<BleConnectionState> getConnectionState(String deviceId) async =>
      BleConnectionState.disconnected;

  @override
  Future<List<BleService>> discoverServices(
    String deviceId,
    bool withDescriptors, {
    DiscoverServicesPlatformConfig? platformConfig,
  }) async => [];

  @override
  Future<int> requestMtu(String deviceId, int expectedMtu) async {
    updateConnection(deviceId, false, 'status 5', null, 5);
    throw UniversalBleException(
      code: UniversalBleErrorCode.failed,
      message: 'Failed to change MTU',
      details: '133',
    );
  }

  @override
  Future<bool> isPaired(String deviceId) async => bonded;

  @override
  Future<Uint8List> readValue(
    String deviceId,
    String service,
    String characteristic, {
    Duration? timeout,
  }) => throw UnimplementedError();

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnimplementedError('${invocation.memberName}');
}

/// A node Android connects to only once a scan has seen it, as with a
/// random static address and no bond; `advertises` false for one out of
/// range.
class HiddenNode extends UniversalBlePlatform {
  final String id;
  final bool advertises;

  /// How long the person takes to type the code.
  final Duration typing;
  bool scanned = false;
  HiddenNode(this.id, {this.advertises = true, this.typing = Duration.zero});

  @override
  Future<void> startScan({
    ScanFilter? scanFilter,
    PlatformConfig? platformConfig,
  }) async {
    if (!advertises) return;
    Timer(const Duration(milliseconds: 10), () {
      scanned = true;
      updateScanResult(BleDevice(deviceId: id, name: 'bark-7572482fc367'));
    });
  }

  @override
  Future<void> stopScan() async {}

  @override
  Future<void> connect(
    String deviceId, {
    Duration? connectionTimeout,
    bool autoConnect = false,
    ConnectionPlatformConfig? platformConfig,
  }) async {
    if (!scanned) throw ConnectionException('timed out after 30 seconds');
    updateConnection(deviceId, true);
  }

  @override
  Future<void> disconnect(String deviceId) async =>
      updateConnection(deviceId, false);

  @override
  Future<BleConnectionState> getConnectionState(String deviceId) async =>
      BleConnectionState.connected;

  @override
  Future<List<BleService>> discoverServices(
    String deviceId,
    bool withDescriptors, {
    DiscoverServicesPlatformConfig? platformConfig,
  }) async => [];

  @override
  Future<int> requestMtu(String deviceId, int expectedMtu) async => expectedMtu;

  @override
  Future<bool> isPaired(String deviceId) async => false;

  @override
  Future<bool> pair(String deviceId) => Future.delayed(typing, () => true);

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnimplementedError('${invocation.memberName}');
}

void main() {
  const device = '00:11:22:33:44:55';

  test('a bonded phone the node drops at once: no longer ours', () async {
    UniversalBle.setInstance(RefusingNode(bonded: true));
    await expectLater(
      BleLink.connect(device, name: 'bark-7572482fc367'),
      throwsA(
        isA<NotOurs>().having((e) => e.node, 'node', 'bark-7572482fc367'),
      ),
    );
  });

  test('the same drop on a phone without a bond fails to connect', () async {
    UniversalBle.setInstance(RefusingNode(bonded: false));
    await expectLater(
      BleLink.connect(device, name: 'bark-7572482fc367'),
      throwsA(isA<UniversalBleException>()),
    );
  });

  test('a bond the phone lost but the app had: no longer ours', () async {
    UniversalBle.setInstance(RefusingNode(bonded: false));
    await expectLater(
      BleLink.connect(device, name: 'bark-7572482fc367', hadBond: true),
      throwsA(isA<NotOurs>()),
    );
  });

  test('without a scan the connect fails; once seen, it pairs', () async {
    final node = HiddenNode(device);
    UniversalBle.setInstance(node);
    await expectLater(
      BleLink.connect(device, name: 'bark-7572482fc367'),
      throwsA(isA<ConnectionException>()),
    );
    expect(await seen(device.toLowerCase()), isTrue);
    final (link, mtu) = await BleLink.connect(
      device,
      name: 'bark-7572482fc367',
    );
    expect(mtu, attMtu);
    await link.close();
  });

  test('a node not seen within the wait', () async {
    UniversalBle.setInstance(HiddenNode(device, advertises: false));
    expect(await seen(device, wait: const Duration(milliseconds: 50)), isFalse);
  });

  test('a pairing longer than the plugin\'s 10 s is waited for', () {
    fakeAsync((async) {
      final node = HiddenNode(device, typing: const Duration(seconds: 25))
        ..scanned = true;
      UniversalBle.setInstance(node);
      int? mtu;
      Object? failed;
      BleLink.connect(
        device,
        name: 'bark-7572482fc367',
      ).then<void>((r) => mtu = r.$2, onError: (Object e) => failed = e);
      async.elapse(const Duration(seconds: 26));
      expect(failed, isNull);
      expect(mtu, attMtu);
    });
  });
}
