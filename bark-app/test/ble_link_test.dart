// The Bluetooth link's set-up against a platform that plays the node:
// a node that ends the link before the app has read it is no longer
// this phone's when the phone holds a bond for it, and a plain failure
// when it does not.

import 'dart:typed_data';

import 'package:bark_app/ble_link.dart';
import 'package:bark_app/node_link.dart';
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
}
