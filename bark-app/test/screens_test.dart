// The screens against the fake node: they build; a text goes to the fake
// friend and its echo comes back into the same thread, sealed both ways;
// the echo is acknowledged, its button no taller than the small text;
// the card, the backup words, adding a contact from a pasted or a scanned
// card, and the debug screen; the fetch button, on the first screen and
// in a thread, and why it sends nothing; the home
// buttons, the home known by its node ID wherever it is, and its card
// sent after a move only by "Send my card"; and a node that another
// phone has paired with since, and pairing with it again.

import 'package:bark_app/app.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/app_layer.dart';
import 'package:bark_app/ui/contacts_screen.dart';
import 'package:bark_app/ui/thread_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:sodium/sodium.dart';

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());

  Future<AppModel> model(WidgetTester t) async {
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, DateTime.now),
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
      now: DateTime.now,
    );
    await t.runAsync(app.load);
    return app;
  }

  testWidgets('conversations, a thread, settings, card and debug', (t) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    expect(find.textContaining('No conversations yet'), findsOneWidget);
    expect(find.textContaining('12 backup words'), findsOneWidget);

    // Settings: the fake node, and the backup words.
    await t.tap(find.textContaining('Not connected'));
    await t.pumpAndSettle();
    await t.tap(find.text('Fake node'));
    await t.pump();
    await t.tap(find.text('Connect'));
    await t.pump();
    expect(find.text('bark-fa4e00000001'), findsOneWidget);
    await t.scrollUntilVisible(
      find.text('Show my words'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.ensureVisible(find.text('Show my words'));
    await t.pumpAndSettle();
    await t.tap(find.text('Show my words'));
    await t.pumpAndSettle();
    expect(find.text('My 12 words'), findsOneWidget);
    expect(find.text(app.me.phrase.join(' ')), findsOneWidget);
    await t.tap(find.text('I wrote them down'));
    await t.pumpAndSettle();
    expect(app.me.backedUp, isTrue);
    await t.scrollUntilVisible(
      find.text('Map point'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.pageBack();
    await t.pumpAndSettle();

    // The fake friend's two first messages, in one conversation.
    expect(find.text('Fake friend'), findsOneWidget);
    expect(find.textContaining('A second message'), findsOneWidget);
    expect(find.textContaining('12 backup words'), findsNothing);

    // A text to the fake friend, and its echo in the same thread.
    await t.tap(find.text('Fake friend'));
    await t.pumpAndSettle();
    expect(find.textContaining('Not verified yet'), findsOneWidget);
    await t.enterText(find.byType(TextField), 'hello');
    await t.tap(find.byTooltip('Send'));
    await t.pump();
    await t.pump();
    expect(find.text('hello'), findsOneWidget);
    expect(find.byTooltip('Taken by the node'), findsOneWidget);
    await t.pump(FakeNode.echoAfter + const Duration(seconds: 1));
    expect(find.text('Echo: hello'), findsOneWidget);
    expect(find.textContaining('Not verified yet'), findsNothing);

    // Acknowledge the echo: its button becomes one plain check. The
    // friend's acknowledgement of "hello" shows two marks on the sent
    // text, in place of the node's one mark.
    Finder inBubble(String text, Finder f) => find.descendant(
      of: find.ancestor(of: find.text(text), matching: find.byType(Card)),
      matching: f,
    );
    final mark = find.byWidgetPredicate(
      (w) => w is Icon && w.icon == Icons.done_all && w.semanticLabel != null,
    );
    final taken = find.byTooltip('Taken by the node');
    final button = find.byTooltip('Acknowledge');
    expect(inBubble('Echo: hello', button), findsOneWidget);
    expect(inBubble('Echo: hello', mark), findsNothing);
    expect(inBubble('hello', mark), findsNothing);
    expect(inBubble('hello', taken), findsOneWidget);
    // The button keeps the small line one line of small text tall.
    Finder line(Finder f) =>
        find.ancestor(of: f, matching: find.byType(Row)).first;
    final withButton = t.getSize(line(inBubble('Echo: hello', button))).height;
    app.session.outbox.singleWhere((o) => o.body == 'hello').acked = true;
    await t.tap(inBubble('Echo: hello', button));
    await t.pump();
    await t.pump();
    expect(inBubble('Echo: hello', button), findsNothing);
    expect(inBubble('Echo: hello', mark), findsNothing);
    expect(inBubble('hello', mark), findsOneWidget);
    expect(inBubble('hello', taken), findsNothing);
    final check = find.byWidgetPredicate(
      (w) => w is Icon && w.icon == Icons.check && w.semanticLabel != null,
    );
    expect(inBubble('Echo: hello', check), findsOneWidget);
    final withCheck = t.getSize(line(inBubble('Echo: hello', check))).height;
    expect(withButton, closeTo(withCheck, 1));
    await t.pageBack();
    await t.pumpAndSettle();

    await t.tap(find.byTooltip('My card'));
    await t.pumpAndSettle();
    expect(find.byType(QrImageView), findsOneWidget);
    expect(find.textContaining('only after you write to them'), findsOneWidget);
    await t.scrollUntilVisible(
      find.text('Scan their card'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    final mine = app.me.card;
    expect(mine.home, isNull);
    await t.scrollUntilVisible(
      find.text(mine.toText()),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.pageBack();
    await t.pumpAndSettle();

    await t.tap(find.byType(PopupMenuButton<Widget Function()>));
    await t.pumpAndSettle();
    await t.tap(find.text('Debug'));
    await t.pumpAndSettle();
    expect(find.text('fa4e00000001'), findsOneWidget);
    expect(find.text('Identity'), findsOneWidget);
    await t.scrollUntilVisible(
      find.text('Clock'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.scrollUntilVisible(
      find.text('Relays known'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Relays known'), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('a contact from a pasted card', (t) async {
    final app = await model(t);
    final where = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    final card = PersonCard(List.filled(32, 5), where, 1, 'Olena');
    await t.pumpWidget(BarkApp(app));
    await t.tap(find.byTooltip('New conversation'));
    await t.pumpAndSettle();
    await t.tap(find.text('Add contact'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField).first, 'not a card');
    await t.pump();
    expect(find.textContaining('Not a card'), findsOneWidget);
    await t.enterText(find.byType(TextField).first, where.toString());
    await t.pump();
    expect(find.textContaining('Not a card'), findsOneWidget);
    await t.enterText(find.byType(TextField).first, card.toText());
    await t.pump();
    await t.tap(find.text('Save'));
    await t.pumpAndSettle();
    // The new contact's thread, empty, ready to write in.
    expect(find.text('Olena'), findsOneWidget);
    expect(find.byIcon(Icons.send), findsOneWidget);
    expect(app.contacts.list.single.key, card.key);
    expect(app.contacts.list.single.verified, isFalse);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('a contact from a scanned card', (t) async {
    final app = await model(t);
    final where = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    final card = PersonCard(List.filled(32, 5), where, 1, 'Olena');
    // What My card and Add contact open once the camera has read a card.
    await t.pumpWidget(MaterialApp(home: AddContactScreen(app, card: card)));
    expect(find.text(card.toText()), findsOneWidget);
    expect(find.text('Olena'), findsOneWidget);
    await t.tap(find.text('Save'));
    await t.pumpAndSettle();
    expect(app.contacts.list.single.key, card.key);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('a stale address and a home: the send button asks where', (
    t,
  ) async {
    final app = await model(t);
    final home = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    final there = Address(Position.fromDegrees(50.1, 30.1), [6, 5, 4, 3, 2, 1]);
    final an = DateTime.now().subtract(const Duration(hours: 1));
    final olena = Contact(
      PersonCard(List.filled(32, 5), home, 1, 'Olena'),
      'Olena',
      verified: true,
    ).heard(sentOf(an), there, an);
    await t.runAsync(() => app.contacts.put(olena));
    await t.pumpWidget(MaterialApp(home: ThreadScreen(app, olena.key)));
    expect(find.text('Last wrote 1 h ago, away from home'), findsOneWidget);

    Future<void> send(String words, String choice) async {
      await t.enterText(find.byType(TextField), words);
      await t.tap(find.byTooltip('Send'));
      await t.pumpAndSettle();
      expect(find.text('Where Olena was 1 h ago'), findsOneWidget);
      expect(find.text('Their home mailbox'), findsOneWidget);
      await t.tap(find.text(choice));
      await t.pumpAndSettle();
    }

    await send('to the mailbox', 'Their home mailbox');
    var o = app.session.outbox.last;
    expect(o.body, 'to the mailbox');
    expect(o.dst.toString(), home.toString());
    expect(o.toward, Toward.home);
    expect(find.byTooltip('To their home'), findsOneWidget);

    // Per text: the next asks again.
    await send('to where she was', 'Where Olena was 1 h ago');
    o = app.session.outbox.last;
    expect(o.body, 'to where she was');
    expect(o.dst.toString(), there.toString());
    expect(o.toward, Toward.there);
    expect(find.byTooltip('To where they were'), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('the fetch button: with a home only, and why nothing is sent', (
    t,
  ) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    expect(find.byTooltip('Fetch my mail'), findsNothing);

    Future<void> fetch(String told) async {
      await t.tap(find.byTooltip('Fetch my mail'));
      await t.pump();
      await t.pump();
      expect(find.textContaining(told), findsOneWidget);
      // In, past its four seconds, and out.
      await t.pumpAndSettle();
      await t.pump(const Duration(seconds: 5));
      await t.pumpAndSettle();
    }

    // A home elsewhere, and no node.
    final away = Address(Position.fromDegrees(50.5, 30.5), [9, 9, 9, 9, 9, 9]);
    await app.me.setHome(away);
    await t.pump();
    await fetch('Not connected to a node: a fetch is sent from another node');
    expect(app.session.outbox, isEmpty);

    // Another node: the fetch goes to the home.
    await app.connectFake();
    await t.pump();
    await t.tap(find.byTooltip('Fetch my mail'));
    await t.pump();
    await t.pump();
    expect(find.text('Fetching your mail'), findsOneWidget);
    final f = app.session.outbox.where((o) => o.kind == OutKind.fetch);
    expect(f.single.dst.toString(), away.toString());

    // It waits for the home's answer, which brings the one item there.
    final waiting = find.byTooltip(
      RegExp('^Waiting for your home node to answer'),
    );
    expect(waiting, findsOneWidget);
    await t.pumpAndSettle();
    await t.pump(FakeNode.fetchAnswerAfter);
    await t.pumpAndSettle();
    expect(f.single.acked, isTrue);
    expect(find.byTooltip('Fetch my mail'), findsOneWidget);
    expect(find.textContaining('Left at your home node'), findsOneWidget);

    // The same button in a thread; now nothing is new, and it says so.
    await t.tap(find.text('Fake friend'));
    await t.pumpAndSettle();
    await t.tap(find.byTooltip('Fetch my mail'));
    await t.pump();
    expect(f.length, 2);
    expect(waiting, findsOneWidget);
    await t.pumpAndSettle();
    await t.pump(FakeNode.fetchAnswerAfter);
    await t.pump();
    await t.pumpAndSettle();
    expect(find.text('No new mail'), findsOneWidget);
    expect(find.byTooltip('Fetch my mail'), findsOneWidget);
    await t.pumpAndSettle();
    await t.pump(const Duration(seconds: 5));
    await t.pumpAndSettle();
    await t.pageBack();
    await t.pumpAndSettle();

    // The home itself: its mail is in its inbox, nothing is sent.
    await app.makeHome(app.session.address!);
    await t.pump();
    final before = app.session.outbox.length;
    await fetch('Connected to your home node');
    expect(app.session.outbox.length, before);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('the home dialog sends the new card; a move waits for a tap', (
    t,
  ) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    await app.connectFake();
    await t.pump();
    await t.tap(find.textContaining('Connected to'));
    await t.pumpAndSettle();

    Future<void> press(String button, String told) async {
      await t.scrollUntilVisible(
        find.text(button),
        100,
        scrollable: find.byType(Scrollable).first,
      );
      await t.tap(find.text(button));
      await t.pumpAndSettle();
      expect(find.textContaining(told), findsOneWidget);
      await t.tap(find.text('Go ahead'));
      await t.pumpAndSettle();
    }

    // Setting and clearing the home both change the card.
    await press('Make this node my home', 'goes to your contact.');
    expect(app.atHome, isTrue);
    // The home at a new place is still the home: no button to make it one.
    await app.session.setMapPoint(Position.fromDegrees(50.46, 30.53));
    await t.pumpAndSettle();
    expect(app.me.home.toString(), app.session.address.toString());
    expect(find.text('No home node'), findsOneWidget);
    expect(find.text('Make this node my home'), findsNothing);
    // The changed card waits for the person.
    final sent = app.session.outbox.length;
    expect(find.textContaining('home node has a new address'), findsOneWidget);
    await t.pageBack();
    await t.pumpAndSettle();
    expect(find.textContaining('home node has a new address'), findsOneWidget);
    expect(app.session.outbox.length, sent);
    await t.tap(find.text('Send my card'));
    await t.pumpAndSettle();
    expect(app.session.outbox.length, sent + app.contacts.list.length);
    expect(find.text('Send my card'), findsNothing);
    await t.tap(find.textContaining('Connected to'));
    await t.pumpAndSettle();
    await t.scrollUntilVisible(
      find.text('No home node'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Send my card'), findsNothing);
    // With the home elsewhere, the connected node is another node.
    final home = app.me.home!;
    await app.me.setHome(Address(home.pos, [1, 2, 3, 4, 5, 6]));
    await t.pumpAndSettle();
    expect(app.atHome, isFalse);
    expect(find.text('Make this node my home'), findsOneWidget);
    await app.me.setHome(home);
    await t.pumpAndSettle();
    // A move, then the dialog: the dialog sends the card, and the line goes.
    await app.session.setMapPoint(Position.fromDegrees(50.47, 30.54));
    await t.pumpAndSettle();
    await t.scrollUntilVisible(
      find.text('Send my card'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    final before = app.session.outbox.length;
    await press('No home node', 'goes to your contact.');
    expect(app.me.home, isNull);
    expect(app.session.outbox.length, before + app.contacts.list.length);
    expect(find.text('Send my card'), findsNothing);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('a connect to the node connected, or during one, does nothing', (
    t,
  ) async {
    final app = await model(t);
    await Future.wait([app.connectFake(), app.connectFake()]);
    await app.connectFake();
    expect(app.session.connected, isTrue);
    expect(app.linking, Linking.none);
    expect(
      app.session.log.where((l) => l.text.startsWith('Connected, MTU')),
      hasLength(1),
    );
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('another phone paired: no longer yours, then paired again', (
    t,
  ) async {
    final app = await model(t);
    app.setFake(true);
    await t.pumpWidget(BarkApp(app));
    expect(find.textContaining('Not paired with a node yet'), findsOneWidget);
    await app.connectFake();
    await t.pump();
    expect(find.text('Connected to bark-fa4e00000001, paired'), findsOneWidget);
    await app.session.disconnect();
    await t.pump();
    expect(
      find.textContaining('Paired with bark-fa4e00000001.'),
      findsOneWidget,
    );

    // The node takes another phone's pairing; this one learns at its next
    // connection, and may leave it so.
    app.fakeNode!.pairElsewhere();
    await t.tap(find.textContaining('Not connected'));
    await t.pumpAndSettle();
    await t.tap(find.text('Connect'));
    await t.pumpAndSettle();
    expect(find.text('This node is no longer yours'), findsOneWidget);
    await t.tap(find.text('Not now'));
    await t.pumpAndSettle();
    expect(app.session.connected, isFalse);
    expect(
      find.text(
        'bark-fa4e00000001 is no longer yours: another phone has paired '
        'with it.',
      ),
      findsOneWidget,
    );

    // Or pair again, at the node.
    await t.tap(find.text('Connect'));
    await t.pumpAndSettle();
    await t.tap(find.text('Pair again'));
    await t.pumpAndSettle();
    expect(app.session.connected, isTrue);
    expect(app.session.pairingLost, isFalse);
    expect(find.textContaining('Connected and paired'), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('the fetch button waits until a timeout or a give-back', (
    t,
  ) async {
    var clock = DateTime.utc(2026, 11, 6);
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, () => clock),
      session: Session(store: MemoryStore(), now: () => clock),
      contacts: Contacts(MemoryStore()),
      now: () => clock,
    );
    await t.runAsync(app.load);
    // A home whose answers never come back: the fake holds no key.
    app.fakeNode = FakeNode(now: app.now, sodium: sodium, app: app.me.identity);
    await t.pumpWidget(BarkApp(app));
    await app.me.setHome(
      Address(Position.fromDegrees(50.5, 30.5), [9, 9, 9, 9, 9, 9]),
    );
    await app.connectFake();
    await t.pump();
    final idle = find.byTooltip('Fetch my mail');
    final waiting = find.byTooltip(
      RegExp('^Waiting for your home node to answer'),
    );

    // Taken, and never answered: the wait ends at its bound.
    await t.tap(idle);
    await t.pump();
    await t.pump(const Duration(seconds: 1));
    expect(waiting, findsOneWidget);
    expect(find.byTooltip(RegExp(r'31:5\d at most$')), findsOneWidget);
    clock = clock.add(Session.fetchAnswerWait);
    await t.pump(const Duration(seconds: 1));
    expect(idle, findsOneWidget);

    // The fake gives every third message it takes back.
    await t.tap(idle);
    await t.pump();
    clock = clock.add(Session.fetchAnswerWait);
    await t.pump(const Duration(seconds: 1));
    await t.tap(idle);
    await t.pump();
    expect(waiting, findsOneWidget);
    await t.pump(FakeNode.handOver);
    await t.pump();
    final fetches = app.session.outbox.where((o) => o.kind == OutKind.fetch);
    expect(fetches.last.state, OutState.givenBack);
    expect(idle, findsOneWidget);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });
}
