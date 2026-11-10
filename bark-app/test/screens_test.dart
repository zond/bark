// The screens against the fake node: they build; a text goes to the fake
// friend and its echo comes back into the same thread, sealed both ways;
// the person reacts to the echo by a long press, from the first row, the
// full list or a search, the first row following their picks, and the
// friend's reaction shows on the sent text's corner;
// the card, and without a name a prompt in its place, the backup words,
// adding a contact from a pasted card or from a scanned one without a
// name, its thread opened at once, and the debug screen; the fetch button, on the first screen and
// in a thread, and why it sends nothing; the home
// buttons, the home known by its node ID wherever it is, and its card
// sent after a move only by "Send my card"; and a node that another
// phone has paired with since, and pairing with it again; and a long
// text, counted in parts as it is written, sent in one bubble and
// received as far as it has come; and a first message marked in a
// verified thread; and the owner's Public switch, and a guest's view;
// and the line for a node that refuses the phone's old bond, with the
// way to Bluetooth settings; and a guest offered its node as the home on
// the first screen, which pairs, and in Settings, beside pairing alone.

import 'package:bark_app/app.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/emojis.dart';
import 'package:bark_app/fake_node.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/main.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/app_layer.dart';
import 'package:bark_app/ble_link.dart' show FoundNode, strongDbm, weakDbm;
import 'package:universal_ble/universal_ble.dart' show UniversalBle;
import 'package:bark_app/ui/common.dart' show receivedMarks, signalWord;
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

import 'alerts_test.dart' show FakeNotifier;
import 'ble_link_test.dart' show FakeNodes, OtherNode;

/// The one check's words: no news from the network past it.
const sentTip = 'Sent into the network, which does not say when it arrives';

void main() {
  test('a found node\'s signal in a word, at the thresholds chosen', () {
    expect(signalWord(-50), 'strong signal');
    expect(signalWord(strongDbm), 'strong signal');
    expect(signalWord(strongDbm - 1), 'fair signal');
    expect(signalWord(weakDbm), 'fair signal');
    expect(signalWord(weakDbm - 1), 'weak signal');
    expect(signalWord(null), 'signal unknown');
  });

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

    // React to the echo: a long press opens the reactions, with a line on
    // what one does; the heart goes, and shows on the echo's corner. The
    // friend's heart for "hello" shows on its corner when it comes.
    // Nothing is pressed for the person, and no mark waits for one.
    Finder inBubble(String text, Finder f) => find.descendant(
      of: find.ancestor(of: find.text(text), matching: find.byType(Card)),
      matching: f,
    );
    List<String> firstRow() => [
      for (final b in t.widgetList<IconButton>(
        find.descendant(
          of: find.byKey(const ValueKey('frequent')),
          matching: find.byType(IconButton),
        ),
      ))
        (b.icon as Text).data!,
    ];
    Future<void> hold() async {
      await t.longPress(find.text('Echo: hello'));
      await t.pumpAndSettle();
    }

    final taken = find.byTooltip(sentTip);
    expect(inBubble('hello', taken), findsOneWidget);
    expect(find.textContaining('Got it'), findsNothing);
    expect(find.byType(ActionChip), findsNothing);
    await hold();
    expect(find.textContaining('sees it on their text'), findsOneWidget);
    expect(find.text('Copy text'), findsOneWidget);
    expect(firstRow(), quickReactions);
    expect(
      t.getSize(find.widgetWithText(IconButton, '❤️').first).height,
      greaterThanOrEqualTo(48),
    );
    await t.tap(find.byTooltip('red heart').first);
    await t.pumpAndSettle();
    expect(find.byTooltip('You reacted: red heart'), findsOneWidget);
    final reacted = app.session.outbox.last;
    expect(reacted.kind, OutKind.reaction);
    expect(reacted.body, '❤️');
    // The chip sits half over the bubble's lower edge, on the corner
    // toward the middle: the right of a received text, with room below.
    Rect chipOf(String text) => t.getRect(
      find.descendant(
        of: find.ancestor(of: find.text(text), matching: find.byType(Stack)),
        matching: find.byKey(const ValueKey('reaction')),
      ),
    );
    Rect cardOf(String text) => t.getRect(
      find.ancestor(of: find.text(text), matching: find.byType(Card)),
    );
    Rect stackOf(String text) => t.getRect(
      find.ancestor(of: find.text(text), matching: find.byType(Stack)).first,
    );
    void onEdge(String text, {required bool right}) {
      final chip = chipOf(text);
      // Material's card margin: the visible edge is 4 inside the box.
      final edge = cardOf(text).deflate(4);
      expect(chip.top, lessThan(edge.bottom));
      expect(chip.bottom, greaterThan(edge.bottom));
      expect((chip.center.dy - edge.bottom).abs(), lessThan(1));
      expect(chip.center.dx > edge.center.dx, right);
      expect(chip.left, greaterThanOrEqualTo(edge.left));
      expect(chip.right, lessThanOrEqualTo(edge.right));
      // The bubble's box holds the chip, so the next bubble starts below.
      expect(chip.bottom, lessThanOrEqualTo(stackOf(text).bottom));
    }

    onEdge('Echo: hello', right: true);

    // A second press replaces it, and goes again; the heart now leads the
    // first row. A pick from the full list, and one found by its name.
    await hold();
    expect(firstRow().first, '❤️');
    expect(find.text('Smileys & emotion'), findsOneWidget);
    await t.tap(find.byTooltip('grinning face').last);
    await t.pumpAndSettle();
    expect(find.byTooltip('You reacted: red heart'), findsNothing);
    expect(find.byTooltip('You reacted: grinning face'), findsOneWidget);
    expect(app.session.outbox.last.body, '😀');
    await hold();
    final search = find.byKey(const ValueKey('emoji search'));
    await t.enterText(search, 'hedge');
    await t.pumpAndSettle();
    expect(find.byTooltip('hedgehog'), findsOneWidget);
    // Found, not in the groups.
    expect(find.text('Smileys & emotion'), findsNothing);
    await t.enterText(search, 'zzzq');
    await t.pumpAndSettle();
    expect(find.text('No emoji by that name.'), findsOneWidget);
    await t.enterText(search, 'hedgehog');
    await t.pumpAndSettle();
    await t.tap(find.byTooltip('hedgehog'));
    await t.pumpAndSettle();
    expect(find.byTooltip('You reacted: hedgehog'), findsOneWidget);
    expect(app.session.outbox.last.body, '🦔');
    expect(
      app.session.outbox.where((o) => o.kind == OutKind.reaction).length,
      3,
    );
    await hold();
    expect(firstRow().take(3), ['🦔', '😀', '❤️']);
    Navigator.pop(t.element(find.byType(BottomSheet)));
    await t.pumpAndSettle();

    final hello = app.session.outbox.firstWhere((o) => o.body == 'hello');
    final f = app.fakeNode!.friend;
    final heart = writePlain(
      Header(greater(f.identity, app.me.identity), hello.sent + 60),
      [ReactionRecord(hello.sent, '❤️')],
    );
    final pair = sodium.crypto.box.precalculate(
      publicKey: app.me.identity,
      secretKey: f.pair.secretKey,
    );
    await t.runAsync(
      () => app.messenger.receive(
        InboxMessage(
          4242,
          sealBox(pair, 0, sodium.randombytes.buf(nonceLen), heart),
        ),
      ),
    );
    await t.pump();
    expect(find.byTooltip('They reacted: red heart'), findsOneWidget);
    // On a sent text, the left corner.
    onEdge('hello', right: false);
    expect(inBubble('hello', taken), findsOneWidget);
    // A tap on a state explains them all, the reaction too.
    await t.tap(inBubble('hello', taken));
    await t.pumpAndSettle();
    expect(find.text('Their reaction'), findsOneWidget);
    expect(find.text('Confirmed by the recipient'), findsNothing);
    expect(
      find.textContaining('The network gives no further news'),
      findsOneWidget,
    );
    // Above them, what the bubble does not say: the node it went to.
    expect(find.text('This message'), findsOneWidget);
    expect(find.text('To node'), findsOneWidget);
    expect(find.text('Message ID'), findsOneWidget);
    expect(find.text('Sealed'), findsOneWidget);
    Navigator.pop(t.element(find.byType(BottomSheet)));
    await t.pumpAndSettle();
    // A tap anywhere on a bubble, its text too, opens the same help:
    // a sent one's, then a received one's.
    await t.tap(find.text('hello').first);
    await t.pumpAndSettle();
    expect(find.text('To node'), findsOneWidget);
    Navigator.pop(t.element(find.byType(BottomSheet)));
    await t.pumpAndSettle();
    await t.tap(find.text('Echo: hello').first);
    await t.pumpAndSettle();
    expect(find.text('This message'), findsOneWidget);
    expect(find.text('Written'), findsOneWidget);
    expect(find.text('Sealed'), findsOneWidget);
    for (final (_, title, words) in receivedMarks) {
      expect(
        find.descendant(
          of: find.byType(BottomSheet),
          matching: find.text(title, skipOffstage: false),
        ),
        findsOneWidget,
      );
      expect(words, isNotEmpty);
    }
    Navigator.pop(t.element(find.byType(BottomSheet)));
    await t.pumpAndSettle();
    await t.pageBack();
    await t.pumpAndSettle();

    await t.runAsync(() => app.me.setName('Ana'));
    await t.tap(find.byTooltip('My card'));
    await t.pumpAndSettle();
    expect(find.byType(QrImageView), findsOneWidget);
    // No home: a temporary card, marked so on the code.
    expect(find.byType(TemporaryFrame), findsOneWidget);
    expect(find.text('TEMPORARY'), findsOneWidget);
    // The code first, then Share and the rest; the hex in Details.
    expect(t.getTopLeft(find.byType(QrImageView)).dy, lessThan(120));
    await t.scrollUntilVisible(
      find.text('Share'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.textContaining('Temporary card:'), findsOneWidget);
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
    // The bench's firmware file is picked here, not in Settings.
    await t.scrollUntilVisible(
      find.text('Pick a firmware file'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Bench'), findsOneWidget);

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
    // The one action, the same under Home node and under Node's place.
    expect(find.text('Use this phone\'s location'), findsNWidgets(2));
    expect(find.text('Phone location'), findsNothing);
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

  testWidgets('a temporary contact: marked with its age until a card with a '
      'home; written to at its node, fresh without asking, old after saying '
      'so', (t) async {
    final app = await model(t);
    final home = Address(Position.fromDegrees(50, 30), [1, 2, 3, 4, 5, 6]);
    final node = Address(Position.fromDegrees(50.1, 30.1), [6, 5, 4, 3, 2, 1]);
    final id = List.filled(32, 5);
    PersonCard card(Address? h, int time, Duration linked) => PersonCard(
      id,
      h,
      time,
      'Olena',
      now: node,
      nowAt: sentOf(DateTime.now().subtract(linked)),
    );
    await t.runAsync(
      () => app.contacts.add(card(null, 1, const Duration(minutes: 20))),
    );
    await t.pumpWidget(MaterialApp(home: ContactsScreen(app)));
    expect(
      find.text('temporary: at the node they used 20 min ago'),
      findsOneWidget,
    );
    expect(find.byIcon(Icons.hourglass_empty), findsOneWidget);

    // Old: the dialog says so, and its one choice sends there.
    await t.runAsync(() => app.me.setNodeAddress(home));
    await t.pumpWidget(MaterialApp(home: ThreadScreen(app, hex(id))));
    await t.enterText(find.byType(TextField), 'hi');
    await t.tap(find.byTooltip('Send'));
    await t.pumpAndSettle();
    expect(
      find.text(
        'Last seen there 20 min ago: it keeps their mail only while they '
        'come back',
      ),
      findsOneWidget,
    );
    expect(find.text('Their home node'), findsNothing);
    await t.tap(find.text('Where Olena was 20 min ago'));
    await t.pumpAndSettle();
    final out = app.session.outbox.single;
    expect(out.dst.toString(), node.toString());
    expect(out.toward, Toward.there);

    // Fresh: there without asking.
    await t.runAsync(
      () => app.contacts.add(card(null, 1, const Duration(seconds: 10))),
    );
    await t.pump();
    await t.enterText(find.byType(TextField), 'again');
    await t.tap(find.byTooltip('Send'));
    await t.pumpAndSettle();
    expect(find.text('Send to'), findsNothing);
    expect(app.session.outbox.last.body, 'again');
    expect(app.session.outbox.last.dst.toString(), node.toString());

    await t.runAsync(
      () => app.contacts.add(card(home, 2, const Duration(seconds: 10))),
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
      expect(find.text('Their home node'), findsOneWidget);
      expect(find.text('Fast if they are still near there'), findsOneWidget);
      expect(
        find.text('Waits at their home until they fetch it'),
        findsOneWidget,
      );
      // With a home there is a choice, so no warning.
      expect(find.textContaining('Last seen there'), findsNothing);
      await t.tap(find.text(choice));
      await t.pumpAndSettle();
    }

    await send('to the mailbox', 'Their home node');
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
    expect(find.byTooltip('Waiting for a node'), findsOneWidget);

    Future<void> redraw() async {
      app.session.dismissNotice();
      await t.pump();
    }

    // They differ: in words.
    out[0].state = OutState.taken;
    await redraw();
    expect(find.text('1 of 2 sent'), findsOneWidget);
    expect(find.byTooltip('Waiting for a node'), findsNothing);
    out[1].state = OutState.taken;
    await redraw();
    expect(find.byTooltip(sentTip), findsOneWidget);
    expect(find.textContaining('of 2'), findsNothing);

    // Received: the parts that came, a line where one has not, and the
    // person's reaction under them, which a part that comes later keeps.
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
    for (final r in app.session.received) {
      r.reaction = '🙏';
    }
    await redraw();
    expect(find.byTooltip('You reacted: folded hands'), findsOneWidget);
    await t.runAsync(() => receive(7001, 2, 'two '));
    await t.pump();
    expect(find.textContaining('not yet here'), findsNothing);
    expect(find.text('one two three'), findsOneWidget);
    expect(find.byTooltip('You reacted: folded hands'), findsOneWidget);

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
    expect(find.text('Not connected. Texts wait on the phone'), findsOneWidget);
    await app.connectFake();
    await t.pump();
    expect(find.text('Connected to ID FA4E00000001, paired'), findsOneWidget);
    await app.session.disconnect();
    await t.pump();
    expect(find.text('Not connected. Texts wait on the phone'), findsOneWidget);

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
    // Settings lists it among the paired nodes, lost.
    expect(find.text('ID FA4E00000001'), findsOneWidget);
    expect(
      find.text('No longer yours: it no longer knows this phone'),
      findsOneWidget,
    );

    // Its row offers to pair again, beside Forget, and a tap on it says
    // what happened and what each does.
    final again = find.widgetWithText(TextButton, 'Pair again');
    expect(again, findsOneWidget);
    expect(find.widgetWithText(TextButton, 'Forget'), findsOneWidget);
    await t.tap(find.text('ID FA4E00000001'));
    await t.pumpAndSettle();
    expect(
      find.text('ID FA4E00000001 no longer knows this phone'),
      findsOneWidget,
    );
    expect(find.textContaining('Pair again makes it yours again'), findsOne);
    expect(find.textContaining('Forget gives the node up'), findsOne);
    await t.tapAt(const Offset(10, 10));
    await t.pumpAndSettle();

    // Or pair again from the dialog, at the node.
    await t.tap(find.text('Connect'));
    await t.pumpAndSettle();
    await t.tap(find.widgetWithText(FilledButton, 'Pair again'));
    await t.pumpAndSettle();
    expect(app.session.connected, isTrue);
    expect(app.session.pairingLost, isEmpty);
    expect(again, findsNothing);

    // Lost once more, and paired again from its row, in one press.
    app.fakeNode!.pairElsewhere();
    await app.session.disconnect();
    await t.pump();
    await t.tap(find.text('Connect'));
    await t.pumpAndSettle();
    await t.tap(find.text('Not now'));
    await t.pumpAndSettle();
    expect(app.session.pairingLost, isNotEmpty);
    await t.tap(again);
    await t.pumpAndSettle();
    expect(app.session.connected, isTrue);
    expect(app.session.pairingLost, isEmpty);
    expect(find.text('Connected to ID FA4E00000001, paired'), findsOneWidget);
    expect(find.text('Connected'), findsOneWidget);
    // The node's health in words, not the link's MTU: the fake is in its
    // first rotation.
    expect(find.textContaining('Starting up, '), findsOneWidget);
    expect(find.textContaining('MTU'), findsNothing);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('Settings lists the paired nodes, the home first, each with '
      'how it stands, and Unpair on the one connected; the line\'s help '
      'says why', (t) async {
    final app = await model(t);
    app.setFake(true);
    await app.connectFake();
    await app.me.setHome(Address(Position.fromDegrees(50, 30), FakeNode.id));
    // Paired with another since, which another phone has paired with.
    await app.session.setPaired('bark-0ce400000002', lost: true);
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    expect(
      find.text('Connected to your home, ID FA4E00000001'),
      findsOneWidget,
    );
    expect(find.text('Paired with'), findsOneWidget);
    final home = find.text('ID FA4E00000001');
    final other = find.text('ID 0CE400000002');
    expect(t.getTopLeft(home).dy, lessThan(t.getTopLeft(other).dy));
    expect(find.text('Your home, connected'), findsOneWidget);
    expect(
      find.text('No longer yours: it no longer knows this phone'),
      findsOneWidget,
    );
    expect(find.widgetWithText(TextButton, 'Unpair'), findsOneWidget);
    await t.tap(find.byTooltip('Why this node'));
    await t.pumpAndSettle();
    expect(
      find.textContaining('This phone is paired with your home node'),
      findsOneWidget,
    );

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('Forget on a node no longer yours: the dialog says what '
      'follows; not heard, the phone\'s pairing is left to Bluetooth '
      'settings; the home stays', (t) async {
    final app = await model(t);
    const lost = 'bark-0ce400000002';
    final home = Address(Position.fromDegrees(50, 30), [
      0x0c,
      0xe4,
      0,
      0,
      0,
      2,
    ]);
    await app.me.setHome(home);
    await app.session.setPaired(lost, lost: true);
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    final forget = find.widgetWithText(TextButton, 'Forget');
    await t.ensureVisible(forget);
    await t.tap(forget);
    await t.pumpAndSettle();
    expect(find.text('Forget ID 0CE400000002?'), findsOneWidget);
    expect(
      find.textContaining('It stays your home node, so texts to you still'),
      findsOneWidget,
    );
    await t.tap(find.widgetWithText(FilledButton, 'Forget'));
    await t.pumpAndSettle();
    expect(app.session.pairedWith, isEmpty);
    expect(app.me.home.toString(), home.toString());
    expect(
      find.textContaining(
        'To do it now, forget the node in the phone\'s '
        'Bluetooth settings.',
      ),
      findsOneWidget,
    );
    expect(
      find.widgetWithText(FilledButton, 'Bluetooth settings'),
      findsOneWidget,
    );
    await t.tap(find.text('Not now'));
    await t.pumpAndSettle();
    expect(find.text('Paired with'), findsNothing);

    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('the node\'s phones in two lists; another\'s removed by the '
      'code on the node\'s screen, a wrong code offering a new one', (t) async {
    final app = await model(t);
    await app.me.setName('Ann');
    app.setFake(true);
    final f = app.fakeNode = FakeNode(
      now: DateTime.now,
      sodium: sodium,
      app: app.me.identity,
    );
    final today = DateTime.now().toUtc().difference(DateTime.utc(2026)).inDays;
    f.others.addAll([
      PhoneRecord(number: 2, paired: true, home: true, day: today, name: 'Bo'),
      PhoneRecord(number: 3, paired: true, day: today - 2),
      const PhoneRecord(number: 4, home: true, name: 'Cy'),
    ]);
    await app.connectFake();
    await app.session.setPaired(nodeName(FakeNode.id));
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    // The name from the card went to the node.
    expect(f.ownerName, 'Ann');
    expect(find.text('Phones paired with this node'), findsOneWidget);
    expect(find.text('Phones with a home here'), findsOneWidget);
    // On the node's list and under My name.
    expect(find.text('Ann'), findsNWidgets(2));
    expect(find.text('This phone, linked now'), findsOneWidget);
    expect(
      find.text('Has a home here too, last linked yesterday'),
      findsOneWidget,
    );
    expect(find.text('Paired too, last linked yesterday'), findsOneWidget);
    expect(find.text('A phone with no name'), findsOneWidget);
    expect(find.text('Last linked 3 days ago'), findsOneWidget);
    expect(find.text('Last linked never'), findsOneWidget);
    expect(find.widgetWithText(TextButton, 'Remove'), findsNWidgets(4));
    expect(find.widgetWithText(TextButton, 'Unpair'), findsOneWidget);

    // A wrong code: not removed, and a new code asked for.
    final remove = find.widgetWithText(TextButton, 'Remove').at(1);
    await t.ensureVisible(remove);
    await t.tap(remove);
    await t.pumpAndSettle();
    expect(find.text('Remove the phone with no name?'), findsOneWidget);
    expect(
      find.textContaining('no longer paired with the node'),
      findsOneWidget,
    );
    expect(find.textContaining('within 30 s'), findsOneWidget);
    await t.tap(find.text('Show the code'));
    await t.pumpAndSettle();
    final first = f.shownCode!;
    final wrong = (first + 1) % removalCodes;
    await t.enterText(
      find.byKey(const Key('removal code')),
      wrong.toString().padLeft(6, '0'),
    );
    await t.pump();
    await t.tap(find.widgetWithText(FilledButton, 'Remove'));
    await t.pumpAndSettle();
    expect(find.text('Not removed'), findsOneWidget);
    expect(f.others.length, 3);
    await t.tap(find.text('New code'));
    await t.pumpAndSettle();
    expect(f.shownCode, isNotNull);
    await t.enterText(
      find.byKey(const Key('removal code')),
      f.shownCode!.toString().padLeft(6, '0'),
    );
    await t.pump();
    await t.tap(find.widgetWithText(FilledButton, 'Remove'));
    await t.pumpAndSettle();
    expect(find.text('Removed'), findsOneWidget);
    expect(f.others.map((r) => r.number), [2, 4]);
    expect(find.text('A phone with no name'), findsNothing);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('a full node: Settings says it takes no more pairings', (
    t,
  ) async {
    final app = await model(t);
    app.setFake(true);
    final f = app.fakeNode = FakeNode(
      now: DateTime.now,
      sodium: sodium,
      app: app.me.identity,
    );
    f.others.addAll([
      for (var n = 2; n <= maxPhones; n++) PhoneRecord(number: n, paired: true),
    ]);
    await app.connectFake();
    expect(app.session.node!.room, 0);
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    expect(
      find.textContaining('takes no more pairings until one is removed'),
      findsOneWidget,
    );
    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  testWidgets('the paired list: stopped, a row says so and a tap connects; '
      'Disconnect below it; a newer node said quietly', (t) async {
    final app = await model(t);
    app.setFake(true);
    app.fakeNode = FakeNode(
      now: DateTime.now,
      sodium: sodium,
      app: app.me.identity,
    )..version = linkVersion + 1;
    await app.connectFake();
    await app.session.setPaired('bark-0ce400000002');
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    expect(app.session.notice, isNull);
    final disconnect = find.widgetWithText(OutlinedButton, 'Disconnect');
    expect(
      t.getTopLeft(disconnect).dy,
      greaterThan(t.getTopLeft(find.text('ID 0CE400000002')).dy),
    );
    await t.scrollUntilVisible(
      find.textContaining('newer than this app'),
      100,
      scrollable: find.byType(Scrollable).first,
    );
    expect(
      find.textContaining('brings the node\'s new features'),
      findsOneWidget,
    );
    await t.scrollUntilVisible(
      disconnect,
      -100,
      scrollable: find.byType(Scrollable).first,
    );
    await t.tap(disconnect);
    await t.pumpAndSettle();
    expect(app.session.stopped, isTrue);
    expect(find.text('Not looking. Tap to connect'), findsNWidgets(2));
    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('the owner unpairs, after the dialog says what follows', (
    t,
  ) async {
    final app = await model(t);
    app.setFake(true);
    await app.connectFake();
    await app.session.setPaired(nodeName(FakeNode.id));
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    final unpair = find.widgetWithText(TextButton, 'Unpair');
    await t.ensureVisible(unpair);
    await t.tap(unpair);
    await t.pumpAndSettle();
    expect(find.text('Unpair ID FA4E00000001?'), findsOneWidget);
    expect(find.textContaining('forgets this phone'), findsOneWidget);
    expect(find.textContaining('UNPAIRED'), findsOneWidget);
    expect(find.textContaining('public again'), findsOneWidget);
    expect(find.textContaining('code on its screen'), findsOneWidget);
    expect(find.textContaining('texts and contacts stay'), findsOneWidget);
    expect(find.textContaining('forgets its own pairing'), findsOneWidget);
    // Cancel changes nothing.
    await t.tap(find.text('Cancel'));
    await t.pumpAndSettle();
    expect(app.fakeNode!.paired, isTrue);
    await t.tap(unpair);
    await t.pumpAndSettle();
    await t.tap(find.widgetWithText(FilledButton, 'Unpair'));
    await t.runAsync(() => Future<void>.delayed(Duration.zero));
    await t.pumpAndSettle();
    // The node forgot the bond and ended the link; the app forgot it.
    expect(app.fakeNode!.paired, isFalse);
    expect(app.fakeNode!.public, isTrue);
    expect(app.session.connected, isFalse);
    expect(app.session.pairedWith, isEmpty);
    expect(app.pairedNodes, isEmpty);
    expect(unpair, findsNothing);
    await t.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('a node that refuses the phone\'s old bond: one line on the '
      'first screen and in Settings, with the way to Bluetooth settings', (
    t,
  ) async {
    final notifier = FakeNotifier();
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, DateTime.now),
      session: Session(store: MemoryStore(), now: DateTime.now),
      contacts: Contacts(MemoryStore()),
      now: DateTime.now,
      notifier: notifier,
    );
    await t.runAsync(app.load);
    const line =
        'ID 0CE400000004 no longer knows this phone\'s old pairing. Forget '
        'it in the phone\'s Bluetooth settings, then try again.';
    await t.pumpWidget(BarkApp(app));
    expect(find.text(line), findsNothing);
    app.staleBonds['bark-0ce400000004'] = 'aa:bb:cc:dd:ee:04';
    app.notifyListeners();
    await t.pump();
    expect(find.text(line), findsOneWidget);
    await t.tap(find.text('Bluetooth settings'));
    await t.pump();
    expect(notifier.log, contains('Bluetooth settings'));
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    expect(find.text(line), findsOneWidget);
    await t.pumpWidget(const SizedBox());
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
      for (final h in ['Node', 'Home node', 'Updates'])
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
    const guestLine = 'Connected to ID FA4E00000001 as a guest';
    expect(find.text(guestLine), findsOneWidget);
    expect(find.text('Send to the node'), findsNothing);
    expect(find.text('Pick a firmware file'), findsNothing);
    expect(find.text('Make this node my home'), findsNothing);
    expect(find.text('Node\'s place'), findsNothing);
    await t.pumpWidget(BarkApp(app));
    await t.pump();
    expect(find.text(guestLine), findsOneWidget);

    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    await t.pump();
    app.dispose();
  });

  /// The app used as a guest at a public node nobody has paired with:
  /// the search started, so the errand's node is the one the phone uses,
  /// but Bluetooth not reported on, so no round runs of itself.
  Future<(AppModel, FakeNodes)> guestOnce(WidgetTester t) async {
    final app = await model(t);
    const device = 'aa:bb:cc:dd:ee:04';
    final ble = FakeNodes({
      device: OtherNode(
        now: DateTime.now,
        sodium: sodium,
        app: app.me.identity,
        id: [0x0c, 0xe4, 0, 0, 0, 4],
      )..owned = false,
    }, bonds: {});
    UniversalBle.setInstance(ble);
    app.startSearch();
    await t.pump();
    app.connectGuest(FoundNode(device, 'bark-0ce400000004', -50, public: true));
    for (var i = 0; i < 10; i++) {
      await t.pump(const Duration(milliseconds: 100));
    }
    return (app, ble);
  }

  Future<void> done(WidgetTester t, AppModel app, FakeNodes ble) async {
    await t.pumpWidget(const SizedBox());
    await app.stopLooking();
    app.session.disconnect();
    await t.pump(const Duration(seconds: 1));
    app.dispose();
    for (final n in ble.nodes.values) {
      n.dispose();
    }
    await t.pump(const Duration(minutes: 1));
  }

  testWidgets('a guest with no home: the first screen offers the node as '
      'the home, which pairs and then makes it the home', (t) async {
    final (app, ble) = await guestOnce(t);
    expect(app.guestNode, 'bark-0ce400000004');
    await t.pumpWidget(BarkApp(app));
    await t.pump();
    expect(
      find.textContaining(
        'This phone has no home node set. As your home, this node keeps '
        'texts to you and gives your card a fixed address. The phone pairs '
        'with it first: you type the code its screen shows.',
      ),
      findsOneWidget,
    );
    await t.tap(find.text('Make this node my home'));
    for (var i = 0; i < 10; i++) {
      await t.pump(const Duration(milliseconds: 100));
    }
    expect(ble.pairings, 1);
    expect((app.session.connected, app.session.guest), (true, false));
    expect(app.me.home?.id, [0x0c, 0xe4, 0, 0, 0, 4]);
    expect(find.text('This node is your home'), findsOneWidget);
    expect(find.textContaining('no home node set'), findsNothing);
    await done(t, app, ble);
  });

  testWidgets('a guest\'s Not now hides the offer for the app\'s run', (
    t,
  ) async {
    final (app, ble) = await guestOnce(t);
    await t.pumpWidget(BarkApp(app));
    await t.pump();
    await t.tap(find.text('Not now'));
    await t.pump();
    expect(find.textContaining('no home node set'), findsNothing);
    expect(app.guestNode, 'bark-0ce400000004');
    expect(ble.pairings, 0);
    await done(t, app, ble);
  });

  testWidgets('a guest\'s Settings: Make this node my home, saying it pairs '
      'first; Pair on the node\'s row in the scan pairs at once', (t) async {
    final (app, ble) = await guestOnce(t);
    const device = 'aa:bb:cc:dd:ee:04';
    app.found[device] = FoundNode(
      device,
      'bark-0ce400000004',
      -50,
      public: true,
    );
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    const home = 'Make this node my home';
    expect(find.text(home, skipOffstage: false), findsOneWidget);
    expect(find.text('Pair with this node', skipOffstage: false), findsNothing);
    // Neither the owner's switch nor its unpairing for a guest.
    expect(find.text('Public: serve phones without pairing'), findsNothing);
    expect(find.widgetWithText(TextButton, 'Unpair'), findsNothing);
    await t.ensureVisible(find.text(home));
    await t.tap(find.text(home));
    await t.pumpAndSettle();
    Finder said(String s) => find.descendant(
      of: find.byType(AlertDialog),
      matching: find.textContaining(s),
    );
    expect(
      said(
        'The phone pairs with this node first: you type the six-digit code '
        'its screen shows.',
      ),
      findsOneWidget,
    );
    expect(said('keeps texts to you and gives your card a fixed'), findsOne);
    await t.tap(find.text('Cancel'));
    await t.pumpAndSettle();
    expect(ble.pairings, 0);
    // Pair where the node is listed: what it gives, and that the home is
    // set apart; then the code, with no errand first.
    final pair = find.widgetWithText(TextButton, 'Pair');
    await t.ensureVisible(pair);
    await t.tap(pair);
    await t.pumpAndSettle();
    expect(said('up to eight phones'), findsOneWidget);
    expect(said('Pairing does not make it your home'), findsOneWidget);
    await t.tap(find.widgetWithText(FilledButton, 'Pair'));
    for (var i = 0; i < 10; i++) {
      await t.pump(const Duration(milliseconds: 100));
    }
    expect(ble.pairings, 1);
    expect((app.session.connected, app.session.guest), (true, false));
    expect(app.pairedNodes, contains('bark-0ce400000004'));
    expect(app.me.home, isNull);
    await done(t, app, ble);
  });

  testWidgets('Pair that ends without a pairing says why in a dialog: the '
      'node ended the link; Android kept the old bond', (t) async {
    final (app, ble) = await guestOnce(t);
    const device = 'aa:bb:cc:dd:ee:04';
    app.found[device] = FoundNode(
      device,
      'bark-0ce400000004',
      -50,
      public: true,
    );
    await t.pumpWidget(MaterialApp(home: SettingsScreen(app)));
    await t.pump();
    Future<void> pair() async {
      final pair = find.widgetWithText(TextButton, 'Pair');
      await t.ensureVisible(pair);
      await t.tap(pair);
      await t.pumpAndSettle();
      await t.tap(find.widgetWithText(FilledButton, 'Pair'));
      // Past the wait for Android to remove a bond.
      for (var i = 0; i < 30; i++) {
        await t.pump(const Duration(milliseconds: 100));
      }
    }

    ble.endsLinks.add(device);
    await pair();
    expect(find.text('Not paired'), findsOneWidget);
    expect(
      find.textContaining(
        'The link to ID 0CE400000004 ended before the node '
        'was read: the node ended it, or is out of reach.',
      ),
      findsOneWidget,
    );
    expect(ble.pairings, 0);
    await t.tap(find.text('OK'));
    await t.pumpAndSettle();

    // Android holds a bond with it, which the node no longer knows, and
    // will not let it go.
    await app.stopLooking();
    ble
      ..endsLinks.clear()
      ..bonds.add(device)
      ..keepsBonds = true
      ..nodes[device]!.paired = false;
    await pair();
    expect(
      find.textContaining(
        'could not forget its old pairing with ID '
        '0CE400000004',
      ),
      findsOneWidget,
    );
    expect(
      find.widgetWithText(FilledButton, 'Bluetooth settings'),
      findsOneWidget,
    );
    expect(ble.pairings, 0);
    await t.tap(find.text('Not now'));
    await t.pumpAndSettle();
    await done(t, app, ble);
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
