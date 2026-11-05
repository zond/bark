// The app's side of the link against the fake node: the queue, the
// inbox, given back and the version check.

import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/node_link.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';

class NewerNode extends FakeNode {
  NewerNode({required super.now});
  @override
  Future<NodeInfo> readNode() async =>
      NodeInfo(linkVersion + 1, FakeNode.id, 1);
}

void main() {
  final start = DateTime.utc(2026, 11, 5);
  final to = Address(Position.fromDegrees(50.0, 30.0), [1, 2, 3, 4, 5, 6]);

  void run(void Function(FakeAsync, Session, FakeNode, MemoryStore) body) {
    fakeAsync((async) {
      DateTime now() => start.add(async.elapsed);
      final store = MemoryStore();
      final s = Session(store: store, now: now);
      final node = FakeNode(now: now);
      s.attach(node.connect());
      async.flushMicrotasks();
      body(async, s, node, store);
      node.dispose();
      s.dispose();
    });
  }

  test('attach drains the inbox and stores it first', () {
    run((async, s, node, store) {
      expect(s.connected, isTrue);
      expect(s.status!.ready, isTrue);
      expect(s.status!.booting, isTrue);
      expect(s.status!.inbox, 0);
      expect(s.received.map((r) => r.text), [
        'Hello from the fake node.',
        'A second message, waiting in the inbox.',
      ]);
      expect((store.state!['received'] as List).length, 2);
      expect(s.address.toString(), '${Address(FakeNode.gps, FakeNode.id)}');
    });
  });

  test('the first rotation counts down here', () {
    run((async, s, node, store) {
      expect(s.bootLeftS, 195);
      // Counted from the last status, whose seconds are rounded up, so
      // never early.
      async.elapse(const Duration(seconds: 100));
      expect(s.bootLeftS, inInclusiveRange(95, 96));
      async.elapse(const Duration(seconds: 100));
      expect(s.status!.booting, isFalse);
      expect(s.status!.relays, FakeNode.relaysHeard);
    });
  });

  test('the queue: taken, given back, not ready, offered again', () {
    run((async, s, node, store) {
      for (final t in ['a', 'b', 'c', 'd']) {
        s.queue(to, t);
      }
      async.elapse(const Duration(seconds: 1));
      final [a, b, c, d] = s.outbox;
      expect(a.state, OutState.taken);
      expect(b.state, OutState.taken);
      expect(c.state, OutState.givenBack);
      // 16 bans, 25.28 s, told as 26 and counted from the notification.
      expect(s.waitLeftS(c), 26);
      expect(d.state, OutState.queued);
      expect(s.status!.ready, isFalse);

      // Ready after the wait; the fourth write finds the node not ready
      // for a ban, and then it is taken.
      async.elapse(const Duration(seconds: 28));
      expect(s.waitLeftS(c), 0);
      expect(d.state, OutState.taken);

      s.offerAgain(c);
      async.elapse(const Duration(seconds: 1));
      expect(c.state, OutState.taken);

      // The echoes of the taken ones.
      async.elapse(const Duration(seconds: 10));
      expect(s.received.map((r) => r.text).where((t) => t.startsWith('Echo')), [
        'Echo: a',
        'Echo: b',
        'Echo: d',
        'Echo: c',
      ]);
    });
  });

  test('given back while away is read at the next connect', () {
    run((async, s, node, store) {
      for (final t in ['a', 'b', 'c']) {
        s.queue(to, t);
      }
      // Gone before the third is given back.
      async.elapse(const Duration(milliseconds: 400));
      expect(s.outbox[2].state, OutState.taken);
      s.disconnect();
      async.elapse(const Duration(seconds: 10));
      expect(s.connected, isFalse);
      s.attach(node.connect());
      async.flushMicrotasks();
      expect(s.outbox[2].state, OutState.givenBack);
      expect(s.waitLeftS(s.outbox[2]), 16);
    });
  });

  test('the fake names the sender, a real node cannot yet', () {
    run((async, s, node, store) {
      // Attached before the hook was set, as with a real node.
      expect(s.received.map((r) => r.from), [null, null]);
      s.senderOf = node.senderOf;
      s.queue(to, 'a');
      async.elapse(const Duration(seconds: 6));
      final echo = s.received.last;
      expect(echo.text, 'Echo: a');
      expect(echo.from.toString(), to.toString());
      // Kept across a restart.
      final again = Session(store: store, now: () => start);
      again.load();
      async.flushMicrotasks();
      expect(again.received.last.from.toString(), to.toString());
      expect(again.outbox.single.at, s.outbox.single.at);
    });
  });

  test('the log and the last given back', () {
    run((async, s, node, store) {
      for (final t in ['a', 'b', 'c']) {
        s.queue(to, t);
      }
      async.elapse(const Duration(seconds: 1));
      final c = s.outbox[2];
      expect(s.givenBack!.msgId, c.id);
      final lines = s.log.map((l) => l.text).toList();
      expect(lines.first, startsWith('Node: bark-fa4e00000001'));
      expect(lines, contains('Send ${msgIdHex(c.id)}: taken'));
      expect(lines, contains(startsWith('Given back: ${msgIdHex(c.id)}')));
      expect(lines, contains(startsWith('Inbox: confirmed')));
    });
  });

  test('a node with a newer link is refused', () {
    fakeAsync((async) {
      final s = Session(store: MemoryStore(), now: () => start);
      final node = NewerNode(now: () => start);
      bool? ok;
      s.attach(node.connect()).then((v) => ok = v);
      async.flushMicrotasks();
      expect(ok, isFalse);
      expect(s.connected, isFalse);
      expect(s.notice, contains('newer'));
      node.dispose();
    });
  });

  test('the queue and the inbox survive a restart', () {
    run((async, s, node, store) {
      s.queue(to, 'kept');
      async.flushMicrotasks();
      final again = Session(store: store, now: () => start);
      again.load();
      async.flushMicrotasks();
      expect(again.received.length, 2);
      expect(again.outbox.single.text, 'kept');
      expect(again.outbox.single.id, s.outbox.single.id);
    });
  });

  test('the fake answers a short Send with invalid length', () {
    fakeAsync((async) {
      final node = FakeNode(now: () => start);
      Object? err;
      node.connect().send(to.encode().sublist(0, 12)).catchError((e) {
        err = e;
      });
      async.flushMicrotasks();
      expect((err as AttException).code, AttError.invalidLength);
      node.dispose();
    });
  });
}
