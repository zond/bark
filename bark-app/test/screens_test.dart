// The screens against the fake node: they build; a text goes to the fake
// friend and its echo comes back into the same thread, sealed both ways;
// the echo is acknowledged; the card, the backup words, adding a contact
// from a pasted or a scanned card, and the debug screen; the fetch button,
// on the first screen and in a thread, and why it sends nothing.

import 'package:bark_app/app.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/contacts_screen.dart';
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
    expect(find.textContaining('taken by the node'), findsOneWidget);
    await t.pump(FakeNode.echoAfter + const Duration(seconds: 1));
    expect(find.text('Echo: hello'), findsOneWidget);
    expect(find.textContaining('Not verified yet'), findsNothing);

    // Acknowledge the echo.
    await t.tap(find.text('Acknowledge').first);
    await t.pump();
    await t.pump();
    expect(find.textContaining('you acknowledged it'), findsOneWidget);
    await t.pageBack();
    await t.pumpAndSettle();

    await t.tap(find.byTooltip('My card'));
    await t.pumpAndSettle();
    expect(find.byType(QrImageView), findsOneWidget);
    await t.scrollUntilVisible(
      find.text('Scan their card'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    final mine = app.me.card!;
    expect(
      mine.address.toString(),
      Address(FakeNode.gps, FakeNode.id).toString(),
    );
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
    await fetch('Fetching your mail');
    final f = app.session.outbox.where((o) => o.kind == OutKind.fetch);
    expect(f.single.dst.toString(), away.toString());

    // The same button in a thread.
    await t.pumpAndSettle();
    await t.tap(find.text('Fake friend'));
    await t.pumpAndSettle();
    await fetch('Fetching your mail');
    expect(f.length, 2);
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

  testWidgets('the home dialog says a card is sent only when one is', (
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

    // The card already carries this node's address.
    await press('Make this node my home', 'stays as it is, so no card');
    expect(app.atHome, isTrue);
    await press('No home node', 'stays as it is, so no card');
    expect(app.me.home, isNull);

    // A home elsewhere: making this node the home moves the card.
    final away = Address(Position.fromDegrees(50.5, 30.5), [9, 9, 9, 9, 9, 9]);
    await app.me.setHome(away);
    await t.pump();
    await press('Make this node my home', 'goes to your contact.');
    expect(app.atHome, isTrue);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });
}
