// The screens against the fake node: they build, and a text goes from
// a thread to the fake and its echo comes back into the same thread.

import 'package:bark_app/app.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qr_flutter/qr_flutter.dart';

void main() {
  testWidgets('conversations, a thread, settings, card and debug', (t) async {
    final app = AppModel(
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
    );
    final olena = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    await app.contacts.put(Contact(PersonCard(olena, 'O.'), 'Olena'));
    await t.pumpWidget(BarkApp(app));
    expect(find.textContaining('No conversations yet'), findsOneWidget);

    // Settings: the fake node.
    await t.tap(find.textContaining('Not connected'));
    await t.pumpAndSettle();
    await t.tap(find.text('Fake node'));
    await t.pump();
    await t.tap(find.text('Connect'));
    await t.pump();
    expect(find.text('bark-fa4e00000001'), findsOneWidget);
    expect(find.text('Map point'), findsOneWidget);
    await t.pageBack();
    await t.pumpAndSettle();

    // The fake's two messages, from itself, named by its address.
    final own = Address(FakeNode.gps, FakeNode.id).toString();
    expect(find.text(own), findsOneWidget);
    expect(find.textContaining('A second message'), findsOneWidget);

    // A text to Olena, and its echo in her thread.
    await t.tap(find.byTooltip('New conversation'));
    await t.pumpAndSettle();
    await t.tap(find.text('Olena'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'hello');
    await t.tap(find.byTooltip('Send'));
    await t.pump();
    expect(find.text('hello'), findsOneWidget);
    expect(find.textContaining('taken by the node'), findsOneWidget);
    await t.pump(FakeNode.echoAfter + const Duration(seconds: 1));
    expect(find.text('Echo: hello'), findsOneWidget);
    await t.pageBack();
    await t.pumpAndSettle();
    expect(find.text('Olena'), findsOneWidget);

    await t.tap(find.byTooltip('My card'));
    await t.pumpAndSettle();
    expect(find.byType(QrImageView), findsOneWidget);
    await t.scrollUntilVisible(
      find.text(own),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    final mine = PersonCard(Address(FakeNode.gps, FakeNode.id), '');
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
    expect(find.text('Clock'), findsOneWidget);
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

  testWidgets('a contact from a pasted card or a pasted address', (t) async {
    final app = AppModel(
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
    );
    final a = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    final b = Address(Position.fromDegrees(51, 31), [6, 5, 4, 3, 2, 1]);
    await t.pumpWidget(BarkApp(app));
    for (final (pasted, name) in [
      (PersonCard(a, 'Olena').toText(), null),
      (b.toString(), 'Petro'),
    ]) {
      await t.tap(find.byTooltip('New conversation'));
      await t.pumpAndSettle();
      await t.tap(find.text('Add contact'));
      await t.pumpAndSettle();
      await t.enterText(find.byType(TextField).first, pasted);
      await t.pump();
      if (name != null) await t.enterText(find.byType(TextField).last, name);
      await t.pump();
      await t.tap(find.text('Save'));
      await t.pumpAndSettle();
      // The new contact's thread, empty, ready to write in.
      expect(find.text(name ?? 'Olena'), findsOneWidget);
      expect(find.byIcon(Icons.send), findsOneWidget);
      await t.pageBack();
      await t.pumpAndSettle();
    }
    expect(app.contacts.list.map((c) => c.name), ['Olena', 'Petro']);
    expect(app.contacts.list.last.address.toString(), b.toString());

    await t.tap(find.byTooltip('New conversation'));
    await t.pumpAndSettle();
    await t.tap(find.text('Add contact'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField).first, 'not a card');
    await t.pump();
    expect(find.textContaining('Not a card'), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });
}
