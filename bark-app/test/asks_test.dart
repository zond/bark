// The rule of whom the map asks and when, as a table: each row a node's
// history, then a look at it (the map opened or refreshed, or its sheet
// opened), and whether that look asks it.

import 'package:bark_app/asks.dart';
import 'package:flutter_test/flutter_test.dart';

/// A step of a node's history, at its time.
typedef Step = void Function(Asks);

void main() {
  final t0 = DateTime.utc(2026, 11, 12);
  const id = 'fa4e00000101';
  DateTime at(Duration d) => t0.add(d);
  const s = Duration(seconds: 1);

  Step look(Duration d) =>
      (a) => a.look([id], at(d));
  Step sent(Duration d) =>
      (a) => a.sent(id, at(d));
  Step answered(Duration d) =>
      (a) => a.answered(id, at(d));
  Step due(Duration d) =>
      (a) => a.due(at(d));

  /// A series that ran out: three asks, each unanswered.
  final ranOut = [
    look(Duration.zero),
    sent(Duration.zero),
    due(s * 2),
    sent(s * 2),
    due(s * 4),
    sent(s * 4),
    due(s * 6),
  ];

  test('a look asks a node unless its asks are under way or it answered '
      'within 10 s', () {
    for (final (what, steps, lookAt, state, asks) in [
      ('never asked', <Step>[], s * 0, AskState.never, true),
      ('its ask being written', [look(s * 0)], s * 0, AskState.asking, false),
      (
        'waiting for its answer',
        [look(s * 0), sent(s * 0)],
        s,
        AskState.asking,
        false,
      ),
      (
        'asked again, waiting',
        [look(s * 0), sent(s * 0), due(s * 2), sent(s * 2)],
        s * 3,
        AskState.asking,
        false,
      ),
      (
        'answered 5 s ago',
        [look(s * 0), sent(s * 0), answered(s)],
        s * 6,
        AskState.answered,
        false,
      ),
      (
        'answered 10 s ago',
        [look(s * 0), sent(s * 0), answered(s)],
        s * 11,
        AskState.answered,
        true,
      ),
      ('gave up', ranOut, s * 6, AskState.gaveUp, true),
      (
        'shut while asking',
        [look(s * 0), sent(s * 0), (Asks a) => a.stop()],
        s,
        AskState.never,
        true,
      ),
    ]) {
      final a = Asks();
      for (final step in steps) {
        step(a);
      }
      expect(a.state(id), state, reason: what);
      expect(a.look([id], at(lookAt)), asks ? [id] : isEmpty, reason: what);
      if (asks) expect(a.state(id), AskState.asking, reason: what);
    }
  });

  test('a series: asked again 2 s after each ask is written, three asks, '
      'then given up', () {
    final a = Asks()..look([id], t0);
    // Its wait starts once its ask is written.
    expect(a.next, isNull);
    a.sent(id, at(s ~/ 2));
    expect(a.next, at(s ~/ 2 + Asks.wait));
    expect(a.due(at(s * 2)).again, isEmpty);
    expect(a.due(at(s ~/ 2 + Asks.wait)).again, [id]);
    expect(a.next, isNull);
    a.sent(id, at(s * 3));
    expect(a.due(at(s * 5)).again, [id]);
    a.sent(id, at(s * 5));
    final last = a.due(at(s * 7));
    expect(last.again, isEmpty);
    expect(last.gaveUp, [id]);
    expect(a.state(id), AskState.gaveUp);
    expect(a.next, isNull);
  });

  test('an answer ends its series', () {
    final a = Asks()
      ..look([id], t0)
      ..sent(id, t0)
      ..answered(id, at(s));
    expect(a.state(id), AskState.answered);
    expect(a.next, isNull);
    expect(a.due(at(s * 10)).again, isEmpty);
  });

  test('a look asks only those it may, each once', () {
    const other = 'fa4e00000102';
    final a = Asks()..look([id], t0);
    expect(a.look([id, other, other], t0), [other]);
  });
}
