// The owner's and the given home's screens: Share my home node in a
// thread's menu for the owner alone; a home given, on the first screen
// and in its thread, with Use it; the owner's list in Settings away from
// the home; and Use my existing home, by an address typed.

import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/settings_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

import 'fake_ble.dart';
import 'fake_node.dart';

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());

  Future<AppModel> model(WidgetTester t) async {
    // The test's clock, which its timers follow, so that a message given
    // back goes again.
    DateTime now() => t.binding.clock.now();
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, now),
      session: Session(store: MemoryStore(), now: now),
      contacts: Contacts(MemoryStore()),
      now: now,
    );
    await t.runAsync(app.load);
    return app;
  }

  Future<void> done(WidgetTester t, AppModel app) async {
    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.disposeWithNode();
  }

  /// Past a snack bar's four seconds.
  Future<void> settle(WidgetTester t) async {
    await t.pumpAndSettle();
    await t.pump(const Duration(seconds: 5));
    await t.pumpAndSettle();
  }

  testWidgets('a thread\'s menu shares the home for its owner alone', (
    t,
  ) async {
    final app = await model(t);
    await app.connectFake();
    await t.pumpWidget(BarkApp(app));
    await t.pumpAndSettle();
    await t.tap(find.text('Fake friend'));
    await t.pumpAndSettle();
    await t.tap(find.byType(PopupMenuButton<void Function()>));
    await t.pumpAndSettle();
    expect(find.text('Share my home node'), findsNothing);
    await t.tapAt(Offset.zero);
    await t.pumpAndSettle();

    await app.mailbox.makeHome(app.session.address!);
    await t.pumpAndSettle();
    expect(app.mailbox.isOwner, isTrue);
    await t.tap(find.byType(PopupMenuButton<void Function()>));
    await t.pumpAndSettle();
    await t.tap(find.text('Share my home node'));
    await t.pump();
    await t.pump();
    expect(find.textContaining('Sent to your home'), findsOneWidget);
    expect(
      app.session.outbox.where((o) => o.kind == OutKind.fetch).single.command,
      isNotNull,
    );
    await settle(t);
    await done(t, app);
  });

  testWidgets('a home given shows with Use it, which makes it the home', (
    t,
  ) async {
    final app = await model(t);
    await app.connectFake();
    final friend = app.contacts.list.single;
    final given = Address(Position.fromDegrees(50.6, 30.6), [7, 7, 7, 7, 7, 7]);
    final key = Uint8List.fromList(List.filled(32, 0x42));
    await app.contacts.put(friend.copyWith(gift: (home: given, key: key)));
    await t.pumpWidget(BarkApp(app));
    await t.pumpAndSettle();
    final line = 'Fake friend gave you a home at ${nodeLabel(given.id)}';
    expect(find.text(line), findsOneWidget);
    await t.tap(find.text('Fake friend'));
    await t.pumpAndSettle();
    expect(find.text(line), findsOneWidget);
    await t.tap(find.text('Use it'));
    await t.pump();
    await t.pump();
    expect(find.text('${nodeLabel(given.id)} is your home'), findsOneWidget);
    expect(find.text(line), findsNothing);
    expect(app.me.home.toString(), given.toString());
    expect(hex(app.me.mailboxKey.extractBytes()), hex(key));
    await settle(t);
    await done(t, app);
  });

  testWidgets('Settings away: the owner\'s list, asked for, and a removal by '
      'command; Use my existing home asks the address typed', (t) async {
    final app = await model(t);
    await app.connectFake();
    final f = app.fakeNode!;
    final away = Address(Position.fromDegrees(50.5, 30.5), [9, 9, 9, 9, 9, 9]);

    // No home yet: the person names theirs.
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pumpAndSettle();
    await t.tap(find.text('Use my existing home'));
    await t.pumpAndSettle();
    expect(find.text('Ask it'), findsOneWidget);
    await t.enterText(find.byKey(const Key('home address')), 'nonsense');
    await t.pump();
    expect(find.text('Not an address or a card'), findsOneWidget);
    await t.enterText(find.byKey(const Key('home address')), away.toString());
    await t.pump();
    await t.tap(find.text('Ask it'));
    await t.pump();
    await t.pump();
    expect(
      find.text('Asking ${nodeLabel(away.id)} for your mail…'),
      findsOneWidget,
    );
    await t.pump(FakeNode.fetchAnswerAfter);
    await t.pumpAndSettle();
    expect(app.me.home.toString(), away.toString());
    expect(app.mailbox.isOwner, isTrue);
    await settle(t);

    // The owner's list, from away.
    f.homeRecords.add(
      FakeHomeRecord(2, List.filled(32, 2), tag: 5, name: 'Bo'),
    );
    expect(find.text('People with a home here'), findsOneWidget);
    await t.ensureVisible(find.text('Show them'));
    await t.tap(find.text('Show them'));
    // Given back or answered, the home has it within the fake's waits.
    for (var i = 0; i < 40 && app.mailbox.listed == null; i++) {
      await t.pump(const Duration(seconds: 1));
    }
    await t.pumpAndSettle();
    expect(find.text('Bo'), findsOneWidget);
    expect(find.text('You, owner, seen today'), findsOneWidget);
    await settle(t);
    final choices = find.byTooltip('Choices');
    await t.ensureVisible(choices);
    await t.tap(choices);
    await t.pumpAndSettle();
    await t.tap(find.text('Remove'));
    await t.pumpAndSettle();
    expect(find.text('Remove Bo\'s home?'), findsOneWidget);
    await t.tap(find.widgetWithText(FilledButton, 'Remove'));
    await t.pump();
    await t.pump();
    // Given back or answered, the home has it within the fake's waits.
    for (var i = 0; i < 40 && f.homeRecords.length > 1; i++) {
      await t.pump(const Duration(seconds: 1));
    }
    expect(f.homeRecords.map((r) => r.name), ['Me']);
    await settle(t);
    await done(t, app);
  });
}
