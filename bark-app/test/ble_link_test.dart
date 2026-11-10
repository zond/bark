// The Bluetooth link's set-up against a platform that plays the node:
// a node that ends the link before the app has read it is no longer
// this phone's when the phone holds a bond for it, and a plain failure
// when it does not; a node Android can reach only once a scan has seen
// it; and the app's search for its paired node, which connects when it
// sees the node, at start, after a drop and after Bluetooth was off, an
// update going on from what the node holds, but not after the person
// disconnected, not to a node no longer theirs, and gives way to the
// person's tap on another node; and with no paired node, or while it
// is not heard, the strongest public node, joined as a guest, but not
// one that refuses the phone's old bond until the bond is forgotten; and
// the owner's unpairing, after which Android forgets its bond too.

import 'dart:async';
import 'dart:typed_data';

import 'package:bark_app/app_layer.dart' show Form, formatByte, recipientTag;
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
import 'package:bark_app/updater.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';
import 'package:universal_ble/universal_ble.dart';

import 'update_test.dart' show imageFile;

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
/// reach and not connected, a public one by the public node's UUID, and
/// answers reads and writes as its fake: as its owner's link to a phone
/// that holds its bond or to a node that is not public, else as a
/// guest's.
class FakeNodes extends UniversalBlePlatform {
  final Map<String, FakeNode> nodes;

  /// The devices whose bond the phone holds.
  final Set<String> bonds;
  final Set<String> inReach;
  final Set<String> _up = {};
  final Map<String, NodeLink> _links = {};

  /// The RSSI each device is heard with, dBm.
  final Map<String, int> rssi = {};

  /// Each scan's start, and each connect's device, in order.
  int scans = 0;
  final List<String> connects = [];

  FakeNodes(this.nodes, {required this.bonds}) : inReach = {...nodes.keys};

  /// The link to `id` drops, from the node's side.
  void drop(String id) {
    _up.remove(id);
    _links.remove(id)?.close();
    updateConnection(id, false);
  }

  /// The link the phone has to `id`.
  NodeLink? linkTo(String id) => _links[id];

  /// Bluetooth is on.
  bool on = true;

  /// Android lets the app use Bluetooth; the person's answer when asked.
  bool allowed = true;
  bool allows = true;
  int asked = 0;

  @override
  Future<bool> hasPermissions({bool withAndroidFineLocation = false}) async =>
      allowed;

  @override
  Future<void> requestPermissions({
    bool withAndroidFineLocation = false,
  }) async {
    asked++;
    allowed = allows;
  }

  /// The person switches Bluetooth off or on. Off, every link is gone,
  /// and Android tells the app of the adapter only, not of the links: a
  /// read or write then finds no characteristic (bench, 2026-11-07).
  void power(bool v) {
    on = v;
    if (!v) {
      for (final id in _up) {
        _links.remove(id)?.close();
      }
      _up.clear();
    }
    updateAvailability(
      v ? AvailabilityState.poweredOn : AvailabilityState.poweredOff,
    );
  }

  /// For a test: Bluetooth goes off once this many pieces of an image
  /// are written.
  int? offAfterPieces;

  void _reach(String deviceId) {
    if (!_up.contains(deviceId)) {
      throw UniversalBleException(
        code: UniversalBleErrorCode.characteristicNotFound,
        message: 'Characteristic not found',
      );
    }
  }

  @override
  Future<AvailabilityState> getBluetoothAvailabilityState() async =>
      on ? AvailabilityState.poweredOn : AvailabilityState.poweredOff;

  @override
  Future<void> startScan({
    ScanFilter? scanFilter,
    PlatformConfig? platformConfig,
  }) async {
    if (!on) {
      throw UniversalBleException(
        code: UniversalBleErrorCode.bluetoothNotEnabled,
        message: 'Bluetooth is off',
      );
    }
    scans++;
    Timer(const Duration(milliseconds: 10), () {
      for (final id in inReach.difference(_up)) {
        final n = nodes[id]!;
        updateScanResult(
          BleDevice(
            deviceId: id,
            name: nodeName(n.nodeId),
            rssi: rssi[id],
            services: [n.public ? Uuids.publicNode : Uuids.service],
          ),
        );
      }
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
    connects.add(deviceId);
    if (!on || !inReach.contains(deviceId)) {
      throw ConnectionException('timed out after 30 seconds');
    }
    _up.add(deviceId);
    updateConnection(deviceId, true);
  }

  @override
  Future<void> disconnect(String deviceId) async {
    _up.remove(deviceId);
    await _links.remove(deviceId)?.close();
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
    final link = bonds.contains(deviceId) || !n.public
        ? n.connect()
        : n.connectGuest();
    _links[deviceId] = link;
    // A guest's link the node ends, at its 30 s or for another guest,
    // ends for Android too.
    if (link is FakeGuest) {
      link.closed.then((_) {
        if (_links[deviceId] != link) return;
        _links.remove(deviceId);
        _up.remove(deviceId);
        updateConnection(deviceId, false);
      });
    }
    return expectedMtu;
  }

  @override
  Future<bool> isPaired(String deviceId) async => bonds.contains(deviceId);

  /// The node ends the link at once after the unpairing write, and
  /// Android reports the write as failed.
  bool dropsOnUnpair = false;

  /// The node refuses the unpairing write.
  bool refusesUnpair = false;

  /// Android keeps the bonds the app asks it to remove, as a phone whose
  /// hidden removal fails does.
  bool keepsBonds = false;

  @override
  Future<void> unpair(String deviceId) async {
    if (!keepsBonds) bonds.remove(deviceId);
  }

  /// The pairings Android was asked for, each a code typed.
  int pairings = 0;

  /// The person types a code that is not the one on the node's screen.
  bool wrongCode = false;

  /// Pairs, which makes a guest's link the owner's.
  @override
  Future<bool> pair(String deviceId) async {
    pairings++;
    if (wrongCode) return false;
    bonds.add(deviceId);
    final n = nodes[deviceId]!..pair();
    final guest = _links[deviceId];
    if (guest is FakeGuest) {
      // The owner's link first, so that the guest's end is not the link's.
      _links[deviceId] = n.connect();
      await guest.close();
    }
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
    _reach(deviceId);
    final n = _links[deviceId]!;
    return switch (characteristic) {
      Uuids.node => (await n.readNode()).encode(),
      Uuids.status => (await n.readStatus()).encode(),
      Uuids.givenBack => (await n.readGivenBack())?.encode() ?? Uint8List(0),
      // A guest's Inbox gives its items in the Guest form.
      Uuids.inbox when n is FakeGuest =>
        (await n.readGuest())?.encode() ?? Uint8List(0),
      Uuids.inbox => (await n.readInbox())?.encode() ?? Uint8List(0),
      Uuids.clock => (await n.readClock()).encode(),
      Uuids.home => (await n.readHome()).encode(),
      Uuids.update => (await n.readUpdate()).encode(),
      Uuids.public => encodePublic(await n.readPublic()),
      _ => throw UnimplementedError(characteristic),
    };
  }

  /// Writes the app makes at every connection, the home's, and an
  /// update's; the clock the fake takes is not what these tests are
  /// about.
  @override
  Future<void> writeValue(
    String deviceId,
    String service,
    String characteristic,
    Uint8List value,
    BleOutputProperty bleOutputProperty,
  ) async {
    _reach(deviceId);
    final n = _links[deviceId]!;
    switch (characteristic) {
      case Uuids.recipient:
        await n.setRecipient(value.single);
      case Uuids.public when value.single == encodeUnpair().single:
        if (refusesUnpair) {
          throw UniversalBleException(
            code: UniversalBleErrorCode.failed,
            message: 'Write failed',
            details: '${0x80}',
          );
        }
        await n.unpair();
        if (dropsOnUnpair) {
          drop(deviceId);
          throw UniversalBleException(
            code: UniversalBleErrorCode.failed,
            message: 'Write failed',
            details: '133',
          );
        }
      case Uuids.public:
        await n.setPublic(value.single == 1);
      case Uuids.guest:
        await n.setGuestAfter((value[0] << 16) | (value[1] << 8) | value[2]);
      case Uuids.send:
        await n.send(value);
      case Uuids.inbox:
        await n.confirm(ByteData.sublistView(value).getUint32(0));
      case Uuids.home:
        await n.setHome(value.first == 1 ? value.sublist(1) : null);
      case Uuids.update:
        await n.writeUpdate(value);
      case Uuids.image:
        await n.writePiece(value);
        if (nodes[deviceId]!.piecesWritten == offAfterPieces) power(false);
    }
  }

  @override
  Future<void> requestConnectionPriority(
    String deviceId,
    BleConnectionPriority priority,
  ) async {}

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnimplementedError('${invocation.memberName}');
}

/// Another node, `bark-0ce400000002`, or of the ID `nodeId`.
class OtherNode extends FakeNode {
  static final otherId = Uint8List.fromList([0x0c, 0xe4, 0, 0, 0, 2]);
  @override
  final Uint8List nodeId;
  OtherNode({
    required super.now,
    required super.sodium,
    required super.app,
    List<int>? id,
  }) : nodeId = Uint8List.fromList(id ?? otherId);
}

void main() {
  const device = '00:11:22:33:44:55';

  test('a node is public unless it advertises the service, as a private '
      'one does', () {
    FoundNode of(List<String> services) => FoundNode.of(
      BleDevice(deviceId: 'd', name: 'bark-x', services: services),
    );
    expect(of([Uuids.publicNode]).public, isTrue);
    expect(of([]).public, isTrue);
    expect(of([Uuids.service]).public, isFalse);
    // A scan response says nothing either way.
    expect(of([Uuids.service]).heardAgain(of([])).public, isFalse);
    expect(of([]).heardAgain(of([Uuids.publicNode])).public, isTrue);
  });

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
            other: OtherNode(now: now, sodium: sodium, app: id)..public = false,
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
        expect(
          lookingLine(app),
          'Not connected. Looking for ID FA4E00000001; texts wait on the phone',
        );
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

    test('Bluetooth off ends the link; on again, it connects', () {
      run((async, app, ble) {
        async.elapse(const Duration(seconds: 1));
        expect(app.session.connected, isTrue);
        ble.power(false);
        async.elapse(const Duration(seconds: 1));
        expect(app.session.connected, isFalse);
        expect(app.lookingFor, isNull);
        expect(
          lookingLine(app),
          'Not connected. Bluetooth is off: turn it on to reach ID FA4E00000001',
        );
        async.elapse(const Duration(minutes: 1));
        expect(ble.connects, [paired]);
        ble.power(true);
        async.elapse(const Duration(seconds: 1));
        expect(app.session.connected, isTrue);
        expect(ble.connects, [paired, paired]);
      });
    });

    test('an update cut by Bluetooth off goes on from what the node '
        'holds', () {
      run((async, app, ble) {
        async.elapse(const Duration(seconds: 1));
        final image = parseImage(imageFile(2, 20001));
        final pieces = (image.length / pieceBytes(attMtu)).ceil();
        app.updater.choose(image, 'file');
        ble.offAfterPieces = 40;
        app.updater.send();
        async.elapse(const Duration(seconds: 1));
        expect(app.updater.phase, Phase.paused);
        expect(app.updater.held, greaterThan(0));
        expect(app.session.connected, isFalse);
        ble.power(true);
        async.elapse(const Duration(seconds: 1));
        expect(app.updater.phase, Phase.committed, reason: app.updater.message);
        expect(ble.nodes[paired]!.piecesWritten, lessThan(pieces + window));
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
        expect(app.session.pairingLost, contains(nodeName(FakeNode.id)));
        expect(app.pairAgain, isNotNull);
        expect(app.lookingFor, isNull);
        final connects = ble.connects.length;
        async.elapse(const Duration(minutes: 5));
        expect(app.session.connected, isFalse);
        expect(ble.connects.length, connects);
        // It goes on looking for a public node, of which none is near.
        expect(
          lookingLine(app),
          'Not connected. Looking for a public node; texts wait on the phone',
        );
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
        expect(app.session.pairedWith, contains('bark-0ce400000002'));
        ble.inReach.add(paired);
        async.elapse(const Duration(minutes: 1));
        expect(app.linkDevice, other);
        expect(ble.connects, [other]);
      });
    });
  });

  group('the choice of a round', () {
    FoundNode n(String name, int? rssi, {bool public = true}) =>
        FoundNode(name, name, rssi, public: public);

    test('the strongest public node; one not public is passed over', () {
      final heard = [
        n('a', -70),
        n('b', -50),
        n('c', -40, public: false),
        n('d', null),
      ];
      expect(chooseNode(heard)?.name, 'b');
      expect(chooseNode([n('d', null), n('a', -100)])?.name, 'a');
      expect(chooseNode([n('d', null)])?.name, 'd');
    });

    test('the paired node, heard, before any public node', () {
      final heard = [n('a', -40), n('mine', -95, public: false)];
      expect(chooseNode(heard, paired: {'mine'})?.name, 'mine');
      expect(chooseNode(heard, paired: {'mine'}, linked: true)?.name, 'mine');
      // Not heard: the strongest public node, unless a link is up.
      expect(chooseNode([n('a', -40)], paired: {'mine'})?.name, 'a');
      expect(chooseNode([n('a', -40)], paired: {'mine'}, linked: true), isNull);
    });

    test('the home before every other paired node, however weak; else the '
        'strongest paired node, before any public node', () {
      final heard = [
        n('home', -95, public: false),
        n('near', -50, public: false),
        n('far', -80),
        n('a', -40),
      ];
      const paired = {'home', 'near', 'far'};
      expect(chooseNode(heard, home: 'home', paired: paired)?.name, 'home');
      expect(chooseNode(heard, paired: paired)?.name, 'near');
      // A home the phone is not paired with is no paired node.
      expect(chooseNode(heard, home: 'home', paired: {'far'})?.name, 'far');
      expect(
        chooseNode(heard.skip(1), home: 'home', paired: paired)?.name,
        'near',
      );
    });

    test('no public node heard: none', () {
      expect(chooseNode([]), isNull);
      expect(chooseNode([n('c', -40, public: false)]), isNull);
    });

    test('never the node whose pairing was lost', () {
      final heard = [n('lost', -40), n('a', -80)];
      expect(chooseNode(heard, lost: {'lost'})?.name, 'a');
      expect(chooseNode([n('lost', -40)], lost: {'lost'}), isNull);
    });

    test('never a node that refuses the phone\'s old bond', () {
      final heard = [n('old', -40), n('a', -80)];
      expect(chooseNode(heard, stale: {'old'})?.name, 'a');
      expect(chooseNode([n('old', -40)], stale: {'old'}), isNull);
    });
  });

  group('the search for a public node', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());
    const paired = 'aa:bb:cc:dd:ee:01';
    const weak = 'aa:bb:cc:dd:ee:03', strong = 'aa:bb:cc:dd:ee:04';
    const shut = 'aa:bb:cc:dd:ee:05';
    final weakId = [0x0c, 0xe4, 0, 0, 0, 3],
        strongId = [0x0c, 0xe4, 0, 0, 0, 4];
    final start = DateTime.utc(2026, 11, 8);

    /// Two public nodes, at −70 and −50 dBm, and a stronger one that is
    /// not public; with `withPaired`, the paired node, in reach or not.
    void run(
      void Function(FakeAsync, AppModel, FakeNodes) body, {
      bool withPaired = false,
      bool pairedInReach = false,
      bool allowed = true,
      Set<String> bonds = const {},
      bool stored = true,
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
        OtherNode other(List<int> nid) =>
            OtherNode(now: now, sodium: sodium, app: id, id: nid);
        final ble = FakeNodes(
          {
            if (withPaired) paired: FakeNode(now: now, sodium: sodium, app: id),
            weak: other(weakId)..owned = false,
            strong: other(strongId)..owned = false,
            shut: other([0x0c, 0xe4, 0, 0, 0, 5])..public = false,
          },
          bonds: {if (withPaired) paired, ...bonds},
        );
        ble.rssi.addAll({weak: -70, strong: -50, shut: -40, paired: -90});
        ble.allowed = allowed;
        if (withPaired && !pairedInReach) ble.inReach.remove(paired);
        UniversalBle.setInstance(ble);
        if (withPaired && stored) app.session.setPaired(nodeName(FakeNode.id));
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

    /// A round's scan and an errand.
    const round = Duration(seconds: 5);

    /// A payload for this phone at `node`, which opens for nobody.
    Uint8List mail(AppModel app, List<int> node, int n) => Uint8List.fromList([
      formatByte(Form.boxed),
      recipientTag(sodium, app.me.identity, node),
      n,
      ...List.filled(40, 0),
    ]);

    test('without a paired node: an errand at the strongest, its tag '
        'written, its mail read, and gone; again a minute later', () {
      run((async, app, ble) {
        final node = ble.nodes[strong]!;
        node.deliver(mail(app, strongId, 1));
        node.deliver(mail(app, strongId, 2));
        async.elapse(const Duration(seconds: 1));
        expect(
          lookingLine(app),
          'Not connected. Looking for a public node; texts wait on the phone',
        );
        async.elapse(round);
        final s = app.session;
        expect(ble.connects, [strong]);
        expect(s.connected, isFalse);
        expect([for (final r in s.received) r.payload[2]], [1, 2]);
        expect(s.guestAfter[hex(strongId)], node.ring.last.number);
        // Connect, tag, read, leave; no pairing, and nothing the guest may
        // not touch.
        final log = [for (final l in s.log) l.text];
        int at(String t) => log.indexWhere((l) => l.startsWith(t));
        expect(at('Recipient: tag') < at('Guest: item'), isTrue);
        expect(at('Guest: item') < at('Disconnected'), isTrue);
        expect(ble.bonds, isEmpty);
        expect(s.pairedWith, isEmpty);
        expect(log.where((l) => l.contains('0x5')), isEmpty);
        // Nothing to send: not before a minute is up, four rounds.
        async.elapse(const Duration(seconds: 45));
        expect(ble.connects, [strong]);
        node.deliver(mail(app, strongId, 3));
        async.elapse(const Duration(seconds: 15));
        expect(ble.connects, [strong, strong]);
        expect([for (final r in s.received) r.payload[2]], [1, 2, 3]);
      });
    });

    test('something to send: an errand at the next round, which sends it '
        'and leaves', () {
      run((async, app, ble) {
        async.elapse(round);
        expect(ble.connects, [strong]);
        final s = app.session;
        final dst = Address(Position.fromDegrees(50, 30), [9, 9, 9, 9, 9, 9]);
        final o = Outgoing(
          dst,
          mail(app, [9, 9, 9, 9, 9, 9], 7),
          at: start,
          kind: OutKind.text,
          sent: 1,
        );
        s.queue(o);
        async.elapse(lookRound);
        expect(ble.connects, [strong, strong]);
        expect(o.state, OutState.taken);
        expect(s.connected, isFalse);
      });
    });

    test('closed at once for another guest: busy, the next round tries '
        'again', () {
      run((async, app, ble) {
        final node = ble.nodes[strong]!;
        final other = node.connectGuest();
        node.deliver(mail(app, strongId, 1));
        async.elapse(round);
        expect(ble.connects, [strong]);
        expect(app.session.connected, isFalse);
        expect(app.session.received, isEmpty);
        other.close();
        async.elapse(lookRound);
        expect(ble.connects, [strong, strong]);
        expect([for (final r in app.session.received) r.payload[2]], [1]);
      });
    });

    test('the paired node not heard in two rounds: out of reach, errands '
        'at a public node; heard again, back on it at once', () {
      run(withPaired: true, (async, app, ble) {
        // One round's miss is no proof.
        async.elapse(round);
        expect(ble.connects, isEmpty);
        expect(app.ownAway, isFalse);
        expect(
          connectionLine(app),
          'Not connected. Looking for ID FA4E00000001; texts wait on the phone',
        );
        // The second, 30 s at a round every 15 s.
        async.elapse(lookRound);
        expect(app.ownAway, isTrue);
        expect(ble.connects, [strong]);
        expect(app.session.connected, isFalse);
        const using = 'Connected to ID 0CE400000004 as a guest';
        expect(connectionLine(app), using);
        expect(app.held?.title, using);
        // The card's node is the one in use.
        expect(app.me.nodeAddress!.id, strongId);
        expect(app.session.pairedWith, contains(nodeName(FakeNode.id)));
        // Still a round every 15 s, for the paired node: two scans in
        // 30 s, within Android's five.
        final scans = ble.scans;
        async.elapse(const Duration(seconds: 30));
        expect(ble.scans - scans, 2);
        expect(connectionLine(app), using);
        ble.inReach.add(paired);
        async.elapse(lookRound);
        expect((app.session.guest, app.linkDevice), (false, paired));
        expect(app.session.connected, isTrue);
        expect(ble.connects, [strong, paired]);
        expect(app.ownAway, isFalse);
        expect(app.guestNode, isNull);
        expect(connectionLine(app), 'Connected to ID FA4E00000001, paired');
        expect(app.me.nodeAddress!.id, FakeNode.id);
        expect(app.held?.title, 'Connected to ID FA4E00000001, paired');
        // On its paired node the search stops.
        final after = ble.scans;
        async.elapse(const Duration(minutes: 1));
        expect(ble.scans, after);
      });
    });

    test('at the edge of the paired node\'s reach, heard every other round, '
        'the phone stays with it; gone for good, it uses a public node', () {
      run(withPaired: true, pairedInReach: true, (async, app, ble) {
        async.elapse(round);
        expect(ble.connects, [paired]);
        for (var i = 0; i < 4; i++) {
          // The link drops, and the next round misses the node.
          ble.inReach.remove(paired);
          ble.drop(paired);
          async.elapse(lookRound);
          ble.inReach.add(paired);
          async.elapse(lookRound);
          expect(app.linkDevice, paired);
        }
        expect(ble.connects.where((d) => d != paired), isEmpty);
        expect(app.ownAway, isFalse);
        ble.inReach.remove(paired);
        ble.drop(paired);
        async.elapse(lookRound * 2);
        expect(app.ownAway, isTrue);
        expect(ble.connects.last, strong);
      });
    });

    test('something to send while the paired node is out of reach goes by '
        'the public node, as a guest\'s', () {
      run(withPaired: true, (async, app, ble) {
        async.elapse(round + lookRound);
        expect(ble.connects, [strong]);
        final dst = Address(Position.fromDegrees(50, 30), [9, 9, 9, 9, 9, 9]);
        final o = Outgoing(
          dst,
          mail(app, [9, 9, 9, 9, 9, 9], 7),
          at: start,
          kind: OutKind.text,
          sent: 1,
        );
        app.session.queue(o);
        async.elapse(lookRound);
        expect(ble.connects, [strong, strong]);
        expect(o.state, OutState.taken);
        expect(ble.bonds, {paired});
      });
    });

    test('a public node that refuses the phone\'s old bond: the bond is '
        'removed and the errand goes on', () {
      run(bonds: {strong}, (async, app, ble) {
        ble.nodes[strong]!.paired = false;
        async.elapse(round + seenWait);
        // Tried as its owner's, refused, then as a guest's.
        expect(ble.connects, [strong, strong]);
        expect(ble.bonds, isEmpty);
        expect(app.staleBond, isNull);
        expect(app.guestNode, nodeName(strongId));
      });
    });

    test('a public node that refuses an old bond the phone keeps: told '
        'once, with the way out, and passed over until it is forgotten', () {
      run(bonds: {strong}, (async, app, ble) {
        ble.nodes[strong]!.paired = false;
        ble.keepsBonds = true;
        async.elapse(round + const Duration(seconds: 3));
        expect(ble.connects.first, strong);
        expect(app.session.connected, isFalse);
        expect(app.staleBond, nodeName(strongId));
        expect(
          staleBondLine(app.staleBond!),
          'ID 0CE400000004 no longer knows this phone\'s old pairing. '
          'Forget it in the phone\'s Bluetooth settings, then try again.',
        );
        // Not tried again every round: the next public node instead.
        async.elapse(lookRound * 2);
        expect(ble.connects.where((d) => d == strong), hasLength(1));
        expect(ble.connects.last, weak);
        final log = [for (final l in app.session.log) l.text];
        expect(log.where((l) => l.contains('old bond')), hasLength(1));
        // The person forgets it in the phone's settings.
        ble.bonds.remove(strong);
        async.elapse(lookRound * 4);
        expect(app.staleBond, isNull);
        expect(ble.connects.last, strong);
        expect(ble.bonds, isEmpty);
      });
    });

    test('a node Android holds a bond with, unknown to the app, that still '
        'holds the phone\'s: taken back as the paired node', () {
      run(withPaired: true, pairedInReach: true, stored: false, (
        async,
        app,
        ble,
      ) {
        async.elapse(round);
        expect((app.session.connected, app.session.guest), (true, false));
        expect(app.linkDevice, paired);
        expect(app.pairedNodes, contains(nodeName(FakeNode.id)));
        expect(connectionLine(app), 'Connected to ID FA4E00000001, paired');
        expect(app.offerHome, app.session.address != null);
        expect(ble.bonds, {paired});
      });
    });

    test('a node Android holds a bond with that refuses it: the bond is '
        'removed and the node used as a guest', () {
      run(withPaired: true, pairedInReach: true, stored: false, (
        async,
        app,
        ble,
      ) {
        ble.nodes[paired]!.paired = false;
        ble.inReach.removeAll({weak, strong});
        async.elapse(round + seenWait);
        expect(ble.bonds, isEmpty);
        expect(app.session.pairedWith, isEmpty);
        expect(app.session.pairingLost, isEmpty);
        expect(app.staleBond, isNull);
        expect(app.guestNode, nodeName(FakeNode.id));
      });
    });

    test('the owner unpairs: the node forgets the phone, then the phone its '
        'bond, and the node serves it as a guest after', () {
      run(withPaired: true, pairedInReach: true, (async, app, ble) {
        async.elapse(round);
        expect((app.session.guest, app.linkDevice), (false, paired));
        ble.inReach.removeAll({weak, strong});
        bool? forgotten;
        app.unpair().then((v) => forgotten = v);
        async.elapse(const Duration(seconds: 1));
        expect(forgotten, isTrue);
        expect(ble.nodes[paired]!.paired, isFalse);
        expect(ble.bonds, isEmpty);
        expect(app.session.pairedWith, isEmpty);
        expect(app.staleBond, isNull);
        async.elapse(lookRound * 5);
        expect(ble.connects.last, paired);
        expect(app.guestNode, nodeName(FakeNode.id));
        expect(app.staleBond, isNull);
      });
    });

    test('the owner unpairs and the link ends under the write: done all '
        'the same, the phone forgets its bond, and the node serves it as a '
        'guest', () {
      run(withPaired: true, pairedInReach: true, (async, app, ble) {
        async.elapse(round);
        ble
          ..inReach.removeAll({weak, strong})
          ..dropsOnUnpair = true;
        Object? failed;
        bool? forgotten;
        app.unpair().then(
          (v) => forgotten = v,
          onError: (Object e) {
            failed = e;
          },
        );
        async.elapse(const Duration(seconds: 1));
        expect(failed, isNull);
        expect(forgotten, isTrue);
        expect(ble.nodes[paired]!.paired, isFalse);
        expect(ble.bonds, isEmpty);
        expect(app.session.pairedWith, isEmpty);
        async.elapse(lookRound * 3);
        expect(ble.connects.last, paired);
        expect(app.guestNode, nodeName(FakeNode.id));
      });
    });

    test('the node refuses the unpairing: an error, and nothing forgotten', () {
      run(withPaired: true, pairedInReach: true, (async, app, ble) {
        async.elapse(round);
        ble.refusesUnpair = true;
        Object? failed;
        app.unpair().then<void>((_) {}, onError: (Object e) => failed = e);
        // Not before the link could have ended.
        async.elapse(unpairEnd - const Duration(seconds: 1));
        expect(failed, isNull);
        async.elapse(const Duration(seconds: 2));
        expect(failed, isA<AttException>());
        expect(ble.nodes[paired]!.paired, isTrue);
        expect(ble.bonds, {paired});
        expect(app.session.pairedWith, contains(nodeName(FakeNode.id)));
        expect(app.session.connected, isTrue);
      });
    });

    test('the owner unpairs on a phone that keeps its bond: the person is '
        'told to forget the node, which is passed over until then', () {
      run(withPaired: true, pairedInReach: true, (async, app, ble) {
        async.elapse(round);
        ble
          ..inReach.removeAll({weak, strong})
          ..keepsBonds = true;
        bool? forgotten;
        app.unpair().then((v) => forgotten = v);
        async.elapse(const Duration(seconds: 3));
        expect(forgotten, isFalse);
        expect(ble.bonds, {paired});
        expect(app.staleBond, nodeName(FakeNode.id));
        final connects = ble.connects.length;
        async.elapse(lookRound * 4);
        expect(ble.connects, hasLength(connects));
        ble.bonds.remove(paired);
        async.elapse(lookRound * 2);
        expect(app.staleBond, isNull);
        expect(ble.connects.last, paired);
        expect(app.guestNode, nodeName(FakeNode.id));
      });
    });

    test('a node no longer this phone\'s, its bond forgotten in the phone\'s '
        'settings: used as a guest like any public node', () {
      // As an older app left it: a Pair on the unpaired node, refused for
      // the bond Android kept, marked the node lost, and the mark outlived
      // the bond.
      run(withPaired: true, pairedInReach: true, (async, app, ble) {
        ble
          ..inReach.removeAll({weak, strong})
          ..bonds.remove(paired);
        ble.nodes[paired]!.paired = false;
        app.session.setPaired(nodeName(FakeNode.id), lost: true);
        async.elapse(lookRound * 2);
        expect(ble.connects, [paired]);
        expect(app.guestNode, nodeName(FakeNode.id));
        expect(ble.bonds, isEmpty);
        // While Android still holds the bond, the node is passed over.
        ble.bonds.add(paired);
        final connects = ble.connects.length;
        async.elapse(guestCheck * 2);
        expect(ble.connects, hasLength(connects));
      });
    });

    test('the paired node before a stronger public node', () {
      run(withPaired: true, pairedInReach: true, (async, app, ble) {
        async.elapse(round);
        expect((app.session.guest, app.linkDevice), (false, paired));
        expect(ble.connects, [paired]);
        final owner = ble.nodes[paired]!;
        expect(
          owner.ownerTag,
          recipientTag(sodium, app.me.identity, FakeNode.id),
        );
      });
    });

    test('the person\'s stop ends it; the fake node wins', () {
      run((async, app, ble) {
        async.elapse(round);
        expect(ble.connects, [strong]);
        app.stopLooking();
        async.elapse(const Duration(seconds: 1));
        final scans = ble.scans;
        async.elapse(const Duration(minutes: 2));
        expect(app.session.connected, isFalse);
        expect(ble.scans, scans);
        expect(ble.connects, [strong]);
        expect(lookingLine(app), isNull);
        app.setFake(true);
        app.session.setStopped(false);
        async.elapse(const Duration(minutes: 1));
        expect(ble.scans, scans);
        expect(app.searchOn, isFalse);
      });
    });

    test('before Bluetooth is allowed nothing runs and nothing shows; the '
        'person\'s Scan asks, and the search runs from then', () {
      run(allowed: false, (async, app, ble) {
        async.elapse(const Duration(minutes: 1));
        expect(ble.scans, 0);
        expect(app.lookingNow, isFalse);
        expect(app.held, isNull);
        expect(lookingLine(app), isNull);
        app.scan();
        async.elapse(const Duration(seconds: 1));
        expect(ble.asked, 1);
        expect(app.bluetoothAllowed, isTrue);
        expect(
          app.held?.title,
          'Not connected. Looking for a public node; texts wait on the phone',
        );
        app.stopScan();
        async.elapse(round);
        expect(ble.connects, [strong]);
        expect(app.held?.title, 'Connected to ID 0CE400000004 as a guest');
      });
    });

    test('at start the app asks for Bluetooth at once, after saying why, '
        'and only once', () {
      run(allowed: false, (async, app, ble) {
        var whys = 0;
        Future<bool> why() async {
          whys++;
          return true;
        }

        app.askForBluetooth(why);
        async.elapse(const Duration(seconds: 1));
        expect((whys, ble.asked), (1, 1));
        expect(app.bluetoothAllowed, isTrue);
        expect(app.me.askedBluetooth, isTrue);
        async.elapse(round);
        expect(ble.connects, [strong]);
        app.askForBluetooth(why);
        async.flushMicrotasks();
        expect((whys, ble.asked), (1, 1));
      });
    });

    test('a public node the search would join: joined as a guest, without '
        'a pairing', () {
      run((async, app, ble) {
        async.elapse(round);
        expect(ble.connects, [strong]);
        expect(ble.pairings, 0);
        expect(app.session.pairedWith, isEmpty);
        async.elapse(const Duration(minutes: 1));
        expect(ble.connects, [strong, strong]);
        expect(ble.pairings, 0);
      });
    });

    test('the lines name the home where one is set, and the help says why '
        'the phone is where it is', () {
      run(withPaired: true, (async, app, ble) {
        // The home is elsewhere, a node this phone is not paired with.
        app.me.setHome(
          Address(Position.fromDegrees(50, 30), [9, 9, 9, 9, 9, 9]),
        );
        async.elapse(round);
        expect(
          connectionLine(app),
          'Not connected. Looking for ID FA4E00000001; texts wait on the phone',
        );
        expect(connectionHelp(app), startsWith('The phone looks every 15'));
        async.elapse(lookRound);
        const line =
            'Connected to ID 0CE400000004 as a guest. Home at ID 090909090909';
        expect(connectionLine(app), line);
        expect(app.held?.title, line);
        expect(
          connectionHelp(app),
          startsWith('None of the nodes this phone is paired with'),
        );
        ble.inReach.add(paired);
        async.elapse(lookRound);
        expect(
          connectionLine(app),
          'Connected to ID FA4E00000001, paired. Home at ID 090909090909',
        );
      });
    });

    test('a phone paired with none says why it uses a public node', () {
      run((async, app, ble) {
        async.elapse(round);
        expect(connectionLine(app), 'Connected to ID 0CE400000004 as a guest');
        expect(
          connectionHelp(app),
          startsWith('This phone is paired with no node near you'),
        );
      });
    });

    test('a guest makes the node its home: the phone pairs by the code on '
        'its screen, then gives it the mailbox key', () {
      run((async, app, ble) {
        async.elapse(round);
        expect(app.session.connected, isFalse);
        expect(app.guestHere?.found.name, nodeName(strongId));
        expect(app.offerHome, isTrue);
        String? told;
        app.homeHere().then((r) => told = r);
        async.elapse(round);
        expect(told, 'This node is your home');
        expect(ble.pairings, 1);
        expect(ble.bonds, {strong});
        expect((app.session.connected, app.session.guest), (true, false));
        expect(app.pairedNodes, contains(nodeName(strongId)));
        expect(ble.nodes[strong]!.homeKey, app.me.keys.mailbox.extractBytes());
        expect(app.me.home?.id, strongId);
        expect(app.offerHome, isFalse);
      });
    });

    test('a wrong code: not paired, no home, and the node still used as a '
        'guest', () {
      run((async, app, ble) {
        async.elapse(round);
        ble.wrongCode = true;
        Object? failed;
        app.homeHere().then<void>((_) {}, onError: (Object e) => failed = e);
        async.elapse(round);
        expect(failed, isA<NotPaired>());
        expect(
          notPairedLine(failed!),
          'Pairing did not complete: it was cancelled, or the code typed was '
          'not the one on the node\'s screen. The phone still uses the node '
          'as a guest.',
        );
        expect(ble.bonds, isEmpty);
        expect(app.session.pairedWith, isEmpty);
        expect(app.me.home, isNull);
        expect(ble.nodes[strong]!.homeKey, isNull);
        final connects = ble.connects.length;
        async.elapse(guestCheck + round);
        expect(ble.connects.length, greaterThan(connects));
        expect(ble.connects.last, strong);
        expect(app.guestNode, nodeName(strongId));
        expect(app.offerHome, isTrue);
      });
    });

    test('a guest\'s node that cannot be a home, or has no place, is not '
        'offered', () {
      run((async, app, ble) {
        ble.nodes[strong]!.sleepy = true;
        ble.nodes[weak]!.fixed = false;
        async.elapse(round);
        expect(app.guestHere?.homeable, isFalse);
        expect(app.offerHome, isFalse);
        ble.inReach.remove(strong);
        async.elapse(guestCheck + round);
        expect(app.guestHere?.found.name, nodeName(weakId));
        expect(app.guestHere?.address, isNull);
        expect(app.offerHome, isFalse);
      });
    });

    test('Not now holds for the app\'s run', () {
      run((async, app, ble) {
        async.elapse(round);
        expect(app.offerHome, isTrue);
        app.notHome();
        expect(app.offerHome, isFalse);
        async.elapse(guestCheck * 3);
        expect(app.guestNode, nodeName(strongId));
        expect(app.offerHome, isFalse);
      });
    });

    test('a keeper pairs with a public node it uses as a guest without '
        'making it the home', () {
      run((async, app, ble) {
        async.elapse(round);
        app.connect(app.guestHere!.found);
        async.elapse(round);
        expect(ble.pairings, 1);
        expect((app.session.connected, app.session.guest), (true, false));
        expect(app.pairedNodes, contains(nodeName(strongId)));
        expect(app.me.home, isNull);
        expect(ble.nodes[strong]!.homeKey, isNull);
        expect(app.guestHere, isNull);
      });
    });

    test(
      'Pair pressed on a node with a live bond adopts it without a code',
      () {
        run(bonds: {strong}, (async, app, ble) {
          app.connect(FoundNode(strong, nodeName(strongId), -50, public: true));
          async.elapse(round);
          expect(ble.pairings, 0);
          expect((app.session.connected, app.session.guest), (true, false));
          expect(app.pairedNodes, contains(nodeName(strongId)));
          expect(ble.bonds, {strong});
        });
      },
    );

    test('Pair on a node with the phone\'s old bond for it: the bond is '
        'forgotten and the pairing makes a new one', () {
      run(bonds: {strong}, (async, app, ble) {
        ble.nodes[strong]!.paired = false;
        app.connect(FoundNode(strong, nodeName(strongId), -50, public: true));
        async.elapse(round + seenWait);
        // Over the old bond first, refused, then paired with a code.
        expect(ble.connects, [strong, strong]);
        expect((app.session.connected, app.session.guest), (true, false));
        expect(app.pairedNodes, contains(nodeName(strongId)));
        expect(app.session.pairingLost, isEmpty);
        expect(app.staleBond, isNull);
        expect(ble.pairings, 1);
      });
    });

    test('a fetch while no node is linked waits, and goes from the next '
        'node the phone links to', () {
      run((async, app, ble) {
        final home = Address(Position.fromDegrees(50, 30), [9, 9, 9, 9, 9, 9]);
        app.stopLooking();
        async.flushMicrotasks();
        app.me.setHome(home);
        String? told;
        app.fetchNow().then((t) => told = t);
        async.flushMicrotasks();
        expect(told, startsWith('The fetch waits for a node'));
        expect(app.me.fetchQueued, isTrue);
        expect(app.session.outbox, isEmpty);
        app.session.setStopped(false);
        async.elapse(round);
        expect(ble.connects, [strong]);
        final f = app.session.outbox.single;
        expect(f.kind, OutKind.fetch);
        expect(f.dst.toString(), home.toString());
        expect(f.state, OutState.taken);
        expect(app.me.fetchQueued, isFalse);
      });
    });
  });

  group('the search with several paired nodes', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());
    const home = 'aa:bb:cc:dd:ee:01', near = 'aa:bb:cc:dd:ee:06';
    const strong = 'aa:bb:cc:dd:ee:04';
    final nearId = [0x0c, 0xe4, 0, 0, 0, 6],
        strongId = [0x0c, 0xe4, 0, 0, 0, 4];
    final start = DateTime.utc(2026, 11, 10);

    /// The phone paired with the fake node at `home`, at −90 dBm, made its
    /// home unless not `withHome`, then with another node at `near`, at
    /// −60 dBm, as the user's round of 2026-11-10 had it; and a public
    /// node nobody owns at −50 dBm.
    void run(
      void Function(FakeAsync, AppModel, FakeNodes) body, {
      bool withHome = true,
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
        OtherNode other(List<int> nid) =>
            OtherNode(now: now, sodium: sodium, app: id, id: nid);
        final ble = FakeNodes(
          {
            home: FakeNode(now: now, sodium: sodium, app: id),
            near: other(nearId)..public = false,
            strong: other(strongId)..owned = false,
          },
          bonds: {home, near},
        );
        ble.rssi.addAll({home: -90, near: -60, strong: -50});
        UniversalBle.setInstance(ble);
        if (withHome) {
          app.me.setHome(Address(Position.fromDegrees(50, 30), FakeNode.id));
        }
        app.session.setPaired(nodeName(FakeNode.id));
        app.session.setPaired(nodeName(nearId));
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

    const round = Duration(seconds: 5);

    test('the home before another paired node, though weaker', () {
      run((async, app, ble) {
        async.elapse(round);
        expect(ble.connects, [home]);
        expect((app.session.connected, app.session.guest), (true, false));
        expect(connectionLine(app), 'Connected to your home, ID FA4E00000001');
        expect(app.held?.title, 'Connected to your home, ID FA4E00000001');
        expect(
          connectionHelp(app),
          startsWith(
            'This phone is paired with your '
            'home node',
          ),
        );
        // Both stay paired.
        expect(app.pairedNodes, {nodeName(FakeNode.id), nodeName(nearId)});
      });
    });

    test('the home out of reach: the other paired node, as its owner, '
        'before a stronger public node and without waiting', () {
      run((async, app, ble) {
        ble.inReach.remove(home);
        async.elapse(round);
        expect(ble.connects, [near]);
        expect((app.session.connected, app.session.guest), (true, false));
        expect(
          connectionLine(app),
          'Connected to ID 0CE400000006, paired. Home at ID FA4E00000001',
        );
        expect(app.ownAway, isFalse);
      });
    });

    test('without a home, the strongest paired node', () {
      run(withHome: false, (async, app, ble) {
        async.elapse(round);
        expect(ble.connects, [near]);
        expect(connectionLine(app), 'Connected to ID 0CE400000006, paired');
      });
    });

    test('none in reach: the search looks for the home first, then uses a '
        'public node after two rounds', () {
      run((async, app, ble) {
        ble.inReach.removeAll({home, near});
        async.elapse(const Duration(seconds: 1));
        // The home, though the other was paired with last.
        expect(app.lookingFor, nodeName(FakeNode.id));
        expect(
          connectionLine(app),
          'Not connected. Looking for ID FA4E00000001; texts wait on the phone',
        );
        async.elapse(round);
        expect(ble.connects, isEmpty);
        async.elapse(lookRound);
        expect(app.ownAway, isTrue);
        expect(ble.connects, [strong]);
        expect(
          connectionLine(app),
          'Connected to ID 0CE400000004 as a guest. Home at ID FA4E00000001',
        );
        // The other paired node back in reach: on it at once.
        ble.inReach.add(near);
        async.elapse(lookRound);
        expect((app.linkDevice, app.session.guest), (near, false));
      });
    });
  });

  group('a node Android holds a bond with that the app does not count as '
      'paired', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());
    const mine = 'aa:bb:cc:dd:ee:01', bonded = 'aa:bb:cc:dd:ee:04';
    final bondedId = [0x0c, 0xe4, 0, 0, 0, 4];
    final start = DateTime.utc(2026, 11, 10);

    /// The phone paired with the fake node at `mine`, out of reach, and
    /// holding a bond the app does not know of with a public node in
    /// reach, as after an older app kept only the node paired with last.
    void run(void Function(FakeAsync, AppModel, FakeNodes) body) {
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
            mine: FakeNode(now: now, sodium: sodium, app: id),
            bonded: OtherNode(now: now, sodium: sodium, app: id, id: bondedId),
          },
          bonds: {mine, bonded},
        );
        ble.inReach.remove(mine);
        ble.rssi.addAll({bonded: -50});
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

    test('the search takes it back as paired, at once, never as a '
        'guest', () {
      run((async, app, ble) {
        async.elapse(const Duration(seconds: 5));
        expect(ble.connects, [bonded]);
        expect((app.session.connected, app.session.guest), (true, false));
        expect(app.guestNode, isNull);
        expect(app.pairedNodes, {nodeName(FakeNode.id), nodeName(bondedId)});
        expect(ble.pairings, 0);
        expect(connectionLine(app), 'Connected to ID 0CE400000004, paired');
      });
    });

    test('a tap on it as a public node takes it back too', () {
      run((async, app, ble) {
        app.stopLooking();
        async.flushMicrotasks();
        app.connectGuest(FoundNode(bonded, nodeName(bondedId), -50));
        async.elapse(const Duration(seconds: 1));
        expect((app.session.connected, app.session.guest), (true, false));
        expect(app.pairedNodes, contains(nodeName(bondedId)));
        expect(ble.pairings, 0);
      });
    });
  });
}
