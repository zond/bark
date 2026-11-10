// A phone's Bluetooth in Dart, for the tests: fake nodes by device ID,
// each advertising while in reach and answering as its node does, and a
// way to link the app to one as its owner, as a phone paired with it is.

import 'dart:async';
import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/ble_link.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/node_link.dart';
import 'package:universal_ble/universal_ble.dart';

import 'fake_node.dart';

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

  /// The RSSI each device is heard with, dBm; one not given is heard at
  /// −60 dBm, a node a room away, as Android reports an RSSI with every
  /// scan result.
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

  /// Switched off for good, at the end of a test: the nodes' timers stop,
  /// and the app, gone too, hears of no link's end.
  bool _off = false;
  void dispose() {
    _off = true;
    for (final n in nodes.values) {
      n.dispose();
    }
  }

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
            rssi: rssi[id] ?? -60,
            services: [n.public ? Uuids.publicNode : Uuids.service],
          ),
        );
      }
    });
  }

  @override
  Future<void> stopScan() async {}

  /// How long Android takes to make a link.
  Duration setUp = Duration.zero;

  @override
  Future<void> connect(
    String deviceId, {
    Duration? connectionTimeout,
    bool autoConnect = false,
    ConnectionPlatformConfig? platformConfig,
  }) async {
    connects.add(deviceId);
    if (setUp > Duration.zero) await Future<void>.delayed(setUp);
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

  /// The devices that end every link as it is set up, as a node did on
  /// the bench, 2026-11-10, 0.2 s after the phone connected.
  final Set<String> endsLinks = {};

  /// The devices that end the link right after the app's first read of
  /// Node, which needs no encryption.
  final Set<String> endsAfterNode = {};

  /// A node that holds another phone's bond ends a bonded phone's link
  /// at once, as [RefusingNode] does, and so does one of [endsLinks];
  /// otherwise the link is the fake's.
  @override
  Future<int> requestMtu(String deviceId, int expectedMtu) async {
    final n = nodes[deviceId]!;
    if ((bonds.contains(deviceId) && !n.paired) ||
        endsLinks.contains(deviceId)) {
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
    // What the node notifies reaches the app as Android gives it, in
    // bytes.
    void notify<T>(Stream<T> s, String c, Uint8List Function(T) bytes) =>
        s.listen((v) {
          if (_links[deviceId] == link) {
            updateCharacteristicValue(deviceId, c, bytes(v), null);
          }
        });
    notify(link.statusNotified, Uuids.status, (st) => st.encode());
    notify(
      link.givenBackNotified,
      Uuids.givenBack,
      (g) => g?.encode() ?? Uint8List(0),
    );
    notify(link.answerNotified, Uuids.ask, (a) => a?.encode() ?? Uint8List(0));
    // A link the node ends, as a guest's at its 30 s or for another
    // guest, or at its reboot, ends for Android too.
    link.closed.then((_) {
      if (_off || _links[deviceId] != link) return;
      _links.remove(deviceId);
      _up.remove(deviceId);
      updateConnection(deviceId, false);
    });
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
    // A full node answers the pairing with Pairing Failed.
    if (wrongCode || !nodes[deviceId]!.takesPairing) return false;
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
    if (characteristic == Uuids.node && endsAfterNode.contains(deviceId)) {
      final v = (await n.readNode()).encode();
      drop(deviceId);
      return v;
    }
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
      Uuids.phones => (await n.readPhones()).encode(),
      Uuids.neighbours => (await n.readNeighbours()).encode(),
      _ => throw UnimplementedError(characteristic),
    };
  }

  /// Every write the app makes, decoded as the node decodes it.
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
        final r = decodeRecipient(value);
        await n.setRecipient(r.tag, r.name);
      case Uuids.phones when value.first == 0:
        await n.setPhonesFrom(value[1]);
      case Uuids.phones when value.first == 1:
        await n.askRemovalCode();
      case Uuids.phones:
        await n.removePhone(
          (value[1] << 8) | value[2],
          bond: value[3] & 1 != 0,
          home: value[3] & 2 != 0,
          code: (value[4] << 16) | (value[5] << 8) | value[6],
        );
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
      case Uuids.clock:
        final b = ByteData.sublistView(value);
        await n.setClock(b.getUint32(0), b.getInt16(4));
      case Uuids.mapPoint:
        await n.setMapPoint(
          value.first == 1 ? Position.decode(value.sublist(1)) : null,
        );
      case Uuids.neighbours:
        await n.setNeighboursFrom(value.single);
      case Uuids.ask:
        await n.ask(value);
      default:
        throw UnimplementedError(characteristic);
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

/// The device of the tests' own node, the one [FakeNodeOf] links to.
const fakeDevice = 'fa:4e:00:00:00:01';

final _bluetooth = Expando<FakeNodes>();

/// The app's own node in a test: a [FakeNode] at [fakeDevice] on a fake
/// Bluetooth, which the phone has paired with.
extension FakeNodeOf on AppModel {
  /// The node, made at the first [connectFake] unless the test sets one.
  FakeNode? get fakeNode => _bluetooth[this]?.nodes[fakeDevice];
  set fakeNode(FakeNode? n) {
    final ble = n == null
        ? null
        : FakeNodes({fakeDevice: n}, bonds: {fakeDevice});
    _bluetooth[this] = ble;
    if (ble != null) UniversalBle.setInstance(ble);
  }

  /// The fake Bluetooth the node is on.
  FakeNodes? get fakeBle => _bluetooth[this];

  /// Links the app to [fakeNode] as its owner, as the person's tap on it
  /// in Settings does; it counts as paired with the phone first.
  Future<void> connectFake() async {
    final n = fakeNode ??= FakeNode(
      now: now,
      sodium: sodium,
      app: me.identity,
      mailbox: me.keys.mailbox,
    );
    final name = nodeName(n.nodeId);
    if (!session.pairedWith.contains(name)) await session.setPaired(name);
    await connect(FoundNode(fakeDevice, name, -60, public: n.public));
  }
}

/// Disposes of the app and of its test node, whose timers run on
/// otherwise.
extension DisposeWithNode on AppModel {
  void disposeWithNode() {
    final ble = fakeBle;
    dispose();
    ble?.dispose();
  }
}
