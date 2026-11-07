// The byte vectors of bark-link/src/tests.rs, test `vectors` and its
// neighbours, so that the app reads and writes what the node does; and
// how a bonded phone tells that the node no longer holds its bond.

import 'package:bark_app/ble_link.dart';
import 'package:bark_app/link.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:universal_ble/universal_ble.dart';

void main() {
  final kyiv = Position.fromDegrees(50.45, 30.52);
  final to = [1, 2, 3, 4, 5, 6];
  final hello = 'hello'.codeUnits;

  test('position encodes as the radio does', () {
    expect(kyiv.encode(), [0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80]);
    expect(Position.decode(kyiv.encode()), kyiv);
  });

  test('position round-trips at the ends of its range', () {
    for (final (lat, lon) in [
      (0.0, 0.0),
      (-89.9, 179.9),
      (89.9, -179.9),
      (-90.0, -180.0),
    ]) {
      final p = Position.fromDegrees(lat, lon);
      expect(Position.decode(p.encode()), p);
      expect((p.latDeg - lat).abs() <= Position.unitDeg, isTrue);
      expect((p.lonDeg - lon).abs() <= Position.unitDeg, isTrue);
    }
    // Clamped, as the node clamps.
    final top = Position.fromDegrees(90.0, 180.0);
    expect(top.lat, (1 << 24) - 1);
    expect(top.lon, (1 << 25) - 1);
  });

  test('send and its message ID', () {
    final dst = Address(kyiv, to);
    expect(encodeSend(dst, hello), [
      0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, 1, 2, 3, 4, 5, 6, //
      ...hello,
    ]);
    expect(msgId(dst, hello), 0x357b6010);
    expect(
      () => encodeSend(dst, List.filled(maxPayload + 1, 0)),
      throwsA(isA<Malformed>()),
    );
    expect(encodeSend(dst, List.filled(maxPayload, 0)).length, 240);
  });

  test('FNV-1a as published', () {
    int fnv(List<int> b) =>
        b.fold(0x811c9dc5, (h, x) => ((h ^ x) * 0x01000193) & 0xffffffff);
    expect(fnv([]), 0x811c9dc5);
    expect(fnv('a'.codeUnits), 0xe40c292c);
    expect(fnv('foobar'.codeUnits), 0xbf9cf968);
    final dst = Address(kyiv, to);
    expect(msgId(dst, hello), fnv([...dst.encode(), ...hello]));
  });

  test('status', () {
    final s = Status(
      ready: true,
      externalPower: true,
      placed: true,
      booting: true,
      relays: 3,
      bootLeftS: 100,
      address: kyiv,
      inbox: 2,
    );
    final bytes = [
      0x63, 3, 0, 100, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, 0, 0, 2, //
    ];
    expect(s.encode(), bytes);
    final d = Status.decode(bytes);
    expect(d.ready && d.externalPower && d.placed && d.booting, isTrue);
    expect(d.sleepy || d.carried || d.mapPoint || d.anchorless, isFalse);
    expect([d.relays, d.bootLeftS, d.batteryMv, d.inbox], [3, 100, 0, 2]);
    expect(d.address, kyiv);
    // Each flag is its own bit.
    for (var bit = 0; bit < 8; bit++) {
      final one = Status.decode([1 << bit, ...List.filled(13, 0)]);
      expect(one.encode()[0], 1 << bit);
    }
  });

  test('status ignores what a later version appends', () {
    final s = Status(relays: 9).encode();
    expect(Status.decode([...s, 0xaa, 0xaa, 0xaa]).relays, 9);
    expect(() => Status.decode(s.sublist(0, 13)), throwsA(isA<Malformed>()));
  });

  test('given back', () {
    expect(const GivenBack(0x357b6010, 200).encode(), [
      0x35, 0x7b, 0x60, 0x10, 0, 200, //
    ]);
    final g = GivenBack.decode([0x35, 0x7b, 0x60, 0x10, 0, 200, 0xaa])!;
    expect([g.msgId, g.waitS], [0x357b6010, 200]);
    expect(GivenBack.decode([]), isNull);
    expect(() => GivenBack.decode([1, 2, 3]), throwsA(isA<Malformed>()));
  });

  test('node', () {
    final n = NodeInfo(linkVersion, to, 7);
    expect(n.encode(), [4, 1, 2, 3, 4, 5, 6, 0, 0, 0, 7]);
    final d = NodeInfo.decode([1, 1, 2, 3, 4, 5, 6, 0, 0, 0, 7, 0xaa]);
    expect([d.version, d.image], [1, 7]);
    expect(d.id, to);
    expect(nodeName(to), 'bark-010203040506');
  });

  test('inbox and confirm', () {
    expect(InboxMessage.decode([]), isNull);
    expect(() => InboxMessage.decode([1, 2, 3]), throwsA(isA<Malformed>()));
    final m = InboxMessage.decode([0x35, 0x7b, 0x60, 0x10, ...hello])!;
    expect(m.msgId, 0x357b6010);
    expect(m.payload, hello);
    expect(InboxMessage.decode([0, 0, 0, 1])!.payload, isEmpty);
    expect(encodeConfirm(0x357b6010), [0x35, 0x7b, 0x60, 0x10]);
  });

  test('clock', () {
    // 2026-11-05 12:35:19 UTC, at UTC+01:00 and UTC−03:30.
    expect(encodeClock(1793882119, 60), [0x6a, 0xec, 0x78, 0x07, 0, 60]);
    expect(encodeClock(1793882119, -210), [0x6a, 0xec, 0x78, 0x07, 0xff, 0x2e]);
    expect(() => encodeClock(0, 841), throwsA(isA<Malformed>()));
    expect(encodeClock(0, -720), [0, 0, 0, 0, 0xfd, 0x30]);
    const t = NodeTime(1793882119, 60, TimeSource.gps);
    expect(t.encode(), [0x6a, 0xec, 0x78, 0x07, 0, 60, 1]);
    expect(const NodeTime(1793882119, 60, TimeSource.none).encode(), [
      0, 0, 0, 0, 0, 60, 0, //
    ]);
    final d = NodeTime.decode([0x6a, 0xec, 0x78, 0x07, 0xff, 0x2e, 2, 0xaa]);
    expect(
      [d.utcS, d.offsetMin, d.source],
      [1793882119, -210, TimeSource.phone],
    );
    expect(
      () => NodeTime.decode([0, 0, 0, 0, 0, 0]),
      throwsA(isA<Malformed>()),
    );
    expect(
      () => NodeTime.decode([0, 0, 0, 0, 0, 0, 3]),
      throwsA(isA<Malformed>()),
    );
  });

  test('map point', () {
    expect(encodeMapPoint(null), [0]);
    expect(encodeMapPoint(kyiv), [1, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80]);
  });

  test('address as hex', () {
    final a = Address(kyiv, to);
    expect(a.toString(), '47c0490ad9ff80010203040506');
    final p = Address.parse('47c0490a d9ff80:01-02-03-04-05-06')!;
    expect(p.pos, kyiv);
    expect(p.id, to);
    expect(Address.parse('47c0490ad9ff8001020304050'), isNull);
    expect(Address.parse('zz'), isNull);
  });

  // bark-link/src/home.rs, test `vectors`.
  test('home', () {
    expect(encodeHome(null), [0]);
    final k = List.generate(mailboxKeyLen, (i) => i);
    expect(encodeHome(k), [1, ...k]);
    expect(() => encodeHome([1, 2]), throwsA(isA<Malformed>()));
    final h = NodeHome.decode([1, 0, 0, 17, 1, 2, 3, 9]);
    expect((h.set, h.oldest, h.newest), (true, 17, 0x010203));
    expect(h.encode(), [1, 0, 0, 17, 1, 2, 3]);
    expect(
      () => NodeHome.decode([2, 0, 0, 0, 0, 0, 0]),
      throwsA(isA<Malformed>()),
    );
    expect(() => NodeHome.decode([1]), throwsA(isA<Malformed>()));
  });

  // bark-link/src/neighbours.rs, tests `vectors`, `a_page_fills_one_pdu`,
  // `reads_ignore_what_a_later_version_appends`, `quality_follows_the_margin`.
  test('neighbours', () {
    expect(NeighbourPage.perPage, 13);
    expect(NeighbourPage.header + 13 * Neighbour.len, 244);
    expect(encodeNeighboursFrom(7), [7]);
    final n = Neighbour(
      to,
      kyiv,
      rssiDdbm: -1105,
      ageS: 300,
      sleepy: true,
      anchor: true,
      proven: true,
    );
    final b = NeighbourPage(-118, 20, kyiv, [n]).encode();
    expect(b, [
      0x8a, 20, 18, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, //
      1, 2, 3, 4, 5, 6, 0x47, 0xc0, 0x49, 0x0a, 0xd9, 0xff, 0x80, //
      0xfb, 0xaf, 0x01, 0x2c, 0x91,
    ]);
    final p = NeighbourPage.decode(b);
    expect((p.floorDbm, p.next, p.own), (-118, 20, kyiv));
    final d = p.neighbours.single;
    expect(d.id, to);
    expect((d.position, d.rssiDdbm, d.ageS), (kyiv, -1105, 300));
    expect((d.sleepy, d.anchor, d.proven, d.banned), (true, true, true, false));
    // A later version's neighbour, three bytes longer, read the same.
    final longer = [...b.sublist(0, 2), 21, ...b.sublist(3), 0xaa, 0xaa, 0xaa];
    expect(NeighbourPage.decode(longer).neighbours.single.rssiDdbm, -1105);
    expect(
      () => NeighbourPage.decode(b.sublist(0, 27)),
      throwsA(isA<Malformed>()),
    );
    expect(
      () => NeighbourPage.decode(b.sublist(0, 9)),
      throwsA(isA<Malformed>()),
    );
    Quality q(int r) => Neighbour(to, kyiv, rssiDdbm: r, ageS: 0).quality(-118);
    expect(q(-1140), Quality.fragile);
    expect(q(-1139), Quality.fair);
    expect(q(-1112), Quality.fair);
    expect(q(-1110), Quality.good);
  });

  test('a bonded phone\'s read the node refuses for want of its bond', () {
    UniversalBleException read(int status) => UniversalBleException(
      code: UniversalBleErrorCode.failed,
      message: 'Failed to read',
      details: '$status',
    );
    // Insufficient authentication, insufficient encryption, Android's
    // authentication failure.
    for (final status in [0x05, 0x0F, 0x89]) {
      expect(bondRefused(read(status), closed: false), isTrue);
    }
    // Any other failure is not the bond's, unless the link closed under it.
    expect(bondRefused(read(0x85), closed: false), isFalse);
    expect(bondRefused(StateError('timeout'), closed: false), isFalse);
    expect(bondRefused(StateError('timeout'), closed: true), isTrue);
  });
}
