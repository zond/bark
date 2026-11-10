// What Settings' sections share: a section's heading, a try that says
// why it failed, and a connection the person asks for, with what it says
// when it ends without one.

import 'package:flutter/material.dart';

import '../app.dart';
import '../ble_link.dart';
import '../link.dart';
import '../node_link.dart';
import 'common.dart';

/// A section's heading, with an info button where `help` says more.
class SettingsHeading extends StatelessWidget {
  final String title;
  final String? help;
  const SettingsHeading(this.title, {this.help, super.key});

  @override
  Widget build(BuildContext context) => Padding(
    padding: EdgeInsets.only(top: 24, bottom: help == null ? 8 : 0),
    child: Row(
      children: [
        Text(title, style: Theme.of(context).textTheme.titleLarge),
        if (help case final h?) HelpButton(title, h),
      ],
    ),
  );
}

/// Runs `f`, and says why it `failed` if it does.
Future<void> trySay(
  BuildContext context,
  Future<void> Function() f,
  String failed,
) async {
  try {
    await f();
  } catch (e) {
    if (context.mounted) say(context, '$failed: $e');
  }
}

/// A connection the person asks for in Settings, whichever section asks.
class ConnectAsk {
  final AppModel app;
  ConnectAsk(this.app);

  /// A connection asked for has not ended: a second tap meanwhile does
  /// nothing, as it would close the link the first is making.
  bool _asked = false;

  /// Connects by `f`, at the first connection after asking to notify;
  /// `pairs` when `f` pairs, whose failure a dialog tells, as the person
  /// looks at the node's screen for the code then, not at a passing line;
  /// `asGuest` for a node the phone uses as a guest, which it goes on
  /// doing if `f` fails.
  Future<void> call(
    BuildContext context,
    Future<void> Function() f, {
    bool pairs = false,
    bool asGuest = false,
  }) async {
    if (_asked) return;
    _asked = true;
    try {
      await _once(context, f, pairs: pairs, asGuest: asGuest);
    } finally {
      _asked = false;
    }
  }

  Future<void> _once(
    BuildContext context,
    Future<void> Function() f, {
    bool pairs = false,
    bool asGuest = false,
  }) async {
    await app.askToNotify(
      () async =>
          await showDialog<bool>(
            context: context,
            builder: (context) => AlertDialog(
              title: const Text('Notifications'),
              content: const Text(
                'bark tells you when a text comes. Android asks next.',
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.pop(context, false),
                  child: const Text('Not now'),
                ),
                FilledButton(
                  onPressed: () => Navigator.pop(context, true),
                  child: const Text('Continue'),
                ),
              ],
            ),
          ) ??
          false,
    );
    try {
      await f();
    } on NotOurs catch (e) {
      if (context.mounted) await _notOurs(context, e);
    } on BondKept catch (e) {
      if (context.mounted) await askToForget(context, app, '$e');
    } catch (e) {
      if (!context.mounted) return;
      final why = asGuest
          ? notPairedLine(e)
          : e is NotPaired || e is LinkEnded || e is NotSeen
          ? '$e.'
          : '${pairs ? 'Pairing' : 'Connecting'} failed: $e';
      if (pairs) {
        await _notPaired(context, why);
      } else {
        say(context, why);
      }
    }
  }

  /// Tells why a pairing ended without one.
  Future<void> _notPaired(BuildContext context, String why) => showDialog<void>(
    context: context,
    builder: (context) => AlertDialog(
      title: const Text('Not paired'),
      content: Text(why),
      actions: [
        FilledButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('OK'),
        ),
      ],
    ),
  );

  /// The node no longer holds this phone's bond: pair again, by the code
  /// on its screen, or leave it.
  Future<void> _notOurs(BuildContext context, NotOurs e) async {
    final again = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('This node is no longer yours'),
        content: Text(
          '${shownName(e.node)} no longer knows this phone. Pair again with '
          'the code on its screen.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Not now'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Pair again'),
          ),
        ],
      ),
    );
    final pair = app.pairAgain;
    if (again != true || pair == null || !context.mounted) return;
    await _once(context, pair, pairs: true);
  }
}
