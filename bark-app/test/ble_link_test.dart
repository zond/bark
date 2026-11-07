// The Bluetooth link's set-up against a platform that plays the node:
// a node that ends the link before the app has read it is no longer
// this phone's when the phone holds a bond for it, and a plain failure
// when it does not; a node Android can reach only once a scan has seen
// it; and the app's search for its paired node, which connects when it
// sees the node, at start and after a drop, but not after the person
// disconnected, not to a node no longer theirs, and gives way to the
// person's tap on another node.

import 'dart:async';
import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/ble_link.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/node_link.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/common.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';
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

/// The node's table as discovered: the Generic Attribute service with
/// Service Changed, and no other.
final _withServiceChanged = [
  BleService('1801', [BleCharacteristic('2a05', [], [])]),
];

/// A node Android connects to only once a scan has seen it, as with a
/// random static address and no bond; `advertises` false for one out of
/// range.
class HiddenNode extends UniversalBlePlatform {
  final String id;
  final bool advertises;

  /// Whether Android's kept table of the node is one from before
  /// Service Changed, which only a discovery without the cache replaces.
  final bool keptOld;

  /// Each discovery's options, in order.
  final List<DiscoverServicesPlatformConfig?> discoveries = [];

  /// How long the person takes to type the code.
  final Duration typing;
  bool scanned = false;
  HiddenNode(
    this.id, {
    this.advertises = true,
    this.typing = Duration.zero,
    this.keptOld = false,
  });

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
  }) async {
    discoveries.add(platformConfig);
    final fresh = platformConfig?.android?.clearGattCache ?? false;
    return keptOld && !fresh ? [] : _withServiceChanged;
  }

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

/// Fake nodes behind Bluetooth, by device ID: each advertises while in
/// reach and not connected, and answers reads and writes as its fake.
class FakeNodes extends UniversalBlePlatform {
  final Map<String, FakeNode> nodes;

  /// The devices whose bond the phone holds.
  final Set<String> bonds;
  final Set<String> inReach;
  final Set<String> _up = {};

  /// Each scan's start, and each connect's device, in order.
  int scans = 0;
  final List<String> connects = [];

  FakeNodes(this.nodes, {required this.bonds}) : inReach = {...nodes.keys};

  /// The link to `id` drops, from the node's side.
  void drop(String id) {
    _up.remove(id);
    nodes[id]!.close();
    updateConnection(id, false);
  }

  @override
  Future<AvailabilityState> getBluetoothAvailabilityState() async =>
      AvailabilityState.poweredOn;

  @override
  Future<void> startScan({
    ScanFilter? scanFilter,
    PlatformConfig? platformConfig,
  }) async {
    scans++;
    Timer(const Duration(milliseconds: 10), () {
      for (final id in inReach.difference(_up)) {
        updateScanResult(BleDevice(deviceId: id, name: _name(nodes[id]!)));
      }
    });
  }

  String _name(FakeNode n) =>
      n is OtherNode ? nodeName(OtherNode.otherId) : nodeName(FakeNode.id);

  @override
  Future<void> stopScan() async {}

  @override
  Future<void> connect(
    String deviceId, {
    Duration? connectionTimeout,
    bool autoConnect = false,
    ConnectionPlatformConfig? platformConfig,
  }) async {
    connects.add(deviceId);
    if (!inReach.contains(deviceId)) {
      throw ConnectionException('timed out after 30 seconds');
    }
    _up.add(deviceId);
    updateConnection(deviceId, true);
  }

  @override
  Future<void> disconnect(String deviceId) async {
    _up.remove(deviceId);
    await nodes[deviceId]!.close();
    updateConnection(deviceId, false);
  }

  @override
  Future<BleConnectionState> getConnectionState(String deviceId) async =>
      _up.contains(deviceId)
      ? BleConnectionState.connected
      : BleConnectionState.disconnected;

  @override
  Future<List<BleService>> discoverServices(
    String deviceId,
    bool withDescriptors, {
    DiscoverServicesPlatformConfig? platformConfig,
  }) async => [];

  /// A node that holds another phone's bond ends a bonded phone's link
  /// at once, as [RefusingNode] does; otherwise the link is the fake's.
  @override
  Future<int> requestMtu(String deviceId, int expectedMtu) async {
    final n = nodes[deviceId]!;
    if (bonds.contains(deviceId) && !n.paired) {
      _up.remove(deviceId);
      updateConnection(deviceId, false, 'status 5', null, 5);
      throw UniversalBleException(
        code: UniversalBleErrorCode.failed,
        message: 'Failed to change MTU',
        details: '133',
      );
    }
    n.connect();
    return expectedMtu;
  }

  @override
  Future<bool> isPaired(String deviceId) async => bonds.contains(deviceId);

  @override
  Future<bool> pair(String deviceId) async {
    bonds.add(deviceId);
    nodes[deviceId]!.pair();
    return true;
  }

  @override
  Future<void> setNotifiable(
    String deviceId,
    String service,
    String characteristic,
    BleInputProperty bleInputProperty,
  ) async {}

  @override
  Future<Uint8List> readValue(
    String deviceId,
    String service,
    String characteristic, {
    Duration? timeout,
  }) async {
    final n = nodes[deviceId]!;
    return switch (characteristic) {
      Uuids.node => (await n.readNode()).encode(),
      Uuids.status => (await n.readStatus()).encode(),
      Uuids.givenBack => (await n.readGivenBack())?.encode() ?? Uint8List(0),
      Uuids.inbox => (await n.readInbox())?.encode() ?? Uint8List(0),
      Uuids.clock => (await n.readClock()).encode(),
      Uuids.home => (await n.readHome()).encode(),
      _ => throw UnimplementedError(characteristic),
    };
  }

  /// Writes the app makes at every connection; the clock and the home
  /// the fake takes are not what these tests are about.
  @override
  Future<void> writeValue(
    String deviceId,
    String service,
    String characteristic,
    Uint8List value,
    BleOutputProperty bleOutputProperty,
  ) async {
    final n = nodes[deviceId]!;
    if (characteristic == Uuids.inbox) {
      await n.confirm(ByteData.sublistView(value).getUint32(0));
    }
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnimplementedError('${invocation.memberName}');
}

/// Another node, `bark-0ce400000002`.
class OtherNode extends FakeNode {
  static final otherId = Uint8List.fromList([0x0c, 0xe4, 0, 0, 0, 2]);
  OtherNode({required super.now, required super.sodium, required super.app});
  @override
  Future<NodeInfo> readNode() async => NodeInfo(linkVersion, otherId, 1);
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

  test(
    'a table kept from before Service Changed is discovered again',
    () async {
      final node = HiddenNode(device, keptOld: true)..scanned = true;
      UniversalBle.setInstance(node);
      final (link, _) = await BleLink.connect(
        device,
        name: 'bark-7572482fc367',
      );
      expect(node.discoveries.map((c) => c?.android?.clearGattCache), [
        null,
        true,
      ]);
      await link.close();
    },
  );

  test('a table with Service Changed is kept', () async {
    final node = HiddenNode(device)..scanned = true;
    UniversalBle.setInstance(node);
    final (link, _) = await BleLink.connect(device, name: 'bark-7572482fc367');
    expect(node.discoveries, hasLength(1));
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

  group('the search for the paired node', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());
    const paired = 'aa:bb:cc:dd:ee:01', other = 'aa:bb:cc:dd:ee:02';
    final start = DateTime.utc(2026, 11, 6);

    /// An app paired with the fake node at `paired`, in reach or not,
    /// with `other` in reach, started as on the phone.
    void run(
      void Function(FakeAsync, AppModel, FakeNodes) body, {
      bool pairedInReach = true,
    }) {
      fakeAsync((async) {
        DateTime now() => start.add(async.elapsed);
        final app = AppModel(
          sodium: sodium,
          me: Me(MemoryStore(), sodium, now),
          session: Session(store: MemoryStore(), now: now),
          contacts: Contacts(MemoryStore()),
          now: now,
        );
        app.load();
        async.flushMicrotasks();
        final id = app.me.identity;
        final ble = FakeNodes(
          {
            paired: FakeNode(now: now, sodium: sodium, app: id),
            other: OtherNode(now: now, sodium: sodium, app: id),
          },
          bonds: {paired},
        );
        if (!pairedInReach) ble.inReach.remove(paired);
        UniversalBle.setInstance(ble);
        app.session.setPaired(nodeName(FakeNode.id));
        async.flushMicrotasks();
        app.startSearch();
        body(async, app, ble);
        app.dispose();
        for (final n in ble.nodes.values) {
          n.dispose();
        }
        async.flushMicrotasks();
      });
    }

    test('at start it connects once the node is seen', () {
      run(pairedInReach: false, (async, app, ble) {
        async.elapse(const Duration(seconds: 1));
        expect(app.lookingFor, 'bark-fa4e00000001');
        expect(lookingLine(app), 'Looking for bark-fa4e00000001…');
        async.elapse(const Duration(minutes: 1));
        expect(app.session.connected, isFalse);
        // A round every 15 s.
        expect(ble.scans, 5);
        ble.inReach.add(paired);
        async.elapse(lookRound);
        expect(app.session.connected, isTrue);
        expect(app.linkDevice, paired);
        expect(app.lookingFor, isNull);
        final scans = ble.scans;
        async.elapse(const Duration(minutes: 1));
        expect(ble.scans, scans);
      });
    });

    test('after a drop it connects again', () {
      run((async, app, ble) {
        async.elapse(const Duration(seconds: 1));
        expect(app.session.connected, isTrue);
        ble.drop(paired);
        async.elapse(const Duration(seconds: 1));
        expect(app.session.connected, isTrue);
        expect(ble.connects, [paired, paired]);
      });
    });

    test('after the person disconnects it does not', () {
      run((async, app, ble) {
        async.elapse(const Duration(seconds: 1));
        app.disconnect();
        async.elapse(const Duration(seconds: 1));
        final scans = ble.scans;
        async.elapse(const Duration(minutes: 5));
        expect(app.session.connected, isFalse);
        expect(app.lookingFor, isNull);
        expect(ble.scans, scans);
      });
    });

    test('a node no longer this phone\'s is not tried again', () {
      run((async, app, ble) {
        async.elapse(const Duration(seconds: 1));
        ble.nodes[paired]!.pairElsewhere();
        ble.drop(paired);
        async.elapse(const Duration(seconds: 1));
        expect(app.session.pairingLost, isTrue);
        expect(app.pairAgain, isNotNull);
        final connects = ble.connects.length;
        final scans = ble.scans;
        async.elapse(const Duration(minutes: 5));
        expect(app.session.connected, isFalse);
        expect(ble.connects.length, connects);
        expect(ble.scans, scans);
      });
    });

    test('a tap on another node ends the search', () {
      run(pairedInReach: false, (async, app, ble) {
        // A round's scan is under way.
        async.elapse(const Duration(seconds: 1));
        expect(app.lookingFor, 'bark-fa4e00000001');
        app.connect(FoundNode(other, nodeName(OtherNode.otherId), -60));
        async.elapse(const Duration(seconds: 1));
        expect(app.session.connected, isTrue);
        expect(app.linkDevice, other);
        expect(app.session.pairedWith, 'bark-0ce400000002');
        ble.inReach.add(paired);
        async.elapse(const Duration(minutes: 1));
        expect(app.linkDevice, other);
        expect(ble.connects, [other]);
      });
    });
  });
}
