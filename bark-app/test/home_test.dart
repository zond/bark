// The home node against the fake node: the mailbox key written and read
// back, cleared at the home, kept by a home the person is not connected
// to, written again on connection to the home, the home followed to
// a fixed node's new place, its card sent only when the person asks, but
// not to a carried node's wandering fix, and a node that does not keep it
// refused as a home; written again only where the node says it is not
// the person's and a home of their tag is left there, and told, not
// overruled, where someone removed it.

import 'package:bark_app/app.dart';
import 'package:bark_app/app_layer.dart' show recipientTag;
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';

import 'fake_ble.dart';
import 'fake_node.dart';

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
      app.disposeWithNode();
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
      expect(
        said(async, app.noHome()),
        contains('Your old home keeps your mail'),
      );
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

  test('a home that says it is the person\'s is not written again', () {
    run((async, app) {
      said(async, app.makeHome(app.session.address!));
      app.session.disconnect();
      async.flushMicrotasks();
      app.connectFake();
      async.flushMicrotasks();
      expect(log(app), isNot(contains(contains('the key written'))));
      expect(app.fakeNode!.homeKeyChanges, 1);
      expect(app.session.nodeHome!.mine, isTrue);
      expect(app.homeKeyGone, isFalse);
    });
  });

  test('a home whose key the person\'s phone left by unpairing is joined '
      'again, silently', () {
    run((async, app) {
      final f = app.fakeNode!;
      said(async, app.makeHome(app.session.address!));
      app.session.disconnect();
      async.flushMicrotasks();
      // The phone unpaired and paired again: the home is in a record of
      // the person's tag with no bond, and the phone's own holds none.
      f.homeKey = null;
      f.others.add(
        PhoneRecord(
          number: 9,
          home: true,
          tag: recipientTag(sodium, app.me.identity, f.nodeId),
        ),
      );
      app.connectFake();
      async.flushMicrotasks();
      expect(f.homeKey, app.me.keys.mailbox.extractBytes());
      expect(log(app), contains(contains('the key written')));
      expect(app.session.notice, isNull);
    });
  });

  test('a home someone removed is told, not overruled, and is made the home '
      'again by the person', () {
    run((async, app) {
      final f = app.fakeNode!;
      said(async, app.makeHome(app.session.address!));
      app.session.disconnect();
      async.flushMicrotasks();
      f.homeKey = null;
      app.connectFake();
      async.flushMicrotasks();
      expect(f.homeKey, isNull);
      expect(app.session.notice, contains('no longer keeps your mail'));
      expect(app.homeKeyGone, isTrue);
      expect(
        said(async, app.makeHome(app.session.address!)),
        'This node is your home',
      );
      expect(f.homeKey, app.me.keys.mailbox.extractBytes());
      expect(app.homeKeyGone, isFalse);
    });
  });

  test('another person\'s home is someone else\'s, with no key to clear', () {
    run((async, app) {
      app.fakeNode!.others.add(const PhoneRecord(number: 2, home: true));
      app.session.disconnect();
      async.flushMicrotasks();
      app.connectFake();
      async.flushMicrotasks();
      expect(app.session.nodeHome!.set, isTrue);
      expect(app.othersHome, isTrue);
      expect(app.holdsOtherKey, isFalse);
    });
  });

  test('the home follows its node to a new place, and the card changes', () {
    run((async, app) {
      final old = app.session.address!;
      said(async, app.makeHome(old));
      final card = app.me.cardTime;
      async.elapse(const Duration(seconds: 5));
      // As a fixed node that took a fresh place at boot.
      app.session.setMapPoint(Position.fromDegrees(50.46, 30.53));
      async.flushMicrotasks();
      final here = app.session.address!;
      expect(here.toString(), isNot(old.toString()));
      expect(app.me.home.toString(), here.toString());
      expect(app.me.cardTime, greaterThan(card));
      expect(app.atHome, isTrue);
    });
  });

  /// Two contacts beside the fake friend.
  void twoContacts(FakeAsync async, AppModel app) {
    for (final b in [5, 6]) {
      final where = Address(Position.fromDegrees(50, 30), List.filled(6, b));
      app.contacts.put(
        Contact(PersonCard(List.filled(32, b), where, 0, '$b'), '$b'),
      );
    }
    async.flushMicrotasks();
  }

  int cards(AppModel app) =>
      app.session.outbox.where((o) => o.kind == OutKind.card).length;

  test('a carried home that wanders sends no card', () {
    run((async, app) {
      final old = app.session.address!;
      said(async, app.makeHome(old));
      twoContacts(async, app);
      async.elapse(const Duration(seconds: 30));
      final before = cards(app);
      app.fakeNode!.carried = true;
      for (var i = 1; i <= 60; i++) {
        async.elapse(const Duration(seconds: 1));
        app.fakeNode!.walkTo(Position.fromDegrees(50.45 + 0.0001 * i, 30.52));
        async.flushMicrotasks();
      }
      expect(app.session.address.toString(), isNot(old.toString()));
      expect(app.me.home.toString(), old.toString());
      expect(cards(app), before);
    });
  });

  test('a fixed home at a new place sends nothing until the person asks', () {
    run((async, app) {
      said(async, app.makeHome(app.session.address!));
      twoContacts(async, app);
      // The fake friend's intros are read and answered first.
      async.elapse(const Duration(seconds: 30));
      final sent = cards(app);
      final each = app.contacts.list.length;
      expect(app.cardUnsent, isFalse);
      app.session.setMapPoint(Position.fromDegrees(50.46, 30.53));
      async.flushMicrotasks();
      expect(app.me.home.toString(), app.session.address.toString());
      expect(cards(app), sent);
      expect(app.cardUnsent, isTrue);
      app.messenger.sendCardToAll();
      async.flushMicrotasks();
      expect(cards(app), sent + each);
      expect(app.cardUnsent, isFalse);
      for (var i = 0; i < 10; i++) {
        async.elapse(const Duration(seconds: 1));
        app.fakeNode!.walkTo(FakeNode.gps);
        async.flushMicrotasks();
      }
      expect(cards(app), sent + each);
      expect(app.cardUnsent, isFalse);
    });
  });

  test('a node that does not keep the key is not made the home', () {
    run((async, app) {
      final r = said(async, app.makeHome(app.session.address!));
      expect(r, contains('refused'));
      expect(app.me.home, isNull);
    }, make: (now, me) => KeylessNode(now: now, sodium: sodium, app: me));
  });
}
