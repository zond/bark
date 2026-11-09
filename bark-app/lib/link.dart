// The phone link's values (spec "Phone link"), in the encodings the
// node's `bark-link` crate uses, and the radio's encodings of positions,
// node IDs and message IDs (spec "Packet formats"). Every integer is
// big-endian.

import 'dart:typed_data';

/// The link version this app speaks.
const int linkVersion = 10;

/// The ATT MTU the node offers and the app asks for (spec "Phone link",
/// "MTU").
const int attMtu = 247;

/// The ATT MTU every connection starts with (Bluetooth Core
/// Specification 5.0, Vol 3 Part F §3.2.8).
const int defaultAttMtu = 23;

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

  /// From link version 4.
  static const neighbours = '28bc000a-a1b8-4893-9537-2ee3559d7644';

  /// From link version 6.
  static const update = '28bc000b-a1b8-4893-9537-2ee3559d7644';
  static const image = '28bc000c-a1b8-4893-9537-2ee3559d7644';

  /// From link version 7.
  static const recipient = '28bc000d-a1b8-4893-9537-2ee3559d7644';
  static const public = '28bc000e-a1b8-4893-9537-2ee3559d7644';

  /// From link version 8, write only: a guest reads its items through
  /// [inbox].
  static const guest = '28bc000f-a1b8-4893-9537-2ee3559d7644';

  /// From link version 9, the paired phone's alone.
  static const ask = '28bc0010-a1b8-4893-9537-2ee3559d7644';

  /// What a public node advertises in place of [service], which no
  /// attribute has (spec "Phone link", Public).
  static const publicNode = '28bc1001-a1b8-4893-9537-2ee3559d7644';
}

/// ATT error codes the node answers a write with.
class AttError {
  /// A characteristic that needs the bond, on a link without it.
  static const insufficientAuthentication = 0x05;
  static const invalidLength = 0x0D;
  static const valueNotAllowed = 0x13;

  /// The node takes no message now; wait for ready.
  static const notReady = 0x80;

  /// The node refused an update's begin, commit or reboot; Update says
  /// why.
  static const refused = 0x81;
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

/// A node's name as a person sees it: its ID in capitals, as the node's
/// own screen shows it, `ID FA4E00000001`; a name not a node's as it is.
String shownName(String name) {
  final m = RegExp(r'^bark-([0-9a-f]{12})$').firstMatch(name);
  return m == null ? name : 'ID ${m[1]!.toUpperCase()}';
}

/// The node of `id` as a person sees it ([shownName]).
String nodeLabel(List<int> id) => shownName(nodeName(id));

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

/// The link version that has Neighbours.
const int neighboursVersion = 4;

/// Neighbours, write: read the table from this slot on.
Uint8List encodeNeighboursFrom(int slot) => Uint8List.fromList([slot & 0xff]);

/// How well a link carries a full-length frame (spec "Phone link",
/// Neighbours).
enum Quality { fragile, fair, good }

/// The margins over the noise floor, dB, from which a link is fair and
/// good: a full frame through a fade of one and of two standard
/// deviations, as `bark-link` `neighbours` derives them.
const double fairAboveFloorDb = 4.08;
const double goodAboveFloorDb = 6.92;

/// One neighbour of the node, as Neighbours gives it.
class Neighbour {
  static const len = nodeIdLen + Position.wireLen + 2 + 2 + 1;
  final Uint8List id;
  final Position position;

  /// The averaged RSSI of its link, tenths of a dBm.
  final int rssiDdbm;

  /// Seconds since it was last heard.
  final int ageS;
  final bool sleepy;
  final bool mapPoint;
  final bool demoted;
  final bool banned;

  /// This sleepy node's anchor, the relay it sends and receives through.
  final bool anchor;
  final bool carried;
  final bool anchorless;

  /// It has taken a message from this node, so it hears this node too.
  final bool proven;

  Neighbour(
    List<int> id,
    this.position, {
    required this.rssiDdbm,
    required this.ageS,
    this.sleepy = false,
    this.mapPoint = false,
    this.demoted = false,
    this.banned = false,
    this.anchor = false,
    this.carried = false,
    this.anchorless = false,
    this.proven = false,
  }) : id = Uint8List.fromList(id);

  double get rssiDbm => rssiDdbm / 10;

  Neighbour withAge(int ageS) => Neighbour(
    id,
    position,
    rssiDdbm: rssiDdbm,
    ageS: ageS,
    sleepy: sleepy,
    mapPoint: mapPoint,
    demoted: demoted,
    banned: banned,
    anchor: anchor,
    carried: carried,
    anchorless: anchorless,
    proven: proven,
  );

  int get _flags => [
    sleepy,
    mapPoint,
    demoted,
    banned,
    anchor,
    carried,
    anchorless,
    proven,
  ].indexed.fold(0, (f, e) => f | ((e.$2 ? 1 : 0) << e.$1));

  List<int> encode() => [
    ...id,
    ...position.encode(),
    ..._be16(rssiDdbm & 0xffff),
    ..._be16(ageS),
    _flags,
  ];

  static Neighbour decode(List<int> b) {
    bool flag(int bit) => b[nodeIdLen + Position.wireLen + 4] & (1 << bit) != 0;
    final r = _u16(b, nodeIdLen + Position.wireLen);
    return Neighbour(
      b.sublist(0, nodeIdLen),
      Position.decode(b.sublist(nodeIdLen)),
      rssiDdbm: r >= 0x8000 ? r - 0x10000 : r,
      ageS: _u16(b, nodeIdLen + Position.wireLen + 2),
      sleepy: flag(0),
      mapPoint: flag(1),
      demoted: flag(2),
      banned: flag(3),
      anchor: flag(4),
      carried: flag(5),
      anchorless: flag(6),
      proven: flag(7),
    );
  }

  /// The link's quality at a node whose noise floor is `floorDbm`.
  Quality quality(int floorDbm) {
    final margin = rssiDbm - floorDbm;
    if (margin >= goodAboveFloorDb) return Quality.good;
    if (margin >= fairAboveFloorDb) return Quality.fair;
    return Quality.fragile;
  }
}

/// Neighbours, read: one page of the table.
class NeighbourPage {
  static const header = 1 + 1 + 1 + Position.wireLen;

  /// Neighbours in a full page, as many as fit one PDU at [attMtu].
  static const perPage = (attMtu - 3 - header) ~/ Neighbour.len;

  /// The node's noise floor, dBm.
  final int floorDbm;

  /// The slot to write for the next page; 0 at the table's end.
  final int next;

  /// The node's own position; zero while it has none.
  final Position own;
  final List<Neighbour> neighbours;

  const NeighbourPage(this.floorDbm, this.next, this.own, this.neighbours);

  Uint8List encode() {
    if (neighbours.length > perPage) throw const Malformed('Neighbours');
    return Uint8List.fromList([
      floorDbm & 0xff,
      next,
      Neighbour.len,
      ...own.encode(),
      for (final n in neighbours) ...n.encode(),
    ]);
  }

  /// Each neighbour is as long as the value says, so that the fields a
  /// later version appends are skipped.
  static NeighbourPage decode(List<int> b) {
    if (b.length < header) throw const Malformed('Neighbours');
    final each = b[2];
    if (each < Neighbour.len || (b.length - header) % each != 0) {
      throw const Malformed('Neighbours');
    }
    return NeighbourPage(
      b[0] >= 0x80 ? b[0] - 0x100 : b[0],
      b[1],
      Position.decode(b.sublist(3)),
      [
        for (var at = header; at < b.length; at += each)
          Neighbour.decode(b.sublist(at, at + each)),
      ],
    );
  }
}

/// The link version that has Update and Image.
const int updateVersion = 6;

/// A firmware image as `bark-fw/scripts/image.py pack` writes it: a
/// header of 256 bytes, then the image (`bark-fw/README.md`, "Updates").
class FirmwareImage {
  static const headerLen = 256;
  static const hashLen = 32;
  static final _magic = 'bark'.codeUnits;
  static const _format = 1;

  final Uint8List header;
  final Uint8List bytes;
  final int version;
  final Uint8List hash;

  FirmwareImage._(this.header, this.bytes, this.version, this.hash);

  int get length => bytes.length;

  /// The image in `file`, if it is one: its header's magic and format,
  /// and a length that is the file's. `sha256` hashes the image's
  /// bytes, which must be the header's hash.
  static FirmwareImage parse(
    List<int> file,
    List<int> Function(List<int>) sha256,
  ) {
    if (file.length <= headerLen) throw const Malformed('image: too short');
    for (var i = 0; i < _magic.length; i++) {
      if (file[i] != _magic[i]) throw const Malformed('image: not one');
    }
    if (file[4] != _format) throw const Malformed('image: format');
    int le32(int at) =>
        file[at] |
        (file[at + 1] << 8) |
        (file[at + 2] << 16) |
        (file[at + 3] << 24);
    final version = le32(8);
    final length = le32(12);
    if (length != file.length - headerLen) {
      throw const Malformed('image: length');
    }
    final bytes = Uint8List.fromList(file.sublist(headerLen));
    final hash = Uint8List.fromList(file.sublist(16, 16 + hashLen));
    if (hex(sha256(bytes)) != hex(hash)) throw const Malformed('image: hash');
    return FirmwareImage._(
      Uint8List.fromList(file.sublist(0, headerLen)),
      bytes,
      version,
      hash,
    );
  }
}

/// Update, write: begin with the image's header.
Uint8List encodeBegin(List<int> header) {
  if (header.length != FirmwareImage.headerLen) {
    throw const Malformed('header');
  }
  return Uint8List.fromList([0, ...header]);
}

/// Update, write: commit the image held.
Uint8List encodeCommit() => Uint8List.fromList([1]);

/// Update, write: reboot, to let the loader swap.
Uint8List encodeReboot() => Uint8List.fromList([2]);

/// Bytes before the image's in a piece.
const int pieceHeader = 4;

/// The image's bytes one write carries at `mtu`: a write spends 3 bytes
/// on its opcode and handle, and a piece 4 on its offset; 240 at the
/// node's MTU of 247.
int pieceBytes(int mtu) => mtu - 3 - pieceHeader;

/// Image, write: the image's bytes from `offset`.
Uint8List encodePiece(int offset, List<int> bytes) {
  if (bytes.isEmpty || bytes.length > pieceBytes(attMtu)) {
    throw const Malformed('piece');
  }
  return Uint8List.fromList([..._be32(offset), ...bytes]);
}

/// Where the node's update stands.
enum UpdateState { idle, receiving, staged, committed }

/// Why the node refused a begin, a commit or a reboot.
enum Refusal {
  header('the file is not an image this node reads'),
  version('the image is not newer than the firmware the node runs'),
  busy('the node has an image committed already: reboot it first'),
  incomplete('the node does not hold every byte of the image'),
  hash('the bytes the node holds are not the image\'s; send it again'),
  battery(
    'the node is on a low battery; it keeps the image, to commit once '
    'it is on USB or charged',
  ),
  flash('the node\'s flash failed'),
  signature(
    'the image is not signed by a key the node holds: an image built '
    'here is signed with `image.py sign`, a release\'s is signed already',
  );

  final String why;
  const Refusal(this.why);
}

/// Update, read.
class UpdateProgress {
  static const len = 1 + 1 + 4 + 4 + FirmwareImage.hashLen;
  final UpdateState state;
  final Refusal? refused;

  /// The image's bytes the node holds, in order: the next piece starts
  /// here.
  final int held;
  final int version;
  final Uint8List hash;

  UpdateProgress(
    this.state, {
    this.refused,
    this.held = 0,
    this.version = 0,
    List<int>? hash,
  }) : hash = Uint8List.fromList(hash ?? List.filled(FirmwareImage.hashLen, 0));

  /// Whether the node is staging, or has staged, `image`.
  bool holds(FirmwareImage image) =>
      state != UpdateState.idle && hex(hash) == hex(image.hash);

  Uint8List encode() => Uint8List.fromList([
    state.index,
    refused == null ? 0 : refused!.index + 1,
    ..._be32(held),
    ..._be32(version),
    ...hash,
  ]);

  static UpdateProgress decode(List<int> b) {
    if (b.length < len ||
        b[0] >= UpdateState.values.length ||
        b[1] > Refusal.values.length) {
      throw const Malformed('Update');
    }
    return UpdateProgress(
      UpdateState.values[b[0]],
      refused: b[1] == 0 ? null : Refusal.values[b[1] - 1],
      held: _u32(b, 2),
      version: _u32(b, 6),
      hash: b.sublist(10, len),
    );
  }
}

/// The link version that has Recipient and Public.
const int publicVersion = 7;

/// Recipient, write: the phone's recipient tag at this node.
Uint8List encodeRecipient(int tag) {
  if (tag < 0 || tag > 0xff) throw const Malformed('recipient tag');
  return Uint8List.fromList([tag]);
}

/// Public, write: whether the node serves phones that have not paired.
Uint8List encodePublic(bool public) => Uint8List.fromList([public ? 1 : 0]);

/// Public, read.
bool decodePublic(List<int> b) {
  if (b.length != 1 || b[0] > 1) throw const Malformed('Public');
  return b[0] == 1;
}

/// The link version that has Guest.
const int guestVersion = 8;

/// Bytes of an item's number in the ring.
const int itemNumberLen = 3;

/// The largest number an item takes.
const int maxItemNumber = (1 << (8 * itemNumberLen)) - 1;

/// Guest, write: the number of the last item the guest took; the next
/// read gives the oldest item after it with the guest's recipient tag.
Uint8List encodeAfter(int number) {
  if (number < 0 || number > maxItemNumber) throw const Malformed('number');
  return Uint8List.fromList(_be32(number).sublist(1));
}

/// Inbox, read on a guest's link: an item of the node's ring for the
/// guest, its number, the message ID it arrived with and its payload;
/// null when none waits after the number written to Guest.
class GuestItem {
  static const header = itemNumberLen + 4;
  final int number;
  final InboxMessage message;
  GuestItem(this.number, this.message);

  Uint8List encode() =>
      Uint8List.fromList([...encodeAfter(number), ...message.encode()]);

  static GuestItem? decode(List<int> b) {
    if (b.isEmpty) return null;
    if (b.length < header || b.length > header + maxPayload) {
      throw const Malformed('Guest');
    }
    return GuestItem(
      (b[0] << 16) | (b[1] << 8) | b[2],
      InboxMessage(_u32(b, itemNumberLen), b.sublist(header)),
    );
  }
}

/// The link version that has Ask.
const int askVersion = 9;

/// Ask, write: the neighbour, by its node ID, that the node asks for its
/// status.
Uint8List encodeAsk(List<int> id) {
  if (id.length != nodeIdLen) throw const Malformed('node ID');
  return Uint8List.fromList(id);
}

/// What a neighbour's own screen shows, as it answers an ask (spec
/// "Phone link", Ask).
class NeighbourStatus {
  static const len = 1 + 1 + 1 + 1 + 1 + 4 + 4 + 1 + 1 + 1;

  /// Its bytes before the image gathered, as link version 9 had them; a
  /// notification at the default MTU also carries no more of it.
  static const len9 = 1 + 1 + 1 + 1 + 1 + 4;

  /// The battery byte at or below the level at which a fixed relay turns
  /// sleepy.
  static const batteryLow = 0;

  /// The highest battery byte that is a percentage.
  static const batteryFull = 100;

  /// The battery byte while the level has not been read, and on USB.
  static const batteryNotRead = 255;

  final bool placed;
  final bool sleepy;

  /// Sleepy, with a relay that holds its mail.
  final bool anchored;
  final bool carried;
  final bool usb;

  /// The battery byte as sent; any byte from 101 to 254 means not read.
  final int battery;

  /// Neighbours whose heartbeats it hears now.
  final int heard;

  /// Of them, those heard loudly enough that a full frame reaches it
  /// through a fade.
  final int strong;

  /// Of them, those that have taken a message from it.
  final int proven;

  /// The firmware image's version it runs; 0 when unknown.
  final int version;

  /// The image it gathers over the mesh, 0 when none: its version, its
  /// blocks, the blocks it holds from the first and the pieces it holds
  /// of the next.
  final int gathers;
  final int blocks;
  final int complete;
  final int have;

  const NeighbourStatus({
    this.placed = false,
    this.sleepy = false,
    this.anchored = false,
    this.carried = false,
    this.usb = false,
    this.battery = batteryNotRead,
    this.heard = 0,
    this.strong = 0,
    this.proven = 0,
    this.version = 0,
    this.gathers = 0,
    this.blocks = 0,
    this.complete = 0,
    this.have = 0,
  });

  /// The percentage left; null while low, not read or on USB, as the
  /// reading on USB is the charger's.
  int? get percent =>
      !usb && battery > batteryLow && battery <= batteryFull ? battery : null;

  int get _flags => [
    placed,
    sleepy,
    anchored,
    carried,
    usb,
  ].indexed.fold(0, (f, e) => f | ((e.$2 ? 1 : 0) << e.$1));

  Uint8List encode() => Uint8List.fromList([
    _flags,
    battery,
    heard,
    strong,
    proven,
    ..._be32(version),
    ..._be32(gathers),
    blocks,
    complete,
    have,
  ]);

  /// The flags' spare bits, for a later version, are ignored; without the
  /// image gathered whole it gathers none.
  static NeighbourStatus decode(List<int> b) {
    if (b.length < len9) throw const Malformed('neighbour status');
    bool flag(int bit) => b[0] & (1 << bit) != 0;
    final whole = b.length >= len;
    return NeighbourStatus(
      placed: flag(0),
      sleepy: flag(1),
      anchored: flag(2),
      carried: flag(3),
      usb: flag(4),
      battery: b[1],
      heard: b[2],
      strong: b[3],
      proven: b[4],
      version: _u32(b, 5),
      gathers: whole ? _u32(b, 9) : 0,
      blocks: whole ? b[13] : 0,
      complete: whole ? b[14] : 0,
      have: whole ? b[15] : 0,
    );
  }

  /// What it is, in the words of its screen's first line.
  String get role => sleepy
      ? (anchored ? 'SLEEPY ANCHORED' : 'SLEEPY NO ANCHOR')
      : !placed
      ? 'RELAY NO PLACE'
      : carried
      ? 'CARRIED RELAY'
      : 'FIXED RELAY';

  /// Its power, as its screen says it.
  String get power {
    if (usb) return 'POWER USB';
    if (battery == batteryLow) return 'BATTERY LOW';
    final p = percent;
    return p == null ? 'BATTERY NOT READ' : 'BATTERY $p%';
  }

  /// The neighbours it knows, as its screen says them.
  String get known => 'KNOWN $heard/$strong/$proven';

  /// The image it gathers, as its screen says it.
  String get getting => 'GET V$gathers $complete/$blocks';

  /// The lines of its screen that an answer carries; the version only
  /// when known, and the image gathered only when there is one.
  List<String> get lines => [
    role,
    power,
    known,
    if (version != 0) 'V$version',
    if (gathers != 0) getting,
  ];
}

/// Ask, read and notify: the latest answer, a neighbour's ID and status;
/// null before the first since the node booted.
class Answer {
  static const len = nodeIdLen + NeighbourStatus.len;
  final Uint8List id;
  final NeighbourStatus status;
  Answer(List<int> id, this.status) : id = Uint8List.fromList(id);

  Uint8List encode() => Uint8List.fromList([...id, ...status.encode()]);

  static Answer? decode(List<int> b) {
    if (b.isEmpty) return null;
    if (b.length < nodeIdLen + NeighbourStatus.len9) {
      throw const Malformed('Ask');
    }
    return Answer(
      b.sublist(0, nodeIdLen),
      NeighbourStatus.decode(b.sublist(nodeIdLen)),
    );
  }
}
