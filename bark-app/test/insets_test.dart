// On an edge-to-edge window the system navigation bar overlaps the bottom of
// the screen; the last row of a scrolled-to-end screen must stay above it.

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/settings_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());

  testWidgets('the last row of Settings sits above the navigation bar', (
    t,
  ) async {
    // A three-button bar is 48 dp high.
    const bar = 48.0;
    final dpr = t.view.devicePixelRatio;
    t.view.padding = FakeViewPadding(bottom: bar * dpr);
    t.view.viewPadding = FakeViewPadding(bottom: bar * dpr);
    addTearDown(t.view.reset);

    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, DateTime.now),
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
      now: DateTime.now,
    );
    await t.runAsync(app.load);
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.drag(find.byType(Scrollable).first, const Offset(0, -10000));
    await t.pumpAndSettle();

    final screen = t.view.physicalSize.height / dpr;
    expect(
      t.getRect(find.text('Restore from words')).bottom,
      lessThanOrEqualTo(screen - bar),
    );

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });
}
