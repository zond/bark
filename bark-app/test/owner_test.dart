// The home's owner against the fake node, whose home away answers the
// owner's commands as a home does: a contact given a home and told, the
// same key used again after a lost answer; the people listed, one
// removed, the mark handed on and then refused; a home given used, with
// its key; the person's home named after a restore from the words, and
// one that does not answer changing nothing; a home that says it was
// removed, fetched from or named, and one that stays silent; and the
// seat taken at the home by writing Home again.

import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/app_layer.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

import 'fake_ble.dart';
import 'fake_node.dart';

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final start = DateTime.utc(2026, 11, 21);

  /// The person's home away from the fake node, with their own key.
  final away = Address(Position.fromDegrees(50.5, 30.5), [9, 9, 9, 9, 9, 9]);

  /// An app connected to the fake node.
  void run(void Function(FakeAsync, AppModel) body) {
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

  /// Long enough for a fetch to go, given back or not, and its answer.
  const answered = Duration(minutes: 1);

  /// The news the home's answer brought, taken.
  String? news(AppModel app) {
    final n = app.session.fetchNews;
    app.session.fetchNews = null;
    return n;
  }

  List<Outcome?> outcomes(FakeNode f) => [
    for (final (c, o) in f.commands)
      if (c != null) o,
  ];

  test('the owner shares their home: the home takes a key made for the '
      'contact, then the contact is sent it; asked again, the same key', () {
    run((async, app) {
      final f = app.fakeNode!;
      said(async, app.makeHome(app.session.address!));
      final home = app.me.home!;
      expect(app.isOwner, isTrue);
      final friend = app.contacts.list.single;
      expect(
        said(async, app.shareHome(friend)),
        'Sent to your home; its answer comes with your mail',
      );
      final share = app.contacts.byKey(friend.key)!.share!;
      expect(share.home.toString(), home.toString());
      async.elapse(answered);
      expect(outcomes(f), [Outcome.ok]);
      final added = f.homeRecords.last;
      expect(hex(added.key), hex(share.key));
      expect(added.tag, recipientTag(sodium, friend.card.identity, home.id));
      expect(added.name, 'Fake friend');
      expect(news(app), 'Fake friend has a home at yours now. They are told');
      final gifts = app.session.outbox.where((o) => o.kind == OutKind.home);
      final pair = SivKeys.pair(
        sodium,
        app.me.identity,
        f.friend.pair.secretKey,
      );
      final (_, rs) = readPlain(openBox(pair, gifts.single.payload));
      final g = rs.single as HomeRecord;
      expect(
        (g.home.toString(), hex(g.key)),
        (home.toString(), hex(share.key)),
      );

      // A lost answer: asked again, the same key, which the home holds.
      said(async, app.shareHome(friend));
      async.elapse(answered);
      expect(outcomes(f), [Outcome.ok, Outcome.already]);
      expect(hex(app.contacts.byKey(friend.key)!.share!.key), hex(share.key));
      expect(f.homeRecords.length, 2);
      expect(gifts.length, 2);
    });
  });

  test('away, the owner lists the people, removes one, hands the mark on, '
      'and is then refused', () {
    run((async, app) {
      final f = app.fakeNode!;
      app.me.setHome(away);
      async.flushMicrotasks();
      expect(app.isOwner, isFalse);
      // The summary says so.
      said(async, app.fetchNow());
      async.elapse(answered);
      expect(app.isOwner, isTrue);
      f.homeRecords.addAll([
        FakeHomeRecord(2, List.filled(32, 2), tag: 5, name: 'Bo'),
        FakeHomeRecord(3, List.filled(32, 3), tag: 6, name: 'Cy'),
      ]);
      said(async, app.askList());
      async.elapse(answered);
      expect(app.listed!.map((r) => (r.name, r.mine, r.owner)), [
        ('Me', true, true),
        ('Bo', false, false),
        ('Cy', false, false),
      ]);
      final [_, bo, cy] = app.listed!;

      said(async, app.removeHome(cy));
      async.elapse(answered);
      expect(news(app), 'Removed from your home');
      expect(app.listed!.map((r) => r.name), ['Me', 'Bo']);
      expect(f.homeRecords.map((r) => r.name), ['Me', 'Bo']);

      said(async, app.handOver(bo));
      async.elapse(answered);
      expect(news(app), 'Bo is your home\'s owner now');
      expect(app.isOwner, isFalse);
      expect(f.homeRecords.map((r) => r.owner), [false, true]);

      // A command now is refused: the mark has gone.
      said(async, app.removeHome(bo));
      async.elapse(answered);
      expect(news(app), 'Your home says you are not its owner');
      expect(f.homeRecords.length, 2);
    });
  });

  test('a home given is used with its key: the card changes, a fetch with '
      'that key goes, and the home before is left', () {
    run((async, app) {
      final f = app.fakeNode!;
      said(async, app.makeHome(app.session.address!));
      final before = app.me.home!;
      final friend = app.contacts.list.single;
      final given = Address(Position.fromDegrees(50.6, 30.6), [
        7,
        7,
        7,
        7,
        7,
        7,
      ]);
      final key = Uint8List.fromList(List.filled(32, 0x42));
      final pair = SivKeys.pair(
        sodium,
        app.me.identity,
        f.friend.pair.secretKey,
      );
      f.deliver(
        sealBox(
          pair,
          recipientTag(sodium, app.me.identity, f.nodeId),
          writePlain(
            Header(greater(f.friend.identity, app.me.identity), 999999),
            [HomeRecord(given, key)],
          ),
        ),
      );
      async.flushMicrotasks();
      final g = app.contacts.byKey(friend.key)!.gift!;
      expect((g.home.toString(), hex(g.key)), (given.toString(), hex(key)));

      f.homeRecords.add(FakeHomeRecord(5, key, name: 'Me too'));
      final cards = app.session.outbox.where((o) => o.kind == OutKind.card);
      final sent = cards.length;
      expect(
        said(async, app.useGift(app.contacts.byKey(friend.key)!)),
        '${nodeLabel(given.id)} is your home. Your old home sends you its '
        'mail, then forgets you.',
      );
      expect(app.me.home.toString(), given.toString());
      expect(hex(app.me.givenKey!), hex(key));
      expect(hex(app.me.mailboxKey.extractBytes()), hex(key));
      expect(app.contacts.byKey(friend.key)!.gift, isNull);
      expect(cards.length, sent + 1);
      expect(app.me.leaving!.home.toString(), before.toString());
      final toGiven = app.session.outbox.where(
        (o) => o.kind == OutKind.fetch && o.dst.toString() == given.toString(),
      );
      final k = SecureKey.fromList(sodium, key);
      expect(
        Fetch.open(sodium, k, given.id, toGiven.single.payload).sent,
        isPositive,
      );
      k.dispose();
      async.elapse(const Duration(minutes: 3));
      expect(toGiven.single.answered, isTrue);
      expect(app.isOwner, isFalse);
      expect(app.me.leaving, isNull);
      // Kept with the person, for the next start.
      expect((app.me.store as MemoryStore).state!['homeKey'], hex(key));
    });
  });

  test('use my existing home: a fetch with the identity\'s key, its summary '
      'makes it the home; no answer changes nothing', () {
    run((async, app) {
      final f = app.fakeNode!;
      f.homeAnswers = false;
      expect(
        said(async, app.tryHome(away)),
        'Asking ${nodeLabel(away.id)} for your mail',
      );
      expect(app.tried!.over, isFalse);
      async.elapse(Session.fetchAnswerWait);
      async.elapse(const Duration(seconds: 1));
      expect(app.tried!.over, isTrue);
      expect(app.me.home, isNull);

      f.homeAnswers = true;
      said(async, app.tryHome(away));
      async.elapse(answered);
      expect(app.me.home.toString(), away.toString());
      expect(app.me.givenKey, isNull);
      // What it reads, as its summary said, kept with the home.
      expect(app.me.homeReads, 2);
      expect((app.me.store as MemoryStore).state!['homeReads'], 2);
      expect(app.isOwner, isTrue);
      expect(app.tried, isNull);
      expect(news(app), '${nodeLabel(away.id)} is your home');
    });
  });

  test('a home that says it was removed: no home, the person told, and the '
      'card without it goes to the contacts', () {
    run((async, app) {
      final f = app.fakeNode!;
      app.me.setHome(away);
      async.flushMicrotasks();
      f.removedRecords.add(f.homeRecords.removeAt(0));
      final cards = app.session.outbox.where((o) => o.kind == OutKind.card);
      final sent = cards.length;
      said(async, app.fetchNow());
      async.elapse(answered);
      expect(f.commands.single, (null, Outcome.removed));
      expect(app.me.home, isNull);
      expect(app.session.notice?.text, contains('no longer keeps your mail'));
      expect(app.session.fetchWaiting, isNull);
      expect(cards.length, sent + 1);
    });
  });

  test('use my existing home, removed there: told so, and nothing changes', () {
    run((async, app) {
      final f = app.fakeNode!;
      f.removedRecords.add(f.homeRecords.removeAt(0));
      said(async, app.tryHome(away));
      async.elapse(answered);
      expect(app.me.home, isNull);
      expect(app.tried, isNull);
      expect(
        app.session.notice?.text,
        '${nodeLabel(away.id)} does not keep your mail. Nothing changed.',
      );
    });
  });

  test('a home that did not answer the last fetch is said so in Settings, '
      'until one is answered', () {
    run((async, app) {
      final f = app.fakeNode!;
      app.me.setHome(away);
      async.flushMicrotasks();
      f.homeAnswers = false;
      said(async, app.fetchNow());
      async.elapse(Session.fetchAnswerWait - const Duration(seconds: 1));
      expect(app.homeSilent, isFalse);
      async.elapse(const Duration(seconds: 2));
      expect(app.homeSilent, isTrue);
      expect(app.me.home.toString(), away.toString(), reason: 'no change');
      f.homeAnswers = true;
      said(async, app.fetchNow());
      async.elapse(answered);
      expect(app.homeSilent, isFalse);
    });
  });

  test('at the home with no owner, Become owner writes Home again and takes '
      'the seat', () {
    run((async, app) {
      final f = app.fakeNode!;
      said(async, app.makeHome(app.session.address!));
      f.mineOwner = false;
      f.changed();
      async.flushMicrotasks();
      expect(app.isOwner, isFalse);
      expect(said(async, app.becomeOwner()), 'You are its owner');
      expect(f.mineOwner, isTrue);
      expect(app.isOwner, isTrue);
      expect(app.me.owner, isTrue);
    });
  });
}
