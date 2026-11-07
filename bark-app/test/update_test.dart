// The firmware update over the link: Update's and Image's values, as
// `bark-link`'s tests give them; an image file's checks; where a
// transfer goes on from; and the whole update against the fake node,
// with lost pieces, a lost link, a low battery and the reboot.

import 'dart:typed_data';

import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/updater.dart';
import 'package:crypto/crypto.dart' show sha256;
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

/// An image file as `image.py pack` writes it: version `version`, of
/// `len` bytes that differ from place to place.
Uint8List imageFile(int version, int len, {int seed = 1}) {
  var x = seed * 2654435761 | 1;
  final bytes = [
    for (var i = 0; i < len; i++)
      ((x = (x * 1103515245 + 12345) & 0x7fffffff) >> 16) & 0xff,
  ];
  List<int> le32(int v) => [for (var i = 0; i < 4; i++) (v >> (8 * i)) & 0xff];
  final header = [
    ...'bark'.codeUnits,
    1,
    0,
    0,
    0,
    ...le32(version),
    ...le32(len),
    ...sha256.convert(bytes).bytes,
  ];
  return Uint8List.fromList([
    ...header,
    ...List.filled(FirmwareImage.headerLen - header.length, 0),
    ...bytes,
  ]);
}

void main() {
  test('the values are bark-link\'s', () {
    expect(pieceBytes(attMtu), 240);
    expect(encodePiece(0x01020304, [0xab, 0xab, 0xab]), [
      1, 2, 3, 4, 0xab, 0xab, 0xab, //
    ]);
    expect(() => encodePiece(0, []), throwsA(isA<Malformed>()));
    expect(
      () => encodePiece(0, List.filled(241, 0)),
      throwsA(isA<Malformed>()),
    );
    final header = List.generate(256, (i) => i & 0xff);
    expect(encodeBegin(header), [0, ...header]);
    expect(encodeCommit(), [1]);
    expect(encodeReboot(), [2]);
    final p = UpdateProgress(
      UpdateState.receiving,
      refused: Refusal.battery,
      held: 0x0001e000,
      version: 7,
      hash: List.generate(32, (i) => i),
    );
    final v = p.encode();
    expect(v.sublist(0, 10), [1, 6, 0, 1, 0xe0, 0, 0, 0, 0, 7]);
    expect(v.length, UpdateProgress.len);
    expect(v[41], 31);
    final back = UpdateProgress.decode([...v, 9, 9]);
    expect(back.state, UpdateState.receiving);
    expect(back.refused, Refusal.battery);
    expect(back.held, 0x0001e000);
    expect(back.version, 7);
    expect(back.hash, p.hash);
    expect(
      () => UpdateProgress.decode([4, ...v.sublist(1)]),
      throwsA(isA<Malformed>()),
    );
    expect(
      () => UpdateProgress.decode([1, 8, ...v.sublist(2)]),
      throwsA(isA<Malformed>()),
    );
  });

  test('an image file is checked before it is sent', () {
    final f = imageFile(7, 20001);
    final i = parseImage(f);
    expect(i.version, 7);
    expect(i.length, 20001);
    expect(i.header, f.sublist(0, 256));
    final bad = Uint8List.fromList(f)..[300] ^= 1;
    expect(() => parseImage(bad), throwsA(isA<Malformed>()));
    expect(() => parseImage(f.sublist(0, 20000)), throwsA(isA<Malformed>()));
    expect(
      () => parseImage(Uint8List.fromList(f)..[0] = 0),
      throwsA(isA<Malformed>()),
    );
  });

  test('a transfer goes on from the bytes the node holds of the image', () {
    final i = parseImage(imageFile(7, 20001));
    final other = parseImage(imageFile(7, 20001, seed: 2));
    UpdateProgress at(UpdateState s, int held, FirmwareImage of) =>
        UpdateProgress(s, held: held, version: 7, hash: of.hash);
    expect(next(UpdateProgress(UpdateState.idle), i), Step.begin);
    expect(next(at(UpdateState.receiving, 4800, i), i), Step.send);
    expect(next(at(UpdateState.receiving, 4800, other), i), Step.begin);
    expect(next(at(UpdateState.staged, 20001, i), i), Step.commit);
    expect(next(at(UpdateState.committed, 20001, i), i), Step.reboot);
  });

  test('the image asset of a release', () {
    final release = <String, dynamic>{
      'tag_name': 'v7',
      'assets': [
        {'name': 'bark-7.apk', 'browser_download_url': 'https://x/apk'},
        {'name': 'bark-fw-7.uf2', 'browser_download_url': 'https://x/uf2'},
        {'name': 'bark-fw-7.img', 'browser_download_url': 'https://x/img'},
      ],
    };
    expect(imageAsset(release), (name: 'bark-fw-7.img', url: 'https://x/img'));
    expect(imageAsset({'assets': <dynamic>[]}), isNull);
  });

  group('against the fake node', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());
    final start = DateTime.utc(2026, 11, 7);

    void run(void Function(FakeAsync, Session, FakeNode, Updater) body) {
      fakeAsync((async) {
        DateTime now() => start.add(async.elapsed);
        final s = Session(store: MemoryStore(), now: now);
        final node = FakeNode(
          now: now,
          sodium: sodium,
          app: List.filled(32, 7),
        );
        s.attach(node.connect());
        async.flushMicrotasks();
        final u = Updater(s, () => attMtu);
        body(async, s, node, u);
        u.dispose();
        node.dispose();
        s.dispose();
      });
    }

    test('an image is sent, committed, and runs after the reboot', () {
      run((async, s, node, u) {
        final file = imageFile(2, 20001);
        u.choose(parseImage(file), 'bark-fw-2.img');
        // Every fifth piece lost, as by a host whose queue is full.
        node.dropEvery = 5;
        u.send();
        async.flushMicrotasks();
        expect(u.phase, Phase.committed, reason: u.message);
        expect(u.held, 20001);
        u.reboot();
        async.flushMicrotasks();
        expect(u.phase, Phase.rebooting);
        expect(s.connected, isFalse);
        // Still swapping: no connection.
        expect(() => node.connect(), throwsStateError);
        async.elapse(FakeNode.swapTime);
        s.attach(node.connect());
        async.flushMicrotasks();
        expect(u.phase, Phase.done, reason: u.message);
        expect(s.node!.image, 2);
      });
    });

    test('a lost link goes on from what the node holds', () {
      run((async, s, node, u) {
        final image = parseImage(imageFile(2, 20001));
        final pieces = (image.length / pieceBytes(attMtu)).ceil();
        u.choose(image, 'file');
        node.closeAfterPieces = 40;
        u.send();
        async.flushMicrotasks();
        expect(u.phase, Phase.paused);
        expect(s.connected, isFalse);
        expect(u.held, greaterThan(0));
        s.attach(node.connect());
        async.flushMicrotasks();
        expect(u.phase, Phase.committed, reason: u.message);
        // Each piece once, but the window cut short by the lost link.
        expect(node.piecesWritten, lessThan(pieces + window));
      });
    });

    test('on a low battery the commit waits, the image kept', () {
      run((async, s, node, u) {
        node.lowBattery = true;
        u.choose(parseImage(imageFile(2, 5000)), 'file');
        u.send();
        async.flushMicrotasks();
        expect(u.phase, Phase.staged);
        expect(u.message, contains(Refusal.battery.why));
        node.lowBattery = false;
        u.commit();
        async.flushMicrotasks();
        expect(u.phase, Phase.committed, reason: u.message);
      });
    });

    test('an image not newer than the node\'s is refused', () {
      run((async, s, node, u) {
        u.choose(parseImage(imageFile(1, 5000)), 'file');
        u.send();
        async.flushMicrotasks();
        expect(u.phase, Phase.failed);
        expect(u.message, contains(Refusal.version.why));
      });
    });
  });
}
