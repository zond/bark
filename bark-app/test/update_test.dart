// The firmware update over the link: Update's and Image's values, as
// `bark-link`'s tests give them; an image file's checks and the name a
// picked one is shown by; where a transfer goes on from; and the whole
// update against the fake node, with lost pieces, a lost link, a low
// battery and the reboot, its answer lost to the link's end or not; what
// the finished update shows, whether the node hears anyone after it, and
// when the latest release is offered; and the screen of a finished
// update.

import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/releases.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/common.dart' show health;
import 'package:bark_app/ui/update_screen.dart';
import 'package:bark_app/updater.dart';
import 'package:crypto/crypto.dart' show sha256;
import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart' hide Step;
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

/// An image file as `image.py pack` writes it: version `version`, of
/// `len` bytes that differ from place to place, and a signature slot that
/// names key 1 unless `unsigned`.
Uint8List imageFile(
  int version,
  int len, {
  int seed = 1,
  bool unsigned = false,
}) {
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
    if (!unsigned) ...[1, ...List.filled(64, 0x5a)],
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
      () => UpdateProgress.decode([1, 9, ...v.sublist(2)]),
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

  test('a picked image is shown by its name, with .img', () {
    expect(pickedName('v2.img'), 'v2.img');
    expect(pickedName('v2.bin'), 'v2.img');
    expect(pickedName('bark-fw-2'), 'bark-fw-2.img');
    expect(pickedName(''), 'a picked file');
  });

  group('against the fake node', () {
    late Sodium sodium;
    setUpAll(() async => sodium = await SodiumInit.init());
    final start = DateTime.utc(2026, 11, 7);
    // The latest release the updater knows of.
    Release? known;
    setUp(() => known = null);

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
        final u = Updater(s, () => attMtu, release: () => known);
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

    /// An update to version 2, sent, rebooted into and back.
    void update(FakeAsync async, Session s, FakeNode node, Updater u) {
      u.choose(parseImage(imageFile(2, 5000)), 'bark-fw-2.img');
      u.send();
      async.flushMicrotasks();
      u.reboot();
      async.flushMicrotasks();
      async.elapse(FakeNode.swapTime);
      s.attach(node.connect());
      async.flushMicrotasks();
      expect(u.phase, Phase.done, reason: u.message);
    }

    test('a node that hears no one after its update is warned of', () {
      run((async, s, node, u) {
        update(async, s, node, u);
        expect(u.hearsNoOne, isFalse);
        async.elapse(hearsNoOneAfter - const Duration(seconds: 1));
        expect(u.hearsNoOne, isFalse);
        var told = 0;
        u.addListener(() => told++);
        async.elapse(const Duration(seconds: 1));
        expect(told, greaterThan(0));
        expect(u.hearsNoOne, isTrue);
        final warning = u.hearsNoOneWarning(u.updated!);
        expect(warning, contains('bark-fw-1.uf2'));
        expect(warning, contains('over USB'));
        u.dismissHearsNoOne();
        expect(u.hearsNoOne, isFalse);
      });
    });

    test('a node that hears relays after its update is not', () {
      run((async, s, node, u) {
        node.hearsAfterUpdate = true;
        update(async, s, node, u);
        async.elapse(hearsNoOneAfter);
        expect(s.status!.relays, FakeNode.relaysHeard);
        expect(u.hearsNoOne, isFalse);
      });
    });

    test('a node that hears only sleepy nodes after its update is not', () {
      run((async, s, node, u) {
        node.hearsAfterUpdate = true;
        node.onlySleepyAfterUpdate = true;
        update(async, s, node, u);
        async.elapse(hearsNoOneAfter);
        expect(s.status!.relays, 0);
        expect(s.status!.heard, FakeNode.relaysHeard);
        expect(u.hearsNoOne, isFalse);
        expect(health(s), 'Hears ${FakeNode.relaysHeard} nodes');
      });
    });

    test('a finished update shows the versions, not the image', () {
      run((async, s, node, u) {
        expect(u.updated, isNull);
        u.choose(parseImage(imageFile(2, 5000)), 'bark-fw-2.img');
        expect(u.showsImage, isTrue);
        u.send();
        async.flushMicrotasks();
        u.reboot();
        async.flushMicrotasks();
        expect(u.showsImage, isTrue);
        async.elapse(FakeNode.swapTime);
        s.attach(node.connect());
        async.flushMicrotasks();
        expect(u.phase, Phase.done, reason: u.message);
        expect(u.showsImage, isFalse);
        expect(u.message, isNull);
        final done = u.updated!;
        expect((done.from, done.to), (1, 2));
        expect(done.at, start.add(async.elapsed));
        // Another pick shows its image again; the node still runs 2.
        u.choose(parseImage(imageFile(3, 5000)), 'bark-fw-3.img');
        expect(u.phase, Phase.idle);
        expect(u.showsImage, isTrue);
        expect(u.newer, isTrue);
        expect(u.updated, same(done));
        // A node on another version is no longer the one updated.
        node.image = 1;
        s.attach(node.connect());
        async.flushMicrotasks();
        expect(u.updated, isNull);
      });
    });

    test('the latest release is offered only when newer than the node', () {
      run((async, s, node, u) {
        expect(u.latest, isNull);
        expect(u.noLatest, 'No release known yet.');
        known = const Release(1, image: 'https://x/img1');
        expect(u.latest, isNull);
        expect(u.noLatest, 'The node already runs the latest release.');
        known = const Release(2);
        expect(u.latest, isNull);
        expect(u.noLatest, 'Release 2 carries no node firmware.');
        known = const Release(2, image: 'https://x/img2');
        expect(u.latest?.n, 2);
        expect(u.noLatest, isNull);
        node.image = 3;
        s.attach(node.connect());
        async.flushMicrotasks();
        expect(u.latest, isNull);
        expect(
          u.noLatest,
          'The node runs a newer version than the latest release, 2.',
        );
      });
    });

    test('a reboot whose answer the link\'s end overtook', () {
      run((async, s, node, u) {
        u.choose(parseImage(imageFile(2, 5000)), 'file');
        u.send();
        async.flushMicrotasks();
        expect(u.phase, Phase.committed, reason: u.message);
        node.rebootAnswerLost = true;
        u.reboot();
        async.flushMicrotasks();
        expect(u.phase, Phase.rebooting, reason: u.message);
        async.elapse(FakeNode.swapTime);
        s.attach(node.connect());
        async.flushMicrotasks();
        expect(u.phase, Phase.done, reason: u.message);
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

    test('an unsigned image is refused at the begin', () {
      run((async, s, node, u) {
        u.choose(parseImage(imageFile(2, 5000, unsigned: true)), 'file');
        u.send();
        async.flushMicrotasks();
        expect(u.phase, Phase.failed);
        expect(u.message, contains(Refusal.signature.why));
        expect(node.piecesWritten, 0);
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

  testWidgets('the screen of a finished update', (t) async {
    final sodium = await SodiumInit.init();
    var clock = DateTime.utc(2026, 11, 8, 10);
    DateTime now() => clock;
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, now),
      session: Session(store: MemoryStore(), now: now),
      contacts: Contacts(MemoryStore()),
      now: now,
    );
    await t.runAsync(app.load);
    app.releases.latest = const Release(2, image: 'https://x/img2');
    app.setFake(true);
    await app.connectFake();
    final u = app.updater;
    await t.pumpWidget(MaterialApp(home: UpdateScreen(app)));
    final latest = find.widgetWithText(FilledButton, 'Latest release');
    final pick = find.widgetWithText(OutlinedButton, 'Pick a file');
    expect(t.widget<FilledButton>(latest).enabled, isTrue);

    u.choose(parseImage(imageFile(2, 5000)), 'bark-fw-2.img');
    await u.send();
    await t.pump();
    expect(u.phase, Phase.committed, reason: u.message);
    expect(find.text('Version 2'), findsOneWidget);
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
    await u.reboot();
    await t.pump();
    clock = clock.add(FakeNode.swapTime);
    await app.connectFake();
    await t.pump();
    expect(u.phase, Phase.done, reason: u.message);

    final name = nodeLabel(app.session.node!.id);
    expect(
      find.text('$name runs version 2, updated from version 1 just now.'),
      findsOneWidget,
    );
    expect(find.text('Version 2'), findsNothing);
    expect(find.byType(LinearProgressIndicator), findsNothing);
    expect(find.textContaining('KiB'), findsNothing);
    expect(t.widget<FilledButton>(latest).enabled, isFalse);
    expect(t.widget<OutlinedButton>(pick).enabled, isTrue);
    expect(
      find.text('The node already runs the latest release.'),
      findsOneWidget,
    );
    clock = clock.add(const Duration(minutes: 3));
    await t.pump(const Duration(seconds: 1));
    expect(
      find.text('$name runs version 2, updated from version 1 3 min ago.'),
      findsOneWidget,
    );
    expect(find.text('The node hears no one'), findsNothing);
    // The fake's new image hears no relay.
    clock = clock.add(hearsNoOneAfter);
    await t.pump(hearsNoOneAfter);
    expect(find.text('The node hears no one'), findsOneWidget);
    expect(find.textContaining('bark-fw-1.uf2'), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    app.dispose();
  });
}
