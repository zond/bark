// The archive on screen: a row swiped aside, with an undo; the line at
// the bottom of each list and the Archived screen it leads to, also from
// the menu; a thread archived from its menu; an archived thread read-only
// until Unarchive.

import 'package:bark_app/app.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/archived_screen.dart';
import 'package:bark_app/ui/contacts_screen.dart';
import 'package:bark_app/ui/thread_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());
  final home = Address(Position.fromDegrees(50, 30), List.filled(6, 1));
  Contact named(int n, String name) =>
      Contact(PersonCard(List.filled(32, n), home, 1, name), name);
  final olena = named(5, 'Olena'), petro = named(6, 'Petro');

  /// The app with a text from each of Olena and Petro.
  Future<AppModel> model(WidgetTester t) async {
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, DateTime.now),
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
      now: DateTime.now,
    );
    await t.runAsync(() async {
      await app.load();
      for (final (i, c) in [olena, petro].indexed) {
        await app.contacts.put(c);
        await app.session.addReceived(
          Received(
            i,
            [i],
            DateTime.now(),
            from: c.key,
            sent: i + 1,
            body: 'from ${c.name}',
          ),
        );
      }
    });
    return app;
  }

  Future<void> done(WidgetTester t, AppModel app) async {
    await t.pumpWidget(const SizedBox());
    app.dispose();
  }

  testWidgets('a conversation swiped aside, undone, then archived and '
      'brought back from the Archived screen', (t) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    expect(find.text('Olena'), findsOneWidget);
    expect(find.textContaining('archived'), findsNothing);

    await t.drag(find.text('Olena'), const Offset(-600, 0));
    await t.pumpAndSettle();
    expect(find.text('Olena'), findsNothing);
    expect(find.text('Olena: conversation archived'), findsOneWidget);
    expect(find.text('1 archived'), findsOneWidget);
    await t.tap(find.text('Undo'));
    await t.pumpAndSettle();
    expect(find.text('Olena'), findsOneWidget);
    expect(find.text('1 archived'), findsNothing);

    await t.drag(find.text('Olena'), const Offset(600, 0));
    await t.pumpAndSettle();
    expect(app.contacts.isArchived(olena.key), isTrue);
    // The contact stays in the contacts.
    expect(app.contacts.byKey(olena.key)!.archived, isFalse);
    await t.tap(find.text('1 archived'));
    await t.pumpAndSettle();
    expect(find.byType(ArchivedScreen), findsOneWidget);
    expect(find.text('Conversations'), findsOneWidget);
    expect(find.text('Olena'), findsOneWidget);
    expect(find.text('Petro'), findsNothing);
    await t.tap(find.byTooltip('Unarchive'));
    await t.pumpAndSettle();
    expect(find.textContaining('Nothing archived'), findsOneWidget);
    await t.pageBack();
    await t.pumpAndSettle();
    expect(find.text('Olena'), findsOneWidget);
    await done(t, app);
  });

  testWidgets('a contact swiped aside takes its conversation; the menu '
      'leads to both', (t) async {
    final app = await model(t);
    await t.pumpWidget(MaterialApp(home: ContactsScreen(app)));
    await t.drag(find.text('Petro'), const Offset(-600, 0));
    await t.pumpAndSettle();
    expect(find.text('Petro'), findsNothing);
    expect(find.text('Petro: contact archived'), findsOneWidget);
    expect(find.text('1 archived'), findsOneWidget);
    expect(app.contacts.isArchived(petro.key), isTrue);

    await t.pumpWidget(BarkApp(app));
    await t.pumpAndSettle();
    expect(find.text('Petro'), findsNothing);
    await t.tap(find.byType(PopupMenuButton<Widget Function()>));
    await t.pumpAndSettle();
    await t.tap(find.text('Archived'));
    await t.pumpAndSettle();
    expect(find.text('Conversations'), findsOneWidget);
    expect(find.text('Contacts'), findsOneWidget);
    expect(find.text('Petro'), findsNWidgets(2));
    await t.tap(find.byTooltip('Unarchive').first);
    await t.pumpAndSettle();
    expect(find.text('Petro'), findsNothing);
    expect(app.contacts.byKey(petro.key)!.archived, isFalse);
    await done(t, app);
  });

  testWidgets('archived from the thread\'s menu; read-only until '
      'Unarchive', (t) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    await t.tap(find.text('Olena'));
    await t.pumpAndSettle();
    expect(find.byType(ThreadScreen), findsOneWidget);
    expect(find.byTooltip('Acknowledge'), findsOneWidget);
    await t.tap(find.byType(PopupMenuButton<void Function()>));
    await t.pumpAndSettle();
    await t.tap(find.text('Archive contact'));
    await t.pumpAndSettle();
    // Back on the first screen, which no longer shows it.
    expect(find.byType(ThreadScreen), findsNothing);
    expect(find.text('Olena: contact archived'), findsOneWidget);
    expect(find.text('1 archived'), findsOneWidget);
    // Let the snack bar go: its Undo would cover the thread's button.
    await t.pump(const Duration(seconds: 5));
    await t.pumpAndSettle();

    await t.tap(find.text('1 archived'));
    await t.pumpAndSettle();
    await t.tap(find.text('Olena').first);
    await t.pumpAndSettle();
    expect(find.text('from Olena'), findsOneWidget);
    expect(find.text('This conversation is archived.'), findsOneWidget);
    expect(find.byType(TextField), findsNothing);
    expect(find.byTooltip('Acknowledge'), findsNothing);
    expect(find.byType(PopupMenuButton<void Function()>), findsNothing);
    await t.tap(find.text('Unarchive'));
    await t.pumpAndSettle();
    expect(find.byType(TextField), findsOneWidget);
    expect(find.byTooltip('Acknowledge'), findsOneWidget);
    expect(app.contacts.byKey(olena.key)!.archived, isFalse);
    await done(t, app);
  });
}
