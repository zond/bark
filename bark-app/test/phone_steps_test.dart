// The phone test's steps (`integration_test/phone_steps.dart`) against
// the fake node, as the bench runs them against a board: found, paired by
// its Pair, a text to a neighbour taken, one from the bench friend read
// and opened, an image sent and rebooted into; over a bond the phone
// holds, and paired anew in its place; a phone without one when a bond is
// asked for, the later steps skipped; and the lines the host reads.

import 'dart:typed_data';

import 'package:bark_app/app.dart';
import 'package:bark_app/contacts.dart';
import 'package:bark_app/link.dart';
import 'package:bark_app/me.dart';
import 'package:bark_app/session.dart';
import 'package:bark_app/store.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sodium/sodium.dart';
import 'package:universal_ble/universal_ble.dart' show UniversalBle;

import '../integration_test/phone_steps.dart';
import 'fake_ble.dart';
import 'fake_node.dart';
import 'update_test.dart' show imageFile;

/// The host's lines as the bench reads them; a board sends what the
/// steps ask for at once, as the fake network delivers it.
class _Host extends PrintedHost {
  _Host(List<String> lines, this.node) : super(lines.add);
  final FakeNode node;

  @override
  void sendToPhone(Address dst, Uint8List payload, int id) {
    super.sendToPhone(dst, payload, id);
    node.deliver(payload);
  }
}

void main() {
  late Sodium sodium;
  setUpAll(() async => sodium = await SodiumInit.init());

  /// Runs the steps against a fake node that the phone has `paired` with
  /// or not; the lines the host was told.
  Future<(bool, List<String>)> run(
    WidgetTester t, {
    required bool paired,
    Pairing pairing = Pairing.any,
    Uint8List? image,
  }) async {
    DateTime now() => t.binding.clock.now();
    final app = AppModel(
      sodium: sodium,
      me: Me(MemoryStore(), sodium, now),
      session: Session(store: MemoryStore(), now: now),
      contacts: Contacts(MemoryStore()),
      now: now,
    );
    await t.runAsync(app.load);
    final node = FakeNode(
      now: now,
      sodium: sodium,
      app: app.me.identity,
      mailbox: app.me.keys.mailbox,
    )..paired = paired;
    final ble = FakeNodes({fakeDevice: node}, bonds: {if (paired) fakeDevice});
    UniversalBle.setInstance(ble);
    if (paired) await app.session.setPaired(nodeName(FakeNode.id));
    final lines = <String>[];
    final to = hex(node.neighbours.values.first.id);
    final ok = await runSteps(
      t,
      app,
      Bench(node: hex(FakeNode.id), to: to, pairing: pairing),
      _Host(lines, node),
      image: () async => image,
    );
    await t.pumpWidget(const SizedBox());
    await app.session.disconnect();
    app.dispose();
    ble.dispose();
    await t.pump(const Duration(minutes: 1));
    return (ok, lines);
  }

  testWidgets('every step against a node the phone pairs with', (t) async {
    final (ok, lines) = await run(t, paired: false, image: imageFile(2, 5000));
    expect(ok, isTrue, reason: lines.join('\n'));
    expect(lines.where((l) => l.startsWith('bark-test pass')), hasLength(5));
    expect(lines, contains('bark-test pairing bark-fa4e00000001'));
    expect(lines, contains(startsWith('bark-test pass 2 pair: paired by')));
    expect(
      lines,
      contains(
        matches(
          RegExp(
            r'^bark-test sent dst=[0-9a-f]{26} '
            r'id=[0-9a-f]{8}$',
          ),
        ),
      ),
    );
    expect(
      lines,
      contains(
        matches(
          RegExp(
            r'^bark-test send dst=[0-9a-f]{26} '
            r'payload=[0-9a-f]+ id=[0-9a-f]{8}$',
          ),
        ),
      ),
    );
    expect(lines.last, 'bark-test done 5/5');
  });

  testWidgets('over the bond, with no image', (t) async {
    final (ok, lines) = await run(t, paired: true, pairing: Pairing.bond);
    expect(ok, isTrue, reason: lines.join('\n'));
    expect(lines, isNot(contains(startsWith('bark-test pairing'))));
    expect(
      lines,
      contains('bark-test pass 2 pair: over the bond the phone held'),
    );
    expect(lines, contains('bark-test skip 5 update: no image given'));
    expect(lines.last, 'bark-test done 4/5');
  });

  testWidgets('a fresh pairing where the phone holds a bond', (t) async {
    final (ok, lines) = await run(t, paired: true, pairing: Pairing.fresh);
    expect(ok, isTrue, reason: lines.join('\n'));
    expect(lines, contains('bark-test pairing bark-fa4e00000001'));
    expect(lines, contains('bark-test pass 2 pair: paired by the code'));
  });

  testWidgets('a bond asked for where the phone holds none', (t) async {
    final (ok, lines) = await run(t, paired: false, pairing: Pairing.bond);
    expect(ok, isFalse);
    expect(lines, [
      'bark-test begin 1 find',
      startsWith('bark-test pass 1 find: ID FA4E00000001, -60 dBm, public'),
      'bark-test begin 2 pair',
      'bark-test fail 2 pair: the phone is not paired with the node',
      for (final (n, s) in [(3, 'send'), (4, 'receive'), (5, 'update')]) ...[
        'bark-test begin $n $s',
        'bark-test skip $n $s: step 2 did not pass',
      ],
      'bark-test done 1/5',
    ]);
  });

  test('a node ID is twelve hex digits, and not the phone\'s', () {
    expect(
      () => Bench(node: 'fa4e0001', to: '0ce400000002'),
      throwsArgumentError,
    );
    expect(
      () => Bench(node: 'FA4E00000001', to: 'fa4e00000001'),
      throwsArgumentError,
    );
    expect(
      Bench(node: 'FA4E00000001', to: '0ce400000002').node,
      'fa4e00000001',
    );
  });
}
