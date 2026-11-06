// The phone link's values (spec "Phone link"), in the encodings the
// node's `bark-link` crate uses, and the radio's encodings of positions,
// node IDs and message IDs (spec "Packet formats"). Every integer is
// big-endian.

import 'dart:typed_data';

/// The link version this app speaks.
const int linkVersion = 3;

/// The ATT MTU the node offers and the app asks for (spec "Phone link",
/// "MTU").
const int attMtu = 247;

/// The longest payload a DATA frame holds (spec "Packet formats").
const int maxPayload = 227;

/// One random UUID, its second 16 bits counting the service and its
/// characteristics.
class Uuids {
  static const service = '28bc0001-a1b8-4893-9537-2ee3559d7644';
  static const node = '28bc0002-a1b8-4893-9537-2ee3559d7644';
  static const status = '28bc0003-a1b8-4893-9537-2ee3559d7644';
  static const send = '28bc0004-a1b8-4893-9537-2ee3559d7644';
  static const inbox = '28bc0005-a1b8-4893-9537-2ee3559d7644';
  static const givenBack = '28bc0006-a1b8-4893-9537-2ee3559d7644';
  static const mapPoint = '28bc0007-a1b8-4893-9537-2ee3559d7644';

  /// From link version 2.
  static const clock = '28bc0008-a1b8-4893-9537-2ee3559d7644';

  /// From link version 3 (spec "Mailbox").
  static const home = '28bc0009-a1b8-4893-9537-2ee3559d7644';
}

/// ATT error codes the node answers a write with.
class AttError {
  static const invalidLength = 0x0D;
  static const valueNotAllowed = 0x13;

  /// The node takes no message now; wait for ready.
  static const notReady = 0x80;
}

/// A value too short, or of a length it never has.
class Malformed implements Exception {
  final String what;
  const Malformed(this.what);
  @override
  String toString() => 'malformed $what';
}

String hex(List<int> bytes) =>
    bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();

/// Hex digits to bytes, ignoring spaces, colons and dashes a person may
/// paste with them; null if anything else is in it.
Uint8List? parseHex(String s) {
  final digits = s.replaceAll(RegExp(r'[\s:\-]'), '');
  if (digits.length.isOdd || !RegExp(r'^[0-9a-fA-F]*$').hasMatch(digits)) {
    return null;
  }
  return Uint8List.fromList([
    for (var i = 0; i < digits.length; i += 2)
      int.parse(digits.substring(i, i + 2), radix: 16),
  ]);
}

const int _latBits = 25;
const int _lonBits = 26;

/// A WGS-84 position in counts of 180° / 2²⁵, as on the radio
/// (spec "Positions on the wire").
class Position {
  static const wireLen = 7;
  static const double unitDeg = 180.0 / (1 << 25);

  final int lat;
  final int lon;
  const Position(this.lat, this.lon);

  /// Rounded to the nearest count, half away from zero, and clamped, as
  /// the node does.
  factory Position.fromDegrees(double latDeg, double lonDeg) {
    int round(double x) => x >= 0 ? (x + 0.5).toInt() : (x - 0.5).toInt();
    final latMax = (1 << (_latBits - 1)) - 1;
    final lonMax = (1 << (_lonBits - 1)) - 1;
    return Position(
      round(latDeg / unitDeg).clamp(-latMax - 1, latMax),
      round(lonDeg / unitDeg).clamp(-lonMax - 1, lonMax),
    );
  }

  double get latDeg => lat * unitDeg;
  double get lonDeg => lon * unitDeg;

  /// 56 bits: latitude in the top 25, longitude in the next 26, five
  /// spare bits zero.
  Uint8List encode() {
    final la = lat & ((1 << _latBits) - 1);
    final lo = lon & ((1 << _lonBits) - 1);
    final v = (la << (_lonBits + 5)) | (lo << 5);
    return Uint8List.fromList([
      for (var i = wireLen - 1; i >= 0; i--) (v >> (8 * i)) & 0xff,
    ]);
  }

  static Position decode(List<int> b) {
    var v = 0;
    for (var i = 0; i < wireLen; i++) {
      v = (v << 8) | b[i];
    }
    int signed(int x, int bits) =>
        x & (1 << (bits - 1)) != 0 ? x - (1 << bits) : x;
    return Position(
      signed((v >> (_lonBits + 5)) & ((1 << _latBits) - 1), _latBits),
      signed((v >> 5) & ((1 << _lonBits) - 1), _lonBits),
    );
  }

  @override
  bool operator ==(Object other) =>
      other is Position && other.lat == lat && other.lon == lon;
  @override
  int get hashCode => Object.hash(lat, lon);
  @override
  String toString() =>
      '${latDeg.toStringAsFixed(6)}, ${lonDeg.toStringAsFixed(6)}';
}

/// Bytes in a node ID (spec "Node identity").
const int nodeIdLen = 6;

/// A destination address: a position and a node ID, 13 bytes, which a
/// person gives others as 26 hex digits (spec "Addresses").
class Address {
  static const len = Position.wireLen + nodeIdLen;
  final Position pos;
  final Uint8List id;
  Address(this.pos, List<int> id) : id = Uint8List.fromList(id) {
    if (id.length != nodeIdLen) throw const Malformed('node ID');
  }

  Uint8List encode() => Uint8List.fromList([...pos.encode(), ...id]);

  static Address? parse(String s) {
    final b = parseHex(s);
    if (b == null || b.length != len) return null;
    return Address(Position.decode(b), b.sublist(Position.wireLen));
  }

  @override
  String toString() => hex(encode());
}

/// The 32-bit FNV-1a of `dst_pos || dst_id || payload`, by which every
/// node names a message (spec "Packet formats").
int msgId(Address dst, List<int> payload) {
  var h = 0x811c9dc5;
  for (final b in [...dst.encode(), ...payload]) {
    h = ((h ^ b) * 0x01000193) & 0xffffffff;
  }
  return h;
}

String msgIdHex(int id) => id.toRadixString(16).padLeft(8, '0');

int _u16(List<int> b, int at) => (b[at] << 8) | b[at + 1];
int _u32(List<int> b, int at) =>
    (b[at] << 24) | (b[at + 1] << 16) | (b[at + 2] << 8) | b[at + 3];
List<int> _be16(int v) => [(v >> 8) & 0xff, v & 0xff];
List<int> _be32(int v) => [
  (v >> 24) & 0xff,
  (v >> 16) & 0xff,
  (v >> 8) & 0xff,
  v & 0xff,
];

/// The advertised name: `bark-` and the ID's twelve hex digits.
String nodeName(List<int> id) => 'bark-${hex(id)}';

/// Node, read. Decoders of values the node writes ignore what a later
/// version appends.
class NodeInfo {
  static const len = 1 + nodeIdLen + 4;
  final int version;
  final Uint8List id;
  final int image;
  NodeInfo(this.version, List<int> id, this.image)
    : id = Uint8List.fromList(id);

  Uint8List encode() => Uint8List.fromList([version, ...id, ..._be32(image)]);

  static NodeInfo decode(List<int> b) {
    if (b.length < len) throw const Malformed('Node');
    return NodeInfo(b[0], b.sublist(1, 1 + nodeIdLen), _u32(b, 1 + nodeIdLen));
  }
}

/// Status, read and notify.
class Status {
  static const len = 1 + 1 + 2 + Position.wireLen + 2 + 1;
  final bool ready;
  final bool externalPower;
  final bool sleepy;
  final bool carried;
  final bool mapPoint;
  final bool placed;
  final bool booting;
  final bool anchorless;
  final int relays;
  final int bootLeftS;
  final Position address;
  final int batteryMv;
  final int inbox;

  const Status({
    this.ready = false,
    this.externalPower = false,
    this.sleepy = false,
    this.carried = false,
    this.mapPoint = false,
    this.placed = false,
    this.booting = false,
    this.anchorless = false,
    this.relays = 0,
    this.bootLeftS = 0,
    this.address = const Position(0, 0),
    this.batteryMv = 0,
    this.inbox = 0,
  });

  Status copyWith({
    bool? ready,
    bool? externalPower,
    bool? sleepy,
    bool? carried,
    bool? mapPoint,
    bool? placed,
    bool? booting,
    bool? anchorless,
    int? relays,
    int? bootLeftS,
    Position? address,
    int? batteryMv,
    int? inbox,
  }) => Status(
    ready: ready ?? this.ready,
    externalPower: externalPower ?? this.externalPower,
    sleepy: sleepy ?? this.sleepy,
    carried: carried ?? this.carried,
    mapPoint: mapPoint ?? this.mapPoint,
    placed: placed ?? this.placed,
    booting: booting ?? this.booting,
    anchorless: anchorless ?? this.anchorless,
    relays: relays ?? this.relays,
    bootLeftS: bootLeftS ?? this.bootLeftS,
    address: address ?? this.address,
    batteryMv: batteryMv ?? this.batteryMv,
    inbox: inbox ?? this.inbox,
  );

  int get _flags => [
    ready,
    externalPower,
    sleepy,
    carried,
    mapPoint,
    placed,
    booting,
    anchorless,
  ].indexed.fold(0, (f, e) => f | ((e.$2 ? 1 : 0) << e.$1));

  Uint8List encode() => Uint8List.fromList([
    _flags,
    relays,
    ..._be16(bootLeftS),
    ...(placed ? address.encode() : List.filled(Position.wireLen, 0)),
    ..._be16(batteryMv),
    inbox,
  ]);

  static Status decode(List<int> b) {
    if (b.length < len) throw const Malformed('Status');
    bool flag(int bit) => b[0] & (1 << bit) != 0;
    return Status(
      ready: flag(0),
      externalPower: flag(1),
      sleepy: flag(2),
      carried: flag(3),
      mapPoint: flag(4),
      placed: flag(5),
      booting: flag(6),
      anchorless: flag(7),
      relays: b[1],
      bootLeftS: _u16(b, 2),
      address: Position.decode(b.sublist(4, 4 + Position.wireLen)),
      batteryMv: _u16(b, 4 + Position.wireLen),
      inbox: b[6 + Position.wireLen],
    );
  }
}

/// Send, write.
Uint8List encodeSend(Address dst, List<int> payload) {
  if (payload.length > maxPayload) throw const Malformed('payload');
  return Uint8List.fromList([...dst.encode(), ...payload]);
}

/// Inbox, read: the oldest message, or null when the inbox is empty. The
/// whole value past the ID is the payload.
class InboxMessage {
  static const header = 4;
  final int msgId;
  final Uint8List payload;
  InboxMessage(this.msgId, List<int> payload)
    : payload = Uint8List.fromList(payload);

  Uint8List encode() => Uint8List.fromList([..._be32(msgId), ...payload]);

  static InboxMessage? decode(List<int> b) {
    if (b.isEmpty) return null;
    if (b.length < header || b.length > header + maxPayload) {
      throw const Malformed('Inbox');
    }
    return InboxMessage(_u32(b, 0), b.sublist(header));
  }
}

/// Inbox, write: the app has this message, the node may forget it.
Uint8List encodeConfirm(int msgId) => Uint8List.fromList(_be32(msgId));

/// Given back, read and notify: null when empty.
class GivenBack {
  static const len = 4 + 2;
  final int msgId;
  final int waitS;
  const GivenBack(this.msgId, this.waitS);

  Uint8List encode() => Uint8List.fromList([..._be32(msgId), ..._be16(waitS)]);

  static GivenBack? decode(List<int> b) {
    if (b.isEmpty) return null;
    if (b.length < len) throw const Malformed('Given back');
    return GivenBack(_u32(b, 0), _u16(b, 4));
  }
}

/// The link version that has Clock.
const int clockVersion = 2;

/// The UTC offsets in use, minutes: UTC−12:00 to UTC+14:00.
const int minOffsetMin = -12 * 60;
const int maxOffsetMin = 14 * 60;

/// Clock, write: the phone's UTC seconds since 1970 and its UTC offset
/// in minutes.
Uint8List encodeClock(int utcS, int offsetMin) {
  if (offsetMin < minOffsetMin || offsetMin > maxOffsetMin) {
    throw const Malformed('offset');
  }
  return Uint8List.fromList([..._be32(utcS), ..._be16(offsetMin & 0xffff)]);
}

/// Where the node's time came from.
enum TimeSource { none, gps, phone }

/// Clock, read: the node's time, the offset it keeps and the source.
class NodeTime {
  static const len = 4 + 2 + 1;
  final int utcS;
  final int offsetMin;
  final TimeSource source;
  const NodeTime(this.utcS, this.offsetMin, this.source);

  Uint8List encode() => Uint8List.fromList([
    ..._be32(source == TimeSource.none ? 0 : utcS),
    ..._be16(offsetMin & 0xffff),
    source.index,
  ]);

  static NodeTime decode(List<int> b) {
    if (b.length < len || b[6] >= TimeSource.values.length) {
      throw const Malformed('Clock');
    }
    final o = _u16(b, 4);
    return NodeTime(
      _u32(b, 0),
      o >= 0x8000 ? o - 0x10000 : o,
      TimeSource.values[b[6]],
    );
  }
}

/// Map point, write: 0 clears it, 1 and a position sets it.
Uint8List encodeMapPoint(Position? p) => p == null
    ? Uint8List.fromList([0])
    : Uint8List.fromList([1, ...p.encode()]);

/// The link version that has Home.
const int homeVersion = 3;

/// Bytes of the mailbox key.
const int mailboxKeyLen = 32;

/// Home, write: 0, the node is nobody's home; 1 and the mailbox key, the
/// person's.
Uint8List encodeHome(List<int>? key) {
  if (key == null) return Uint8List.fromList([0]);
  if (key.length != mailboxKeyLen) throw const Malformed('mailbox key');
  return Uint8List.fromList([1, ...key]);
}

/// Home, read: whether the node holds a mailbox key, and the numbers of
/// the oldest and newest items its ring holds, 0 when none.
class NodeHome {
  static const len = 1 + 3 + 3;
  final bool set;
  final int oldest;
  final int newest;
  const NodeHome(this.set, this.oldest, this.newest);

  Uint8List encode() => Uint8List.fromList([
    set ? 1 : 0,
    ..._be32(oldest).sublist(1),
    ..._be32(newest).sublist(1),
  ]);

  static NodeHome decode(List<int> b) {
    if (b.length < len || b[0] > 1) throw const Malformed('Home');
    int n(int at) => (b[at] << 16) | (b[at + 1] << 8) | b[at + 2];
    return NodeHome(b[0] == 1, n(1), n(4));
  }

  @override
  String toString() => !set
      ? 'not a home'
      : oldest == 0
      ? 'a home, its mailbox empty'
      : 'a home, items $oldest to $newest';
}
