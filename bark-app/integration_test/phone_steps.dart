// The phone test's steps (README.md, "The phone test"), apart from where
// they run: on a phone against a board (`phone_test.dart`), or against the
// tests' fake node (`test/phone_steps_test.dart`). Each step drives the
// app as a person would: through the screen where the screen itself could
// break, the pairing and the send box, and through the app's model where
// a tap only calls it.

import 'dart:async';
import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/app_layer.dart' show seedLen;
import 'package:bark_app/ble_link.dart';
import 'package:bark_app/card.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/decision.dart' show ownGone;
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/messenger.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:bark_app/ui/settings_screen.dart';
import 'package:bark_app/ui/thread_screen.dart';
import 'package:bark_app/ui/update_screen.dart';
import 'package:bark_app/updater.dart';
import 'package:crypto/crypto.dart' show sha256;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// How the test comes by the node's bond.
enum Pairing {
  /// Over the bond the phone holds, or by pairing where it holds none.
  any,

  /// Over the bond the phone holds; a phone without one fails.
  bond,

  /// By pairing anew, the phone's bond forgotten first.
  fresh,
}

/// What the test is told.
class Bench {
  /// The node the phone pairs with, and the one its text goes to: twelve
  /// hex digits each, as `bark-` names them.
  final String node;
  final String to;
  final Pairing pairing;

  /// How long the app waits for a text from the second node. A bench
  /// runner sends it at once, a person by hand may take a while.
  final Duration receiveWait;

  Bench({
    required String node,
    required String to,
    this.pairing = Pairing.any,
    this.receiveWait = const Duration(minutes: 5),
  }) : node = node.toLowerCase(),
       to = to.toLowerCase() {
    for (final id in [this.node, this.to]) {
      if (!RegExp(r'^[0-9a-f]{12}$').hasMatch(id)) {
        throw ArgumentError('$id: a node ID is twelve hex digits');
      }
    }
    if (this.node == this.to) {
      throw ArgumentError('the text goes to another node than the phone\'s');
    }
  }

  String get nodeName => 'bark-$node';
}

enum Outcome { pass, fail, skip }

/// What the steps tell the host that runs them, and ask of it.
abstract class Host {
  void began(int n, String step);
  void ended(int n, String step, Outcome o, String detail);

  /// Android now asks for the code on the node's screen.
  void pairing(String node);

  /// A board is to send `payload` to `dst`, the phone's node, as a phone
  /// writes Send: the message whose ID is `id`.
  void sendToPhone(Address dst, Uint8List payload, int id);

  /// The phone's text `id` to `dst` is the network's now.
  void sent(Address dst, int id);

  void done(int passed, int of);
}

/// The host as the bench reads it, one line each, from the Android log
/// (`bark-fw/scripts/phonetest.py`, which reads these forms).
class PrintedHost implements Host {
  final void Function(String) out;
  const PrintedHost(this.out);

  static const tag = 'bark-test';

  @override
  void began(int n, String step) => out('$tag begin $n $step');

  @override
  void ended(int n, String step, Outcome o, String detail) =>
      out('$tag ${o.name} $n $step: ${detail.replaceAll('\n', ' ')}');

  @override
  void pairing(String node) => out('$tag pairing $node');

  @override
  void sendToPhone(Address dst, Uint8List payload, int id) =>
      out('$tag send dst=$dst payload=${hex(payload)} id=${msgIdHex(id)}');

  @override
  void sent(Address dst, int id) =>
      out('$tag sent dst=$dst id=${msgIdHex(id)}');

  @override
  void done(int passed, int of) => out('$tag done $passed/$of');
}

/// A step that did not pass, and why.
class Failed implements Exception {
  final String why;
  const Failed(this.why);
  @override
  String toString() => why;
}

/// A step that could not run, and why.
class Skipped implements Exception {
  final String why;
  const Skipped(this.why);
  @override
  String toString() => why;
}

/// How often a wait looks again: often beside a person's taps, and few
/// frames on the bench.
const Duration _tick = Duration(milliseconds: 250);

/// Longer than a Material dialog's entrance, 150 ms, so that its buttons
/// take a tap.
const Duration _dialogOpens = Duration(milliseconds: 500);

/// How long a scan looks for the node: three of the person's scans, each
/// ten of its advertising intervals ([seenWait]).
final Duration findWait = seenWait * 3;

/// How long the pairing may take: Android's connect, which gives up after
/// 30 s, and its report of the bond ([pairWait]).
final Duration pairingWait = const Duration(seconds: 30) + pairWait;

/// How long the node may take to take the text. A choice: a ready node
/// takes it within a second; the rest leaves room for turns among its
/// phones, each asked again after 12.64 s (spec "Phone link", Sending).
const Duration takeWait = Duration(minutes: 3);

/// How long the image may take to send. A choice: about a minute where
/// Android grants no short connection interval (README.md, "Updating a
/// node"), and room for links lost and taken up again.
const Duration sendImageWait = Duration(minutes: 10);

/// How long the node may take to come back on its new image. A choice:
/// the swap and boot, about a minute, and the search's rounds of 15 s
/// that find it ([lookRound]), with room to spare.
const Duration rebootWait = Duration(minutes: 4);

/// How long a dropped link may take to come back: the search's rounds
/// that call a paired node out of reach ([ownGone] of [lookRound]).
final Duration _relink = lookRound * ownGone;

/// The second node's person, made on the phone with the app's own code:
/// a fixed identity, its home at the second node, so that its contact is
/// one across runs.
class BenchFriend {
  final Me me;
  final Contacts contacts;
  final Session session;
  final Messenger messenger;

  BenchFriend._(this.me, this.contacts, this.session, this.messenger);

  static const name = 'Bench friend';

  static Future<BenchFriend> at(AppModel app, Address home) async {
    final me = Me(MemoryStore('friend me'), app.sodium, app.now);
    await me.load();
    final seed = sha256.convert('bark phone test'.codeUnits).bytes;
    await me.restore(Uint8List.fromList(seed.sublist(0, seedLen)));
    await me.setName(name);
    await me.setHome(home);
    await me.setNodeAddress(home);
    final contacts = Contacts(MemoryStore('friend contacts'));
    final session = Session(store: MemoryStore('friend queue'), now: app.now);
    return BenchFriend._(
      me,
      contacts,
      session,
      Messenger(app.sodium, me, contacts, session, app.now),
    );
  }

  /// Seals `text` to the person on the phone, by the card their app
  /// shows: the message a board then sends to the phone's node.
  Future<Outgoing> write(PersonCard card, String text) async {
    final c = await contacts.add(card);
    await messenger.sendText(c, text);
    return session.outbox.last;
  }
}

/// Runs the five steps in order, each told to `host`; a step whose
/// earlier one failed is skipped. `image` gives the firmware image to
/// update the node with, or null for none. Whether every step that ran
/// passed.
Future<bool> runSteps(
  WidgetTester t,
  AppModel app,
  Bench bench,
  Host host, {
  required Future<Uint8List?> Function() image,
}) async {
  // As the app runs it from its start (`main.dart`): it links again by
  // itself after the update's reboot. Started later, while a link listens
  // to Bluetooth's state already, it would not hear that it is on.
  app.startSearch();
  final s = _Steps(t, app, bench, host);
  var passed = 0, ran = 0;
  final steps = <(String, Future<String> Function(), int?)>[
    ('find', s.findNode, null),
    ('pair', s.pairNode, 1),
    ('send', s.sendText, 2),
    ('receive', s.receiveText, 2),
    ('update', () => s.updateNode(image), 2),
  ];
  final ok = <int>{};
  for (final (i, (name, run, after)) in steps.indexed) {
    final n = i + 1;
    host.began(n, name);
    if (after != null && !ok.contains(after)) {
      host.ended(n, name, Outcome.skip, 'step $after did not pass');
      continue;
    }
    try {
      final detail = await run();
      ok.add(n);
      passed++;
      ran++;
      host.ended(n, name, Outcome.pass, detail);
    } on Skipped catch (e) {
      host.ended(n, name, Outcome.skip, '$e');
    } catch (e) {
      ran++;
      host.ended(n, name, Outcome.fail, '$e');
    }
  }
  host.done(passed, steps.length);
  return passed == ran;
}

class _Steps {
  final WidgetTester t;
  final AppModel app;
  final Bench bench;
  final Host host;
  _Steps(this.t, this.app, this.bench, this.host);

  FoundNode? _found;
  BenchFriend? _friend;

  /// Pumps until `f` gives a value, at most `within`; then fails with
  /// what `why` says of the state.
  Future<T> _until<T>(
    FutureOr<T?> Function() f,
    Duration within,
    String Function() why,
  ) async {
    for (var waited = Duration.zero; ; waited += _tick) {
      final v = await f();
      if (v != null) return v;
      if (waited >= within) throw Failed(why());
      await t.pump(_tick);
    }
  }

  /// Every text in the dialogs shown, for a reason.
  String _dialogs() => [
    for (final d in find.byType(AlertDialog).evaluate())
      for (final e
          in find
              .descendant(
                of: find.byWidget(d.widget),
                matching: find.byType(Text),
              )
              .evaluate())
        (e.widget as Text).data ?? '',
  ].where((s) => s.isNotEmpty).join(' / ');

  /// The last lines of the app's log, for a reason.
  String _lastLog([int n = 3]) {
    final log = app.session.log;
    final from = log.length > n ? log.length - n : 0;
    return log.skip(from).map((l) => l.text).join(' / ');
  }

  Future<void> _show(Widget w) async {
    await t.pumpWidget(MaterialApp(home: w));
    await t.pump();
  }

  Future<String> findNode() async {
    await _show(SettingsScreen(app));
    await t.tap(find.widgetWithText(FilledButton, 'Scan'));
    final name = bench.nodeName;
    final n = await _until(
      () => app.found.values.where((f) => f.name == name).firstOrNull,
      findWait,
      () => app.bluetoothAllowed
          ? 'not heard in ${findWait.inSeconds} s; heard '
                '${app.found.values.map((f) => shownName(f.name)).join(', ')}'
          : 'Android does not let the app use Bluetooth',
    );
    _found = n;
    return '${shownName(name)}, ${n.rssi ?? '?'} dBm, '
        '${n.public ? 'public' : 'private'}';
  }

  Future<String> pairNode() async {
    final name = bench.nodeName;
    if (bench.pairing == Pairing.fresh && app.nodes.own.contains(name)) {
      await _letGo(name);
    }
    final ours = app.nodes.own.contains(name);
    if (bench.pairing == Pairing.bond && !ours) {
      throw const Failed('the phone is not paired with the node');
    }
    // The row's buttons are off while the search connects.
    await _until(
      () => app.connecting ? null : true,
      pairingWait,
      () => 'the search still connects: ${_lastLog()}',
    );
    await t.pump();
    final row = find.widgetWithText(ListTile, shownName(name)).last;
    await t.ensureVisible(row);
    await t.pump();
    final pairButton = find.descendant(
      of: row,
      matching: find.widgetWithText(TextButton, 'Pair'),
    );
    if (!ours) host.pairing(name);
    if (pairButton.evaluate().isNotEmpty) {
      // A public node the phone is not paired with: the row would use it
      // as a guest, its Pair pairs.
      await t.tap(pairButton);
      await t.pump(_dialogOpens);
      await t.tap(find.widgetWithText(FilledButton, 'Pair'));
    } else if (!app.connectedTo(_found!.deviceId) || app.session.guest) {
      await t.tap(row);
    }
    var began = false;
    await _until(
      () async {
        // The first connection asks to notify, in the app's words first;
        // Android's own question, after Continue, the test cannot answer.
        final notNow = find.widgetWithText(TextButton, 'Not now');
        if (find.text('Notifications').evaluate().isNotEmpty &&
            notNow.evaluate().isNotEmpty) {
          await t.tap(notNow);
          return null;
        }
        final s = app.session;
        if (s.connected &&
            !s.guest &&
            nodeName(s.node!.id) == name &&
            app.nodes.own.contains(name) &&
            !app.connecting) {
          return true;
        }
        if (app.connecting) began = true;
        if (began && !app.connecting) {
          throw Failed(
            'no link: ${_dialogs().isEmpty ? _lastLog() : _dialogs()}',
          );
        }
        return null;
      },
      pairingWait,
      () => 'no link in ${pairingWait.inSeconds} s: ${_lastLog()}',
    );
    return ours ? 'over the bond the phone held' : 'paired by the code';
  }

  /// The second node's address, from the neighbour table of the phone's
  /// node, as a person reads it off the map; read once.
  Future<Address> _toAddress() async => _to ??= await _readTo();
  Address? _to;

  Future<Address> _readTo() async {
    final to = parseHex(bench.to)!;
    Neighbour? them;
    var tried = Duration.zero;
    const every = Duration(seconds: 5), within = Duration(minutes: 1);
    var heard = 0;
    while (them == null) {
      final table = await app.session.readNeighbours();
      heard = table.list.length;
      them = table.list.where((n) => hex(n.id) == bench.to).firstOrNull;
      if (them != null) break;
      if (tried >= within) {
        throw Failed(
          '${shownName('bark-${bench.to}')} is not among the '
          '$heard neighbours of ${shownName(bench.nodeName)}',
        );
      }
      await t.pump(every);
      tried += every;
    }
    if (them.position == const Position(0, 0)) {
      throw Failed(
        '${shownName('bark-${bench.to}')} has no place: give it a map point',
      );
    }
    return Address(them.position, to);
  }

  /// The second node's person, whose home it is.
  Future<BenchFriend> _benchFriend() async =>
      _friend ??= await BenchFriend.at(app, await _toAddress());

  /// Gives up the phone's bond with `name`, as the person does: Unpair
  /// on an owner's link, which the node forgets too, else Forget.
  Future<void> _letGo(String name) async {
    final s = app.session;
    final bool? gone;
    if (s.connected && !s.guest && nodeName(s.node!.id) == name) {
      gone = await app.unpair();
    } else {
      gone = await app.forget(name);
    }
    if (gone != true) {
      throw const Failed(
        'Android kept its bond: forget the node in Bluetooth settings',
      );
    }
  }

  Future<String> sendText() async {
    await _here();
    final friend = await _benchFriend();
    final c = await app.contacts.add(friend.me.card);
    await _show(ThreadScreen(app, c.key));
    final text = 'Bench text ${app.now().toUtc().toIso8601String()}';
    await t.enterText(find.byType(TextField), text);
    await t.tap(find.byTooltip('Send'));
    final o = await _until(
      () => app.session.outbox
          .where((o) => o.to == c.key && o.body == text)
          .firstOrNull,
      const Duration(seconds: 5),
      () => 'the send box queued nothing: ${_lastLog()}',
    );
    await _until(
      () => switch (o.state) {
        OutState.taken => true,
        OutState.refused => throw Failed('refused: ${o.error}'),
        _ => null,
      },
      takeWait,
      () => 'not taken in ${takeWait.inMinutes} min, ${o.state.name}',
    );
    host.sent(o.dst, o.id);
    return 'taken by ${shownName(bench.nodeName)}, ${msgIdHex(o.id)} to '
        '${shownName('bark-${bench.to}')}';
  }

  /// The phone is on its node as its owner, linked again by the search
  /// if the link dropped, and the node has a place, which the person's
  /// card names.
  Future<void> _here() async {
    final s = app.session;
    await _until(
      () => s.connected && !s.guest && nodeName(s.node!.id) == bench.nodeName
          ? true
          : null,
      _relink,
      () => 'not linked to ${shownName(bench.nodeName)}: ${_lastLog()}',
    );
    await _until(
      () {
        final a = s.address;
        return a != null && a.toString() == app.me.nodeAddress?.toString()
            ? true
            : null;
      },
      const Duration(seconds: 10),
      () =>
          '${shownName(bench.nodeName)} has no place yet: set one under '
          'This node',
    );
  }

  Future<String> receiveText() async {
    await _here();
    final friend = await _benchFriend();
    final text = 'Bench reply ${app.now().toUtc().toIso8601String()}';
    final o = await friend.write(app.me.card, text);
    if (hex(o.dst.id) != bench.node) {
      throw Failed(
        'the phone\'s card leads to ${nodeLabel(o.dst.id)}, not '
        '${shownName(bench.nodeName)}',
      );
    }
    host.sendToPhone(o.dst, o.payload, o.id);
    final r = await _until(
      () => app.session.received
          .where((r) => r.from == friend.me.key && r.body == text)
          .firstOrNull,
      bench.receiveWait,
      () =>
          'nothing from ${BenchFriend.name} in '
          '${bench.receiveWait.inMinutes} min; the node counts '
          '${app.session.status?.inbox ?? '?'} in the inbox',
    );
    return '${msgIdHex(r.id)} read and opened';
  }

  Future<String> updateNode(Future<Uint8List?> Function() file) async {
    final bytes = await file();
    if (bytes == null) throw const Skipped('no image given');
    final FirmwareImage img;
    try {
      img = parseImage(bytes);
    } catch (e) {
      throw Failed('not an image: $e');
    }
    await _here();
    final u = app.updater;
    u.choose(img, 'the bench\'s file');
    final before = app.session.node?.image;
    if (!u.newer) {
      throw Failed('version ${img.version} is not above the node\'s $before');
    }
    await _show(UpdateScreen(app));
    await t.tap(find.widgetWithText(FilledButton, 'Send to the node'));
    await _until(
      () => switch (u.phase) {
        Phase.committed => true,
        Phase.failed || Phase.staged => throw Failed('${u.message}'),
        _ => null,
      },
      sendImageWait,
      () =>
          'not committed in ${sendImageWait.inMinutes} min: '
          '${u.phase.name}, ${u.held} of ${img.length} B',
    );
    await t.pump();
    await t.tap(
      find.widgetWithText(FilledButton, 'Reboot into version ${img.version}'),
    );
    await _until(
      () => switch (u.phase) {
        Phase.done => true,
        Phase.failed => throw Failed('${u.message}'),
        _ => null,
      },
      rebootWait,
      () =>
          'not back in ${rebootWait.inMinutes} min: ${u.phase.name}, '
          '${_lastLog()}',
    );
    return '${shownName(bench.nodeName)} runs version ${img.version}, '
        'from $before';
  }
}
