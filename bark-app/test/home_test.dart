// The home node against the fake node: the mailbox key written and read
// back, cleared at the home, kept by a home the person is not connected
// to, written again to a home that lost it, and a node that does not
// keep it refused as a home.

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

/// A node that takes the key but reads no home.
class KeylessNode extends FakeNode {
  KeylessNode({required super.now, required super.sodium, required super.app});
  @override
  Future<NodeHome> readHome() async => const NodeHome(false, 0, 0);
}

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 6);

  /// An app connected to the fake node, or to the node `make` gives.
  void run(
    void Function(FakeAsync, AppModel) body, {
    FakeNode Function(DateTime Function(), List<int>)? make,
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
      if (make != null) app.fakeNode = make(now, app.me.identity);
      app.connectFake();
      async.flushMicrotasks();
      body(async, app);
      app.dispose();
    });
  }

  String? said(FakeAsync async, Future<String> f) {
    String? r;
    f.then((v) => r = v);
    async.flushMicrotasks();
    return r;
  }

  List<String> log(AppModel app) => app.session.log.map((l) => l.text).toList();

  test('made the home: the key written and read back', () {
    run((async, app) {
      expect(app.session.nodeHome!.set, isFalse);
      final here = app.session.address!;
      expect(said(async, app.makeHome(here)), 'This node is your home');
      expect(app.fakeNode!.homeKey, app.me.keys.mailbox.extractBytes());
      expect(app.session.nodeHome!.set, isTrue);
      expect(app.me.home.toString(), here.toString());
      expect(app.atHome, isTrue);
    });
  });

  test('no home node, connected to it: the node forgets the key', () {
    run((async, app) {
      said(async, app.makeHome(app.session.address!));
      expect(said(async, app.noHome()), 'No home node');
      expect(app.fakeNode!.homeKey, isNull);
      expect(app.session.nodeHome!.set, isFalse);
      expect(app.me.home, isNull);
    });
  });

  test('no home node elsewhere: the old home keeps the key until cleared '
      'there', () {
    run((async, app) {
      said(async, app.makeHome(app.session.address!));
      app.session.disconnect();
      async.flushMicrotasks();
      expect(said(async, app.noHome()), contains('still holds'));
      expect(app.me.home, isNull);
      expect(app.fakeNode!.homeKey, isNotNull);

      app.connectFake();
      async.flushMicrotasks();
      expect(app.holdsOtherKey, isTrue);
      expect(said(async, app.clearKey()), contains('no longer'));
      expect(app.fakeNode!.homeKey, isNull);
      expect(app.holdsOtherKey, isFalse);
    });
  });

  test('a home that lost the key is given it again on connection', () {
    run((async, app) {
      said(async, app.makeHome(app.session.address!));
      // As a reflashed node: its key gone while the person is away.
      app.fakeNode!.homeKey = null;
      app.session.disconnect();
      async.flushMicrotasks();
      app.connectFake();
      async.flushMicrotasks();
      expect(app.fakeNode!.homeKey, app.me.keys.mailbox.extractBytes());
      expect(app.session.nodeHome!.set, isTrue);
      expect(log(app), contains(contains('held no key; written again')));
      expect(app.session.notice, isNull);
    });
  });

  test('a node that does not keep the key is not made the home', () {
    run((async, app) {
      final r = said(async, app.makeHome(app.session.address!));
      expect(r, contains('did not keep your mailbox key'));
      expect(app.me.home, isNull);
    }, make: (now, me) => KeylessNode(now: now, sodium: sodium, app: me));
  });
}
