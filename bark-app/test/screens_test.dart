// The screens against the fake node: they build; a text goes to the fake
// friend and its echo comes back into the same thread, sealed both ways;
// the echo is acknowledged, its button no taller than the small text;
// the card, and without a name a prompt in its place, the backup words,
// adding a contact from a pasted card or from a scanned one without a
// name, its thread opened at once, and the debug screen; the fetch button, on the first screen and
// in a thread, and why it sends nothing; the home
// buttons, the home known by its node ID wherever it is, and its card
// sent after a move only by "Send my card"; and a node that another
// phone has paired with since, and pairing with it again; and a long
// text, counted in parts as it is written, sent in one bubble and
// received as far as it has come; and a first message marked in a
// verified thread; and the owner's Public switch, and a guest's view.

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
import 'package:bark_app/ui/card_screen.dart';
import 'package:bark_app/ui/contacts_screen.dart';
import 'package:bark_app/ui/debug_screen.dart';
import 'package:bark_app/ui/conversations_screen.dart';
import 'package:bark_app/ui/settings_screen.dart';
import 'package:bark_app/ui/thread_screen.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:qr_flutter/qr_flutter.dart';
import 'package:sodium/sodium.dart';

/// The one check's words: no news from the network past it.
const sentTip = 'Sent into the network; no further news until they confirm';

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
    // No contact yet, so nobody to lose: no backup banner.
    expect(find.textContaining('these words'), findsNothing);

    // Settings: the demo at the bottom, and the backup words.
    await t.tap(find.textContaining('Not connected'));
    await t.pumpAndSettle();
    await t.scrollUntilVisible(
      find.text('Try without a node (demo)'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.tap(find.text('Try without a node (demo)'));
    await t.pump();
    await t.scrollUntilVisible(
      find.text('Connect'),
      -100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.tap(find.text('Connect'));
    await t.pump();
    expect(find.text('ID FA4E00000001'), findsOneWidget);
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
      find.text('Node\'s place'),
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
    expect(find.byTooltip(sentTip), findsOneWidget);
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
    final taken = find.byTooltip(sentTip);
    final button = find.byTooltip('Tell the sender you read it');
    expect(inBubble('Echo: hello', button), findsOneWidget);
    expect(inBubble('Echo: hello', mark), findsNothing);
    expect(inBubble('hello', mark), findsNothing);
    expect(inBubble('hello', taken), findsOneWidget);
    // A labelled chip with a tap target of 48 dp, and a hint, once.
    expect(inBubble('Echo: hello', find.text('Got it')), findsOneWidget);
    expect(
      t.getSize(inBubble('Echo: hello', find.byType(ActionChip))).height,
      greaterThanOrEqualTo(48),
    );
    const hint = 'Tap Got it so the sender sees you read it.';
    expect(find.text(hint), findsOneWidget);
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
    expect(find.text(hint), findsNothing);
    // A tap on a state explains them all.
    await t.tap(inBubble('hello', mark));
    await t.pumpAndSettle();
    expect(find.text('Confirmed by the recipient'), findsOneWidget);
    expect(
      find.textContaining('The network gives no further news'),
      findsOneWidget,
    );
    await t.tapAt(Offset.zero);
    await t.pumpAndSettle();
    await t.pageBack();
    await t.pumpAndSettle();

    await t.runAsync(() => app.me.setName('Ana'));
    await t.tap(find.byTooltip('My card'));
    await t.pumpAndSettle();
    expect(find.byType(QrImageView), findsOneWidget);
    // No home: a temporary card, marked so on the code.
    expect(find.textContaining('Temporary card:'), findsOneWidget);
    expect(find.byType(TemporaryFrame), findsOneWidget);
    expect(find.text('TEMPORARY'), findsOneWidget);
    // The code first, then Share and the rest; the hex in Details.
    expect(t.getTopLeft(find.byType(QrImageView)).dy, lessThan(200));
    await t.scrollUntilVisible(
      find.text('Share'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    final mine = app.me.card;
    expect(mine.home, isNull);
    expect(find.text(mine.toText()), findsNothing);
    await t.scrollUntilVisible(
      find.text('Details'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.tap(find.text('Details'));
    await t.pumpAndSettle();
    await t.scrollUntilVisible(
      find.text(mine.toText()),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text(app.me.key), findsOneWidget);
    // What Share hands the share sheet: a PNG of the code.
    final png = (await t.runAsync(() => qrPng(mine.toText())))!;
    expect(png.sublist(0, 4), [0x89, 0x50, 0x4e, 0x47]);
    await t.pageBack();
    await t.pumpAndSettle();

    // Debug is at the bottom of Settings.
    await t.tap(find.byType(PopupMenuButton<Widget Function()>));
    await t.pumpAndSettle();
    await t.tap(find.text('Settings'));
    await t.pumpAndSettle();
    await t.scrollUntilVisible(
      find.text('Debug'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.ensureVisible(find.text('Debug'));
    await t.pumpAndSettle();
    await t.tap(find.text('Debug'));
    await t.pumpAndSettle();
    expect(find.byType(DebugScreen), findsOneWidget);
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

  testWidgets('without a name, My card asks for one in place of the card', (
    t,
  ) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    await t.tap(find.byTooltip('My card'));
    await t.pumpAndSettle();
    expect(find.textContaining('Set your name first'), findsOneWidget);
    expect(find.byType(QrImageView), findsNothing);
    expect(find.text('Card'), findsNothing);
    expect(find.text('Scan their card'), findsOneWidget);
    await t.tap(find.text('Set my name'));
    await t.pumpAndSettle();
    await t.enterText(find.byType(TextField), 'Ana');
    await t.tap(find.text('Save'));
    await t.pumpAndSettle();
    expect(app.me.name, 'Ana');
    expect(find.textContaining('Set your name first'), findsNothing);
    expect(find.byType(QrImageView), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('a scanned card without a name opens its thread, Unnamed', (
    t,
  ) async {
    final app = await model(t);
    final where = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    final card = PersonCard(List.filled(32, 5), where, 1, '');
    await t.runAsync(() => app.me.setName('Ana'));
    await t.pumpWidget(BarkApp(app));
    await t.tap(find.byTooltip('My card'));
    await t.pumpAndSettle();
    // What My card and Add contact do once the camera has read a card.
    await openScanned(t.element(find.byType(CardScreen)), app, card);
    await t.pumpAndSettle();
    const unnamed = 'Unnamed (05050505…)';
    expect(find.byType(ThreadScreen), findsOneWidget);
    expect(find.text(unnamed), findsOneWidget);
    expect(find.byIcon(Icons.send), findsOneWidget);
    expect(app.contacts.list.single.key, card.key);

    // Back leads to the conversations; the contact is in Contacts, a
    // button of its own and no longer in the menu.
    await t.pageBack();
    await t.pumpAndSettle();
    expect(find.byType(ConversationsScreen), findsOneWidget);
    await t.tap(find.byType(PopupMenuButton<Widget Function()>));
    await t.pumpAndSettle();
    expect(find.text('Contacts'), findsNothing);
    expect(find.text('Nearby nodes'), findsOneWidget);
    await t.tapAt(Offset.zero);
    await t.pumpAndSettle();
    await t.tap(find.byTooltip('Contacts'));
    await t.pumpAndSettle();
    expect(find.byType(ContactsScreen), findsOneWidget);
    expect(find.text(unnamed), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('Get started: three steps ticked as they are done, then gone', (
    t,
  ) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    await t.pump();
    expect(find.text('Get started'), findsOneWidget);
    expect(find.byIcon(Icons.check_circle), findsNothing);
    await t.runAsync(() => app.me.setName('Ana'));
    await t.pump();
    expect(find.byIcon(Icons.check_circle), findsOneWidget);
    await t.runAsync(
      () => app.contacts.add(PersonCard(List.filled(32, 5), null, 1, 'Olena')),
    );
    await t.pump();
    expect(find.byIcon(Icons.check_circle), findsNWidgets(2));
    expect(find.text('Get started'), findsOneWidget);
    await app.connectFake();
    await t.pump();
    expect(find.text('Get started'), findsNothing);
    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('the home: a node without a place offers the phone\'s '
      'location; a sleepy one cannot be made home', (t) async {
    final app = await model(t);
    await app.connectFake();
    final fake = app.fakeNode!
      ..fixed = false
      ..changed();
    await t.pump();
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.scrollUntilVisible(
      find.text('This node has no place yet.'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Use this phone\'s location'), findsOneWidget);
    expect(find.text('Make this node my home'), findsNothing);

    fake
      ..fixed = true
      ..sleepy = true
      ..changed();
    await t.pump();
    expect(find.text('This node has no place yet.'), findsNothing);
    final make = find.widgetWithText(FilledButton, 'Make this node my home');
    expect(t.widget<FilledButton>(make).enabled, isFalse);
    expect(
      find.text('A home must be a node that stays on power at a fixed place.'),
      findsOneWidget,
    );
    fake
      ..sleepy = false
      ..changed();
    await t.pump();
    expect(t.widget<FilledButton>(make).enabled, isTrue);
    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('the backup banner shows once there is a contact, and lands '
      'on the backup, tinted for a moment', (t) async {
    final app = await model(t);
    await t.pumpWidget(BarkApp(app));
    const banner = 'Write down your 12 backup words';
    expect(find.text(banner), findsNothing);
    await t.runAsync(
      () => app.contacts.add(PersonCard(List.filled(32, 5), null, 1, 'Olena')),
    );
    await t.pump();
    expect(find.text(banner), findsOneWidget);
    expect(
      find.textContaining('the only way to keep your contacts'),
      findsOneWidget,
    );
    await t.tap(find.text(banner));
    await t.pump();
    await t.pump(const Duration(milliseconds: 100));
    expect(find.byType(SettingsScreen), findsOneWidget);
    final words = find.text('Show my words');
    Color? tint() {
      final c = find.ancestor(
        of: words,
        matching: find.byType(AnimatedContainer),
      );
      final box = t.renderObject<RenderDecoratedBox>(
        find.descendant(of: c.first, matching: find.byType(DecoratedBox)).first,
      );
      return (box.decoration as BoxDecoration).color;
    }

    // Tinted at first, the tint gone after the fade, and in view.
    expect(tint()!.a, greaterThan(0));
    await t.pumpAndSettle();
    expect(tint()!.a, 0);
    final view = t.view.physicalSize / t.view.devicePixelRatio;
    final at = t.getRect(words);
    expect(at.top, greaterThanOrEqualTo(0));
    expect(at.bottom, lessThanOrEqualTo(view.height));
    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('Remove contact and Restore from words ask first', (t) async {
    final app = await model(t);
    final olena = Contact(
      PersonCard(List.filled(32, 5), null, 1, 'Olena'),
      'Olena',
    );
    await t.runAsync(() => app.contacts.put(olena));
    await t.pumpWidget(MaterialApp(home: ThreadScreen(app, olena.key)));
    Future<void> remove(String choice) async {
      await t.tap(find.byType(PopupMenuButton<void Function()>));
      await t.pumpAndSettle();
      await t.tap(find.text('Remove contact'));
      await t.pumpAndSettle();
      expect(find.text('Remove Olena?'), findsOneWidget);
      await t.tap(find.text(choice));
      await t.runAsync(() => Future<void>.delayed(Duration.zero));
      await t.pumpAndSettle();
    }

    await remove('Cancel');
    expect(app.contacts.byKey(olena.key), isNotNull);
    await remove('Remove');
    expect(app.contacts.byKey(olena.key), isNull);

    final key = app.me.key;
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.scrollUntilVisible(
      find.text('Restore from words'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.ensureVisible(find.text('Restore from words'));
    await t.pumpAndSettle();
    await t.tap(find.text('Restore from words'));
    await t.pumpAndSettle();
    expect(find.textContaining('cannot reach the new one'), findsOneWidget);
    await t.tap(find.text('Cancel'));
    await t.pumpAndSettle();
    expect(find.text('Your 12 words'), findsNothing);
    expect(app.me.key, key);
    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('a temporary contact: marked in its row until a card with a '
      'home, and written to at the node on its card', (t) async {
    final app = await model(t);
    final home = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    final node = Address(Position.fromDegrees(50.1, 30.1), [6, 5, 4, 3, 2, 1]);
    final id = List.filled(32, 5);
    await t.runAsync(
      () => app.contacts.add(PersonCard(id, null, 1, 'Olena', now: node)),
    );
    await t.pumpWidget(MaterialApp(home: ContactsScreen(app)));
    expect(
      find.text('temporary: reach them at the node they last used'),
      findsOneWidget,
    );
    expect(find.byIcon(Icons.hourglass_empty), findsOneWidget);

    // Writable: no dead end, and the text goes to the node on the card.
    await t.runAsync(() => app.me.setNodeAddress(home));
    await t.pumpWidget(MaterialApp(home: ThreadScreen(app, hex(id))));
    expect(find.byType(TextField), findsOneWidget);
    await t.enterText(find.byType(TextField), 'hi');
    await t.tap(find.byTooltip('Send'));
    await t.pumpAndSettle();
    final out = app.session.outbox.single;
    expect(out.dst.toString(), node.toString());
    expect(out.toward, Toward.node);

    await t.runAsync(
      () => app.contacts.add(PersonCard(id, home, 2, 'Olena', now: node)),
    );
    await t.pumpWidget(MaterialApp(home: ContactsScreen(app)));
    expect(find.text('Has a home node'), findsOneWidget);
    expect(find.byIcon(Icons.hourglass_empty), findsNothing);
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
      expect(find.text('Fast if they are still near there'), findsOneWidget);
      expect(
        find.text('Waits at their home until they fetch it'),
        findsOneWidget,
      );
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

  testWidgets('a first message in a verified thread is marked', (t) async {
    final app = await model(t);
    final olena = Contact(
      PersonCard(List.filled(32, 5), null, 1, 'Olena'),
      'Olena',
      verified: true,
    );
    await t.runAsync(() async {
      await app.contacts.put(olena);
      for (final (sent, intro) in [(1, true), (2, false)]) {
        await app.session.addReceived(
          Received(
            sent,
            [sent],
            DateTime.now(),
            from: olena.key,
            sent: sent,
            body: 'text $sent',
            intro: intro,
          ),
        );
      }
    });
    await t.pumpWidget(MaterialApp(home: ThreadScreen(app, olena.key)));
    expect(find.byTooltip(unverifiedText), findsOneWidget);
    expect(find.textContaining('Not verified yet'), findsNothing);

    // Unverified and made from a first message, the line above the
    // thread says it of every text.
    await t.runAsync(
      () => app.contacts.put(
        olena.copyWith(verified: false, cameByMessage: true),
      ),
    );
    await t.pump();
    expect(find.byTooltip(unverifiedText), findsNothing);
    expect(find.textContaining('Not verified yet'), findsOneWidget);

    // Unverified but from a card the person scanned: no line, as the card
    // came from its holder's hand; a first message is still marked.
    await t.runAsync(
      () => app.contacts.put(
        olena.copyWith(verified: false, cameByMessage: false),
      ),
    );
    await t.pump();
    expect(find.textContaining('Not verified yet'), findsNothing);
    expect(find.byTooltip(unverifiedText), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('a long text: its parts counted, one bubble, states summed', (
    t,
  ) async {
    final app = await model(t);
    final there = Address(Position.fromDegrees(50.1, 30.1), [6, 5, 4, 3, 2, 1]);
    final now = DateTime.now();
    final olena = Contact(
      PersonCard(List.filled(32, 5), null, 1, 'Olena'),
      'Olena',
      verified: true,
    ).heard(sentOf(now), there, now);
    await t.runAsync(() => app.contacts.put(olena));
    await t.pumpWidget(MaterialApp(home: ThreadScreen(app, olena.key)));

    // A box without an address holds 156 bytes of text, a part 155.
    await t.enterText(find.byType(TextField), 'a' * 156);
    await t.pump();
    expect(find.textContaining('parts'), findsNothing);
    await t.enterText(find.byType(TextField), 'a' * 157);
    await t.pump();
    expect(find.text('2 parts'), findsOneWidget);
    final long = List.filled(40, 'word').join(' ');
    await t.enterText(find.byType(TextField), long);
    await t.pump();
    expect(find.text('2 parts'), findsOneWidget);
    // Cyrillic counts one byte a letter.
    await t.enterText(find.byType(TextField), 'ї' * 300);
    await t.pump();
    expect(find.text('2 parts'), findsOneWidget);
    await t.enterText(find.byType(TextField), 'x' * 3000);
    await t.pump();
    expect(find.text('20 parts, 16 at most'), findsOneWidget);

    // Sent: one bubble with the whole text; the parts agree, so the
    // usual mark.
    await t.enterText(find.byType(TextField), long);
    await t.tap(find.byTooltip('Send'));
    await t.pumpAndSettle();
    final out = app.session.outbox;
    expect(out.map((o) => o.part), [1, 2]);
    expect(find.text(long), findsOneWidget);
    expect(find.byTooltip('Waiting on this phone'), findsOneWidget);

    Future<void> redraw() async {
      app.session.dismissNotice();
      await t.pump();
    }

    // They differ: in words.
    out[0].state = OutState.taken;
    await redraw();
    expect(find.text('1 of 2 sent'), findsOneWidget);
    expect(find.byTooltip('Waiting on this phone'), findsNothing);
    out[1].state = OutState.taken;
    await redraw();
    expect(find.byTooltip(sentTip), findsOneWidget);
    final both = find.byWidgetPredicate(
      (w) => w is Icon && w.icon == Icons.done_all,
    );
    out[0].acked = true;
    await redraw();
    expect(find.text('2 of 2 sent, 1 confirmed'), findsOneWidget);
    expect(both, findsNothing);
    out[1].acked = true;
    await redraw();
    expect(both, findsOneWidget);
    expect(find.textContaining('of 2'), findsNothing);

    // Received: the parts that came, a line where one has not, and the
    // button until every part that came is acknowledged.
    Future<void> receive(int sent, int part, String body) =>
        app.session.addReceived(
          Received(
            sent,
            [sent],
            DateTime.now(),
            from: olena.key,
            sent: sent,
            body: body,
            part: part,
            of: 3,
          ),
        );
    await t.runAsync(() => receive(7002, 3, 'three'));
    await t.runAsync(() => receive(7000, 1, 'one '));
    await t.pump();
    expect(find.textContaining('part 2 of 3 not yet here'), findsOneWidget);
    expect(find.byTooltip('Tell the sender you read it'), findsOneWidget);
    for (final r in app.session.received) {
      r.acked = true;
    }
    await redraw();
    expect(find.byTooltip('Tell the sender you read it'), findsNothing);
    await t.runAsync(() => receive(7001, 2, 'two '));
    await t.pump();
    expect(find.textContaining('not yet here'), findsNothing);
    expect(find.text('one two three'), findsOneWidget);
    expect(find.byTooltip('Tell the sender you read it'), findsOneWidget);

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
    // Queued until a node links, then sent from it to the home.
    await fetch('The fetch waits for a node');
    expect(app.session.outbox, isEmpty);
    expect(find.byTooltip('Fetch waits for a node'), findsOneWidget);
    await app.connectFake();
    await t.pump();
    await t.pump();
    expect(app.me.fetchQueued, isFalse);
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
      // Built below the screen's edge already, it is found, not shown.
      await t.ensureVisible(find.text(button));
      await t.pumpAndSettle();
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
    expect(find.text('Connected to ID FA4E00000001, paired'), findsOneWidget);
    await app.session.disconnect();
    await t.pump();
    expect(find.textContaining('Paired with ID FA4E00000001.'), findsOneWidget);

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
        'ID FA4E00000001 is no longer yours: another phone has paired '
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
    // The node's health in words, not the link's MTU: the fake is in its
    // first rotation.
    expect(find.textContaining('Starting up, '), findsOneWidget);
    expect(find.textContaining('MTU'), findsNothing);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('the owner\'s Public switch; a guest\'s line, without the '
      'owner\'s settings', (t) async {
    final app = await model(t);
    app.setFake(true);
    await app.connectFake();
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    const public = 'Public: serve phones without pairing';
    expect(find.text(public), findsOneWidget);
    // The home node second, right under the node.
    final ys = [
      for (final h in ['Node', 'Home node', 'Firmware'])
        t.getTopLeft(find.text(h, skipOffstage: false)).dy,
    ];
    expect(ys, [...ys]..sort());
    // On by default; the owner turns it off.
    final toggle = find.widgetWithText(SwitchListTile, public);
    expect(t.widget<SwitchListTile>(toggle).value, isTrue);
    await t.tap(find.text(public));
    await t.runAsync(() => Future<void>.delayed(Duration.zero));
    await t.pump();
    expect(app.fakeNode!.public, isFalse);
    expect(app.session.nodePublic, isFalse);

    // Another phone's view, on the node public again: connected as a guest.
    app.fakeNode!.public = true;
    await app.session.disconnect();
    await app.session.attach(app.fakeNode!.connectGuest(), guest: true);
    await t.pump();
    expect(find.text(public), findsNothing);
    expect(find.textContaining('Connected as a guest'), findsOneWidget);
    expect(find.text('Update firmware'), findsNothing);
    expect(find.text('Make this node my home'), findsNothing);
    expect(find.text('Node\'s place'), findsNothing);
    await t.pumpWidget(BarkApp(app));
    await t.pump();
    expect(
      find.text(
        'Connected to ID FA4E00000001 as a guest: a public node, not yours',
      ),
      findsOneWidget,
    );

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
