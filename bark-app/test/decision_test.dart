// The search's decision, as tables of situations and what the search
// does in each: whether it runs, when it listens, and which node it takes
// as whose (`bark-app/README.md`, "Screens"; roadmap D163, D171).

import 'package:bark_app/ble_link.dart';
import 'package:bark_app/decision.dart';
import 'package:bark_app/node_link.dart' show swapAway;
import 'package:bark_app/nodes.dart';
import 'package:flutter_test/flutter_test.dart';

final t0 = DateTime.utc(2026, 11, 10, 12);

/// A round's scan started 5 s ago, so the next is due in 10 s.
final scanned = t0.subtract(const Duration(seconds: 5));

NodeRecord paired(String n) => NodeRecord(n)..paired = true;
NodeRecord lost(String n) => NodeRecord(n)
  ..paired = true
  ..lost = true;
NodeRecord kept(String n) => NodeRecord(n)..bondKept = true;

/// A node that refused the phone over the link version, paired or not.
NodeRecord unserved(String n, {bool isPaired = false}) => NodeRecord(n)
  ..paired = isPaired
  ..unserved = true;
NodeRecord read(String n, Duration ago) =>
    NodeRecord(n)..ringReadAt = t0.subtract(ago);

FoundNode h(String n, int? rssi, {bool public = true}) =>
    FoundNode(n, n, rssi, public: public);

/// A home `home`, paired, and another paired node `mine`.
final homeAndMine = [paired('home'), paired('mine')];

Situation at({
  String? home,
  List<NodeRecord> nodes = const [],
  LinkNow? link,
  bool linking = false,
  bool radio = true,
  bool stopped = false,
  Awaited? awaited,
  int missed = 0,
  bool busy = false,
  bool toSend = false,
  DateTime? scannedAt,
  bool scanNever = false,
  DateTime? droppedAt,
  List<FoundNode>? heard,
}) => Situation(
  now: t0,
  home: home,
  nodes: nodes,
  link: link,
  linking: linking,
  radio: radio,
  stopped: stopped,
  awaited: awaited,
  missed: missed,
  busy: busy,
  toSend: toSend,
  scannedAt: scanNever ? null : scannedAt ?? scanned,
  droppedAt: droppedAt,
  heard: heard,
);

void table(String name, List<(String, Situation, String)> rows) {
  group(name, () {
    for (final (what, s, want) in rows) {
      test(what, () => expect('${decide(s)}', want));
    }
  });
}

void main() {
  group('chooseNode', () {
    FoundNode n(String name, int? rssi, {bool public = true}) =>
        FoundNode(name, name, rssi, public: public);

    test('the strongest public node; one not public is passed over', () {
      final heard = [
        n('a', -70),
        n('b', -50),
        n('c', -40, public: false),
        n('d', null),
      ];
      expect(chooseNode(heard)?.name, 'b');
      expect(chooseNode([n('d', null), n('a', -100)])?.name, 'a');
      expect(chooseNode([n('d', null)])?.name, 'd');
    });

    test('the paired node, heard, before any public node', () {
      final heard = [n('a', -40), n('mine', -80, public: false)];
      expect(chooseNode(heard, paired: {'mine'})?.name, 'mine');
      // From a guest's link at a public node, the paired node is higher.
      expect(chooseNode(heard, paired: {'mine'}, on: 'b')?.name, 'mine');
      // Not heard: the strongest public node, unless a link is up.
      expect(chooseNode([n('a', -40)], paired: {'mine'})?.name, 'a');
      expect(chooseNode([n('a', -40)], paired: {'mine'}, on: 'b'), isNull);
    });

    test('the home before every other paired node; else the strongest '
        'paired node, before any public node', () {
      final heard = [
        n('home', -84, public: false),
        n('near', -50, public: false),
        n('far', -80),
        n('a', -40),
      ];
      const paired = {'home', 'near', 'far'};
      expect(chooseNode(heard, home: 'home', paired: paired)?.name, 'home');
      expect(chooseNode(heard, paired: paired)?.name, 'near');
      // A home the phone is not paired with is no paired node.
      expect(chooseNode(heard, home: 'home', paired: {'far'})?.name, 'far');
      expect(
        chooseNode(heard.skip(1), home: 'home', paired: paired)?.name,
        'near',
      );
    });

    test('a weak home is passed over for a fair paired node', () {
      final heard = [
        n('home', weakDbm - 1, public: false),
        n('fair', weakDbm, public: false),
      ];
      const paired = {'home', 'fair'};
      expect(chooseNode(heard, home: 'home', paired: paired)?.name, 'fair');
    });

    test('a weak paired node is passed over for a fair public one', () {
      final heard = [n('mine', weakDbm - 1, public: false), n('a', weakDbm)];
      expect(chooseNode(heard, paired: {'mine'})?.name, 'a');
      // And a weak home, the same.
      expect(chooseNode(heard, home: 'mine', paired: {'mine'})?.name, 'a');
    });

    test('a weak node is taken when it is the only one', () {
      expect(
        chooseNode(
          [n('home', -100, public: false)],
          home: 'home',
          paired: {'home'},
        )?.name,
        'home',
      );
      expect(chooseNode([n('a', -100)])?.name, 'a');
    });

    test('nothing usable heard: the strongest, whatever its kind', () {
      // A weak home and a slightly stronger weak public node: the public.
      expect(
        chooseNode(
          [n('home', -90, public: false), n('a', -89)],
          home: 'home',
          paired: {'home'},
        )?.name,
        'a',
      );
      final weak = [
        n('a', -95),
        n('mine', -88, public: false),
        n('home', -100, public: false),
      ];
      expect(
        chooseNode(weak, home: 'home', paired: {'home', 'mine'})?.name,
        'mine',
      );
      // Only nodes the phone may use: not one private and not its own,
      // nor one whose bond Android keeps.
      expect(
        chooseNode([...weak, n('shut', -86, public: false)], paired: {})?.name,
        'a',
      );
      expect(
        chooseNode([n('kept', -86), n('a', -95)], passed: {'kept'})?.name,
        'a',
      );
      // Heard without an RSSI: ranked as the weakest.
      expect(
        chooseNode(
          [n('mine', -90, public: false), n('a', null)],
          paired: {'mine'},
        )?.name,
        'mine',
      );
    });

    test('on a link, only a node higher in the order, heard usable', () {
      final heard = [
        n('home', -60, public: false),
        n('other', -40, public: false),
        n('a', -30),
      ];
      const paired = {'home', 'mine', 'other'};
      // From a paired node, up to the home.
      expect(
        chooseNode(heard, home: 'home', paired: paired, on: 'mine')?.name,
        'home',
      );
      // Never sideways to another paired node, however strong.
      expect(
        chooseNode(heard.skip(1), home: 'home', paired: paired, on: 'mine'),
        isNull,
      );
      // On the home, nothing is higher.
      expect(
        chooseNode(heard, home: 'home', paired: paired, on: 'home'),
        isNull,
      );
      // A weak home is not worth the link in use.
      expect(
        chooseNode(
          [n('home', -95, public: false)],
          home: 'home',
          paired: paired,
          on: 'mine',
        ),
        isNull,
      );
      // From a guest's link, never sideways to another public node.
      expect(chooseNode([n('b', -30)], on: 'a'), isNull);
    });

    test('no public node heard: none', () {
      expect(chooseNode([]), isNull);
      expect(chooseNode([n('c', -40, public: false)]), isNull);
    });

    test('never a node passed over, of any kind', () {
      final heard = [n('old', -40), n('a', -80)];
      expect(chooseNode(heard, passed: {'old'})?.name, 'a');
      expect(chooseNode([n('old', -40)], passed: {'old'}), isNull);
      // Nor one paired with, nor with none usable.
      expect(
        chooseNode(
          [n('home', -40, public: false), n('a', -95)],
          home: 'home',
          paired: {'home'},
          passed: {'home'},
        )?.name,
        'a',
      );
    });
  });

  table('whether the search runs', [
    ('the person stopped it', at(stopped: true), 'Idle'),
    ('a connection is being made', at(linking: true), 'Idle'),
    ('Bluetooth cannot scan', at(radio: false), 'Idle'),
    (
      'on the home',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'home', guest: false, byTap: false),
      ),
      'Idle',
    ),
    (
      'on another paired node without a home: never sideways',
      at(
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: false),
        scanNever: true,
      ),
      'Idle',
    ),
    (
      'on another paired node the person tapped',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: true),
        scanNever: true,
      ),
      'Idle',
    ),
    (
      'a tap on a public node while the home is in reach: the person\'s '
          'link holds',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'cafe', guest: true, byTap: true),
        heard: [h('home', -60, public: false), h('cafe', -50)],
      ),
      'Idle',
    ),
  ]);

  table('when it listens', [
    ('at start, for any node', at(scanNever: true), 'Listen(all)'),
    (
      'at start, ending early on the home',
      at(home: 'home', nodes: homeAndMine, scanNever: true),
      'Listen(home)',
    ),
    (
      'without a home, ending early on the one paired node',
      at(nodes: [paired('mine')], scanNever: true),
      'Listen(mine)',
    ),
    (
      'a home not paired with is no stop',
      at(home: 'home', nodes: [paired('a'), paired('b')], scanNever: true),
      'Listen(all)',
    ),
    (
      'a home passed over for its version is no stop; the one other '
          'paired node is',
      at(
        home: 'home',
        nodes: [unserved('home', isPaired: true), paired('mine')],
        scanNever: true,
      ),
      'Listen(mine)',
    ),
    ('within a round of the last scan: the rest of it', at(), 'Wait(10 s)'),
    (
      'a link lost since the last scan: at once',
      at(droppedAt: t0.subtract(const Duration(seconds: 1))),
      'Listen(all)',
    ),
    (
      'on another paired node, a minute after the last scan: for the home',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: false),
        scannedAt: t0.subtract(lookAround),
      ),
      'Listen(home)',
    ),
    (
      'on another paired node within the minute: the rest of it',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: false),
      ),
      'Wait(55 s)',
    ),
  ]);

  table('which node a round takes', [
    (
      'the home before every other paired node, though weaker',
      at(
        home: 'home',
        nodes: homeAndMine,
        heard: [h('home', -84, public: false), h('mine', -50, public: false)],
      ),
      'Connect(home)',
    ),
    (
      'without a home, the paired node heard strongest',
      at(
        nodes: [paired('a'), paired('b')],
        heard: [h('a', -70, public: false), h('b', -60, public: false)],
      ),
      'Connect(b)',
    ),
    (
      'a paired node before a stronger public one, at once',
      at(nodes: [paired('mine')], heard: [h('mine', -80), h('cafe', -40)]),
      'Connect(mine)',
    ),
    (
      'a weak home with a fair paired node: the paired node',
      at(
        home: 'home',
        nodes: homeAndMine,
        heard: [
          h('home', weakDbm - 1, public: false),
          h('mine', weakDbm, public: false),
        ],
      ),
      'Connect(mine)',
    ),
    (
      'a weak paired node with a fair public one: an errand there at once',
      at(
        nodes: [paired('mine')],
        heard: [h('mine', weakDbm - 1, public: false), h('cafe', weakDbm)],
      ),
      'Errand(cafe)',
    ),
    (
      'nothing usable: the strongest, a paired node',
      at(
        nodes: [paired('mine')],
        heard: [h('mine', -88, public: false), h('cafe', -95)],
      ),
      'Connect(mine)',
    ),
    (
      'nothing usable: the strongest, a public node, before a weaker '
          'paired one',
      at(
        nodes: [paired('mine')],
        heard: [h('mine', -95, public: false), h('cafe', -88)],
      ),
      'Errand(cafe)',
    ),
    ('nothing heard', at(nodes: [paired('mine')], heard: []), 'Wait(10 s)'),
    (
      'only a node not public and not the phone\'s',
      at(heard: [h('shut', -40, public: false)]),
      'Wait(10 s)',
    ),
    (
      'a paired node missed once, a public node in reach: not yet',
      at(nodes: [paired('mine')], heard: [h('cafe', -50)]),
      'Wait(10 s)',
    ),
    (
      'a paired node missed in this round too: out of reach, an errand',
      at(nodes: [paired('mine')], missed: 1, heard: [h('cafe', -50)]),
      'Errand(cafe)',
    ),
    (
      'no paired node: the strongest public node',
      at(heard: [h('a', -70), h('b', -50)]),
      'Errand(b)',
    ),
    (
      'a node no longer the phone\'s, its bond gone: a public node',
      at(nodes: [lost('old')], heard: [h('old', -50)]),
      'Errand(old)',
    ),
    (
      'a node whose bond Android keeps: never',
      at(nodes: [kept('old')], heard: [h('old', -40), h('a', -80)]),
      'Errand(a)',
    ),
    (
      'only a node whose bond Android keeps: nothing',
      at(nodes: [kept('old')], heard: [h('old', -40)]),
      'Wait(10 s)',
    ),
    (
      'the home passed over for its version, another paired node heard: '
          'that node',
      at(
        home: 'home',
        nodes: [unserved('home', isPaired: true), paired('mine')],
        heard: [h('home', -50, public: false), h('mine', -80, public: false)],
      ),
      'Connect(mine)',
    ),
    (
      'the only paired node passed over for its version, a public node '
          'heard: an errand there at once, no round of waiting',
      at(
        nodes: [unserved('mine', isPaired: true)],
        heard: [h('mine', -50), h('cafe', -80)],
      ),
      'Errand(cafe)',
    ),
    (
      'the strongest public node passed over for its version: the weaker '
          'one',
      at(nodes: [unserved('a')], heard: [h('a', -40), h('b', -70)]),
      'Errand(b)',
    ),
    (
      'only a node passed over for its version heard: nothing',
      at(nodes: [unserved('a')], heard: [h('a', -40)]),
      'Wait(10 s)',
    ),
    (
      'a public node read 30 s ago, nothing to send: not yet',
      at(
        nodes: [read('cafe', const Duration(seconds: 30))],
        heard: [h('cafe', -50)],
      ),
      'Wait(10 s)',
    ),
    (
      'a public node read 30 s ago, a text to send: at once',
      at(
        nodes: [read('cafe', const Duration(seconds: 30))],
        toSend: true,
        heard: [h('cafe', -50)],
      ),
      'Errand(cafe)',
    ),
    (
      'a public node read a minute ago: due',
      at(nodes: [read('cafe', guestCheck)], heard: [h('cafe', -50)]),
      'Errand(cafe)',
    ),
  ]);

  final swapping = (
    node: 'home',
    gone: t0.subtract(const Duration(seconds: 20)),
  );
  final swapped = (
    node: 'home',
    gone: t0.subtract(swapAway + const Duration(seconds: 1)),
  );
  table('the wait for a node that installs', [
    (
      'in its swap, listening for it alone',
      at(home: 'home', nodes: homeAndMine, awaited: swapping, scanNever: true),
      'Listen(home)',
    ),
    (
      'in its swap, another paired node and a public node in reach: '
          'neither',
      at(
        home: 'home',
        nodes: homeAndMine,
        awaited: swapping,
        heard: [h('mine', -50, public: false), h('cafe', -40)],
      ),
      'Wait(10 s)',
    ),
    (
      'the first round after its swap: still that node alone',
      at(
        home: 'home',
        nodes: homeAndMine,
        awaited: swapped,
        heard: [h('mine', -50, public: false), h('cafe', -40)],
      ),
      'Wait(10 s)',
    ),
    (
      'back: taken, however weak',
      at(
        home: 'home',
        nodes: homeAndMine,
        awaited: swapping,
        heard: [h('home', -95, public: false), h('cafe', -40)],
      ),
      'Connect(home)',
    ),
    (
      'back, and passed over for its version: given up at once, before '
          'the round',
      at(
        home: 'home',
        nodes: [unserved('home', isPaired: true), paired('mine')],
        awaited: swapping,
        scanNever: true,
      ),
      'GiveUp(home)',
    ),
    (
      'missed in the first round after its swap too: given up',
      at(
        home: 'home',
        nodes: homeAndMine,
        awaited: swapped,
        missed: 1,
        heard: [h('cafe', -40)],
      ),
      'GiveUp(home)',
    ),
  ]);

  table('from a paired node to the home', [
    (
      'the home heard well, nothing in flight: there',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: false),
        heard: [h('home', -60, public: false)],
      ),
      'MoveHome(home)',
    ),
    (
      'the home heard well with a send in flight: the next minute',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: false),
        busy: true,
        heard: [h('home', -60, public: false)],
      ),
      'Wait(55 s)',
    ),
    (
      'the home heard weak: the link in use is kept',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: false),
        heard: [h('home', weakDbm - 1, public: false)],
      ),
      'Wait(55 s)',
    ),
    (
      'only a stronger public node heard: never down',
      at(
        home: 'home',
        nodes: homeAndMine,
        link: (node: 'mine', guest: false, byTap: false),
        heard: [h('cafe', -30)],
      ),
      'Wait(55 s)',
    ),
    (
      'the home passed over for its version: no scan for it, and no move',
      at(
        home: 'home',
        nodes: [unserved('home', isPaired: true), paired('mine')],
        link: (node: 'mine', guest: false, byTap: false),
        heard: [h('home', -60, public: false)],
      ),
      'Idle',
    ),
  ]);

  group('what a round leaves', () {
    test('a round without the paired nodes is a miss of them, but not '
        'while one swaps in its image', () {
      expect(countsMiss(at(nodes: [paired('mine')])), isTrue);
      expect(countsMiss(at()), isFalse);
      // Paired nodes all passed over for their version are no paired
      // nodes to miss.
      expect(
        countsMiss(at(nodes: [unserved('mine', isPaired: true)])),
        isFalse,
      );
      expect(
        countsMiss(
          at(
            nodes: [paired('home')],
            awaited: (
              node: 'home',
              gone: t0.subtract(const Duration(seconds: 20)),
            ),
          ),
        ),
        isFalse,
      );
    });

    test('the paired nodes heard but passed over were heard too weakly', () {
      expect(
        ownPassedOver(
          at(
            nodes: [paired('mine')],
            heard: [h('mine', weakDbm - 1, public: false), h('cafe', -50)],
          ),
        ),
        isTrue,
      );
      expect(
        ownPassedOver(
          at(nodes: [paired('mine')], heard: [h('mine', -50, public: false)]),
        ),
        isFalse,
      );
      expect(
        ownPassedOver(at(nodes: [paired('mine')], heard: [h('cafe', -50)])),
        isFalse,
      );
      // One passed over for its version was not heard too weakly.
      expect(
        ownPassedOver(
          at(
            nodes: [unserved('mine', isPaired: true)],
            heard: [h('mine', -50, public: false), h('cafe', -50)],
          ),
        ),
        isFalse,
      );
    });
  });
}
