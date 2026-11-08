// A public node (spec "Phone link", Public), against the fake: the owner
// writes its recipient tag and sets Public; a guest's link takes nothing
// that needs the bond, and the app's guest touches nothing of it; the
// node hands each phone only the messages with its tag, keeps the
// owner's, and drops what it can place with nobody; and phones take
// turns at the one message it holds, the turn being Ready.

import 'dart:typed_data';

import 'package:bark_app/app_layer.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/node_link.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

import 'session_test.dart' show Phone;

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 8);
  final there = Address(Position.fromDegrees(50.0, 30.0), [1, 2, 3, 4, 5, 6]);

  /// The owner's phone attached to the fake, and a guest's phone, each of
  /// a fixed identity.
  void run(void Function(FakeAsync, Phone, Phone, FakeNode) body) {
    fakeAsync((async) {
      DateTime now() => start.add(async.elapsed);
      final owner = Phone(sodium, now), guest = Phone(sodium, now);
      async.flushMicrotasks();
      owner.me.restore(Uint8List.fromList(List.filled(seedLen, 1)));
      guest.me.restore(Uint8List.fromList(List.filled(seedLen, 2)));
      async.flushMicrotasks();
      final node = FakeNode(now: now, sodium: sodium, app: owner.me.identity);
      owner.session.attach(node.connect());
      async.flushMicrotasks();
      body(async, owner, guest, node);
      node.dispose();
      owner.session.dispose();
      guest.session.dispose();
    });
  }

  int tagOf(Phone p) => recipientTag(sodium, p.me.identity, FakeNode.id);

  /// A payload of the current format for the tag `tag`, which opens for
  /// nobody.
  Uint8List payloadFor(int tag, int n) => Uint8List.fromList([
    formatByte(Form.boxed),
    tag,
    n,
    ...List.filled(40, 0),
  ]);

  Matcher refused(int code) =>
      throwsA(isA<AttException>().having((e) => e.code, 'code', code));

  test('the owner writes its tag, reads Public, on by default, and sets '
      'it', () {
    run((async, owner, guest, node) {
      expect(node.ownerTag, tagOf(owner));
      expect(owner.session.nodePublic, isTrue);
      owner.session.writePublic(false);
      async.flushMicrotasks();
      expect((node.public, owner.session.nodePublic), (false, false));
      owner.session.writePublic(true);
      async.flushMicrotasks();
      expect((node.public, owner.session.nodePublic), (true, true));
    });
  });

  test('a guest\'s link: refused everything on a node not public, and on '
      'a public one what needs the bond', () {
    run((async, owner, guest, node) {
      node.public = false;
      final g = node.connectGuest();
      expect(g.readNode(), refused(AttError.insufficientAuthentication));
      node.public = true;
      expect(g.readNode(), completes);
      for (final closed in <Future<void> Function()>[
        g.readClock,
        g.readHome,
        g.readNeighbours,
        g.readUpdate,
        g.readPublic,
        () => g.setPublic(true),
        () => g.setMapPoint(null),
        () => g.setHome(null),
      ]) {
        expect(closed(), refused(AttError.insufficientAuthentication));
      }
      async.flushMicrotasks();
    });
  });

  test('a guest attaches: its tag written, nothing that needs the bond '
      'touched, and the owner\'s settings not its', () {
    run((async, owner, guest, node) {
      node.public = true;
      final g = node.connectGuest();
      guest.session.attach(g, guest: true);
      async.flushMicrotasks();
      final s = guest.session;
      expect((s.connected, s.guest), (true, true));
      expect(g.tag, tagOf(guest));
      expect(tagOf(guest), isNot(tagOf(owner)));
      expect((s.nodeTime, s.nodeHome, s.nodePublic), (null, null, null));
      expect(s.log.where((l) => l.text.contains('0x5')), isEmpty);
      Object? failed;
      s.writePublic(true).then<void>((_) {}, onError: (Object e) => failed = e);
      async.flushMicrotasks();
      expect(failed, isA<StateError>());
      expect(node.public, isTrue);
      // The fake friend's first messages are the owner's alone.
      expect(s.received, isEmpty);
      expect(owner.session.received, hasLength(2));
    });
  });

  test('each phone gets the messages with its tag; the owner\'s are kept '
      'while it is away; what is no one\'s is dropped', () {
    run((async, owner, guest, node) {
      node.public = true;
      final g = node.connectGuest();
      guest.session.attach(g, guest: true);
      async.flushMicrotasks();
      final nobody = {for (var t = 0; t < 256; t++) t}
          .difference({tagOf(owner), tagOf(guest)})
          .first;
      node.deliver(payloadFor(tagOf(guest), 1));
      node.deliver(payloadFor(tagOf(owner), 2));
      node.deliver(payloadFor(nobody, 3));
      node.deliver(payloadFor(tagOf(guest), 4));
      async.flushMicrotasks();
      expect(guest.session.received.map((r) => r.payload[2]), [1, 4]);
      expect(owner.session.received.last.payload[2], 2);
      expect(node.dropped, 1);
      // The owner away, its message waits; the guest gone, its own is
      // dropped.
      owner.session.disconnect();
      guest.session.disconnect();
      async.flushMicrotasks();
      node.deliver(payloadFor(tagOf(owner), 5));
      node.deliver(payloadFor(tagOf(guest), 6));
      expect(node.dropped, 2);
      owner.session.attach(node.connect());
      async.flushMicrotasks();
      expect(owner.session.received.last.payload[2], 5);
    });
  });

  test('the turn is Ready: one phone at a time, in link order; it passes '
      'when the node takes its message, after a ban unused, or when its '
      'link ends', () {
    run((async, owner, guest, node) {
      node.public = true;
      final g1 = node.connectGuest(), g2 = node.connectGuest();
      final links = <NodeLink>[node, g1, g2];
      List<bool?> ready() {
        final r = List<bool?>.filled(links.length, null);
        for (final (i, l) in links.indexed) {
          l.readStatus().then((s) => r[i] = s.ready);
        }
        async.flushMicrotasks();
        return r;
      }

      Uint8List send(int n) => encodeSend(there, payloadFor(tagOf(owner), n));
      // The owner's link came first: its turn.
      expect(ready(), [true, false, false]);
      // Out of turn: not ready.
      Object? failed;
      g1.send(send(1)).then<void>((_) {}, onError: (Object e) => failed = e);
      async.flushMicrotasks();
      expect(failed, isA<AttException>().having((e) => e.code, 'code', 0x80));
      // The owner sits on it for a ban: the next link's turn.
      async.elapse(FakeNode.ban);
      expect(ready(), [false, true, false]);
      // Taken: the turn passes, and Ready comes back once the node takes
      // a message again.
      var taken = false;
      g1.send(send(2)).then((_) => taken = true);
      async.flushMicrotasks();
      expect(taken, isTrue);
      expect(ready(), [false, false, false]);
      async.elapse(FakeNode.handOver);
      expect(ready(), [false, false, true]);
      // Its link ends: the turn goes on, round to the owner's.
      g2.close();
      links.removeLast();
      async.flushMicrotasks();
      expect(ready(), [true, false]);
    });
  });
}
