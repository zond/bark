// The phone link over Bluetooth LE.

import 'dart:async';
import 'dart:typed_data';

import 'package:universal_ble/universal_ble.dart';

import 'link.dart';
import 'node_link.dart';

/// A node seen in a scan; [public] unless it advertised the service's
/// UUID, as a node whose owner made it private does: a node is public,
/// serving phones that have not paired with it, unless its owner says
/// otherwise (spec "Phone link", Public).
class FoundNode {
  final String deviceId;
  final String name;
  final int? rssi;
  final bool public;
  const FoundNode(this.deviceId, this.name, this.rssi, {this.public = true});

  /// A scan result, named by its device where Android gives no name yet:
  /// the name is in the scan response, which may come after the
  /// advertisement.
  factory FoundNode.of(BleDevice d) {
    final name = d.name ?? '';
    return FoundNode(
      d.deviceId,
      name.isEmpty ? d.deviceId : name,
      d.rssi,
      public: !d.services.any((u) => u.toLowerCase() == Uuids.service),
    );
  }

  /// This node and `n`, the same device heard again: its name once one
  /// came, the stronger RSSI, as a fade takes some advertisements down
  /// and none up, and private once any of them said so, as a scan
  /// response says nothing either way.
  FoundNode heardAgain(FoundNode n) => FoundNode(
    deviceId,
    name == deviceId ? n.name : name,
    switch ((rssi, n.rssi)) {
      (final a?, final b?) => a > b ? a : b,
      (final a, final b) => a ?? b,
    },
    public: public && n.public,
  );

  /// Whether it was heard at a signal a link holds at: not below
  /// [weakDbm]. One heard without an RSSI ranks as the weakest.
  bool get usable => (rssi ?? noRssi) >= weakDbm;
}

/// Ranks a node heard without an RSSI below every RSSI a controller
/// reports, −127 dBm and up (Bluetooth Core Specification 5.0, Vol 4
/// Part E §7.7.65.2).
const int noRssi = -128;

/// Strength, in dBm, at or above which a node's advertisement reads as
/// strong. A choice: these are the advertisement's strength as the phone
/// hears it before any link, so the node's noise floor, by which a link's
/// quality is judged, does not apply.
const int strongDbm = -70;

/// Strength, in dBm, below which a node's advertisement reads as weak,
/// and the search passes a node over for one heard above it; a choice,
/// as [strongDbm].
const int weakDbm = -85;

/// The ATT transaction timeout (Bluetooth Core Specification 5.0, Vol 3
/// Part F §3.3.3). The node answers Send only once it has the message,
/// so a write may wait longer than the plugin's default.
const Duration attTimeout = Duration(seconds: 30);

/// Scans for nodes by the service UUID and the public node's; each result
/// goes to `found`.
Future<void> startScan(void Function(FoundNode) found) async {
  UniversalBle.onScanResult = (d) => found(FoundNode.of(d));
  await _scanForNodes();
}

Future<void> _scanForNodes({AndroidScanMode? mode}) => UniversalBle.startScan(
  scanFilter: ScanFilter(withServices: [Uuids.service, Uuids.publicNode]),
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

/// How long a round of the search scans: four of a node's advertising
/// intervals of 1022.5 ms (spec "Phone link", Finding the node). The scan
/// listens without a break ([AndroidScanMode.lowLatency]), so with the
/// screen on it hears nearly every advertisement in reach; the extra
/// intervals cover a scan's start and an advertisement lost. With the
/// screen off Android listens less, and a later round makes up for a
/// miss. Short of the paired node, a round listens all of it, so that it
/// weighs each public node by the best of its four advertisements.
const Duration lookScan = Duration(microseconds: 4 * 1022500);

/// How often a round of the search starts. Android refuses an app's
/// sixth scan started within 30 s, silently (its Bluetooth service,
/// since Android 7); a round every 15 s takes two of the five and leaves
/// three for the person's own Scan and Pair again.
const Duration lookRound = Duration(seconds: 15);

/// Scans for [lookScan], or until a node heard is one `until` stops at or
/// `stop` completes: the nodes heard, by device, each at the best RSSI it
/// was heard with.
Future<Map<String, FoundNode>> listen({
  bool Function(FoundNode)? until,
  Future<void>? stop,
}) async {
  final heard = <String, FoundNode>{};
  await _scanUntil(
    (d) {
      final n = FoundNode.of(d);
      final best = heard.update(
        d.deviceId,
        (was) => was.heardAgain(n),
        ifAbsent: () => n,
      );
      return until != null && until(best);
    },
    lookScan,
    mode: AndroidScanMode.lowLatency,
    stop: stop,
  );
  return heard;
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

class BleLink implements NodeLink {
  final String deviceId;
  final _closed = Completer<void>();
  StreamSubscription<bool>? _connection;
  StreamSubscription<AvailabilityState>? _adapter;

  BleLink._(this.deviceId);

  /// Connects, asks for the MTU of [attMtu], and pairs if the phone has
  /// no bond with the node, telling `pairing` first, as Android then asks
  /// the person for the code on the node's screen, and `paired` once the
  /// bond is made; returns the MTU agreed.
  ///
  /// As a `guest` of a public node it does not pair, and reads to check a
  /// bond only where the phone holds one: without, Android may start a
  /// pairing by itself on a read the node refuses for want of one, and
  /// the node is not the person's. Throws [StaleBond] for a guest's link
  /// the node refuses for the phone's bond, as for [NotOurs] below.
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
    bool guest = false,
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
    // Android reports no end of the link when its Bluetooth is switched
    // off: the link stayed up for the app, whose writes then failed with
    // characteristicNotFound until it was restarted (bench, 2026-11-07).
    // The adapter's going off ends it here, as a drop, and the close lets
    // Android forget the dead connection, so that the next connect makes
    // a new one.
    link._adapter = UniversalBle.availabilityStream.listen((a) {
      if (a != AvailabilityState.poweredOn) unawaited(link._adapterOff());
    });
    final int mtu;
    try {
      await UniversalBle.discoverServices(deviceId);
      mtu = await UniversalBle.requestMtu(deviceId, attMtu);
    } catch (e) {
      await link.close();
      if (await setUpRefused(deviceId, hadBond: hadBond)) {
        throw guest ? StaleBond(name) : NotOurs(name);
      }
      rethrow;
    }
    try {
      if (guest) {
        if (await bonded(deviceId)) await link._probe(name, guest: true);
        return (link, mtu);
      }
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
  ///
  /// Returns whether the bond is gone. Android removes a bond only by a
  /// method it hides from apps, which the Bluetooth plugin calls by
  /// reflection and whose failure it does not report, so the bond state
  /// is the only answer; a phone that keeps the bond leaves the person to
  /// forget the node in its Bluetooth settings.
  static Future<bool> forget(String deviceId) async {
    try {
      await UniversalBle.unpair(deviceId);
    } catch (_) {
      // The bond state below says whether it went.
    }
    for (var waited = Duration.zero; await bonded(deviceId);) {
      if (waited >= _forgetWait) return false;
      await Future<void>.delayed(_forgetStep);
      waited += _forgetStep;
    }
    return true;
  }

  /// Whether Android lists a bond with the node; false where it cannot
  /// say.
  static Future<bool> bonded(String deviceId) async {
    try {
      return await UniversalBle.isPaired(deviceId) ?? false;
    } catch (_) {
      return false;
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
    return hadBond || await bonded(deviceId);
  }

  /// A bonded phone's first read, which shows whether the node still
  /// holds its bond: [NotOurs] if not, or for a `guest`, [StaleBond].
  Future<void> _probe(String name, {bool guest = false}) async {
    try {
      await _read(Uuids.node);
    } catch (e) {
      if (bondRefused(e, closed: _closed.isCompleted)) {
        throw guest ? StaleBond(name) : NotOurs(name);
      }
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
  Future<void> setRecipient(int tag, String name) =>
      _write(Uuids.recipient, encodeRecipient(tag, name));

  @override
  Future<void> setPhonesFrom(int slot) =>
      _write(Uuids.phones, encodePhonesFrom(slot));

  @override
  Future<PhonesPage> readPhones() async =>
      PhonesPage.decode(await _read(Uuids.phones));

  @override
  Future<void> askRemovalCode() => _write(Uuids.phones, encodeRemovalCode());

  @override
  Future<void> removePhone(
    int number, {
    required bool bond,
    required bool home,
    required int code,
  }) => _write(
    Uuids.phones,
    encodeRemove(number, bond: bond, home: home, code: code),
  );

  @override
  Future<bool> readPublic() async => decodePublic(await _read(Uuids.public));

  @override
  Future<void> setPublic(bool public) =>
      _write(Uuids.public, encodePublic(public));

  @override
  Future<void> unpair() => _write(Uuids.public, encodeUnpair());

  @override
  Future<void> setGuestAfter(int number) =>
      _write(Uuids.guest, encodeAfter(number));

  @override
  Future<GuestItem?> readGuest() async =>
      GuestItem.decode(await _read(Uuids.inbox));

  @override
  Future<void> subscribeAsk() =>
      UniversalBle.subscribeNotifications(deviceId, Uuids.service, Uuids.ask);

  @override
  Future<void> ask(List<int> id) => _write(Uuids.ask, encodeAsk(id));

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
  Stream<Answer?> get answerNotified => _notified(Uuids.ask).map(Answer.decode);

  @override
  Future<void> get closed => _closed.future;

  Future<void> _adapterOff() async {
    try {
      await close();
    } catch (_) {
      // The link is closed for the app whatever Android answers.
    }
  }

  @override
  Future<void> close() async {
    unawaited(_connection?.cancel());
    unawaited(_adapter?.cancel());
    try {
      await UniversalBle.disconnect(deviceId);
    } finally {
      if (!_closed.isCompleted) _closed.complete();
    }
  }
}
