// Small pieces the screens share.

import 'package:flutter/material.dart';

import '../app.dart';
import '../session.dart';

void say(BuildContext context, String s) =>
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(s)));

String clock(int s) => '${s ~/ 60}:${(s % 60).toString().padLeft(2, '0')}';

/// Hours and minutes, or the date too when not today.
String when(DateTime t) {
  final l = t.toLocal();
  final n = DateTime.now();
  String two(int v) => v.toString().padLeft(2, '0');
  final hm = '${two(l.hour)}:${two(l.minute)}';
  if (l.year == n.year && l.month == n.month && l.day == n.day) return hm;
  return '${l.year}-${two(l.month)}-${two(l.day)} $hm';
}

/// How long ago, in a few words.
String ago(Duration d) {
  if (d.inMinutes < 1) return 'just now';
  if (d.inHours < 1) return '${d.inMinutes} min ago';
  if (d.inDays < 2) return '${d.inHours} h ago';
  return '${d.inDays} days ago';
}

/// What a sent text's state means to the person.
String stateLabel(Session s, Outgoing o) => switch (o.state) {
  OutState.queued => 'queued',
  OutState.writing => 'offering to the node',
  OutState.taken => 'taken by the node',
  OutState.givenBack => switch (s.waitLeftS(o)) {
    0 => 'given back, wait over',
    final w => 'given back, wait ${clock(w)}',
  },
  OutState.refused => 'refused: ${o.error}',
};

/// Whether a sent text's parts are all in one state, so that one part's
/// marks stand for them all.
bool partsAgree(List<Outgoing> outs) => outs.every(
  (o) => o.state == outs.first.state && o.acked == outs.first.acked,
);

/// A sent text's parts' states in words when they differ: how many the
/// node has taken, how many are acknowledged, and the first problem.
String partsLabel(Session s, List<Outgoing> outs) {
  final taken = outs.where((o) => o.acked || o.state == OutState.taken);
  final acked = outs.where((o) => o.acked);
  final problem = outs
      .where(
        (o) =>
            !o.acked &&
            (o.state == OutState.givenBack || o.state == OutState.refused),
      )
      .firstOrNull;
  return [
    '${taken.length} of ${outs.length} taken',
    if (acked.isNotEmpty) '${acked.length} acknowledged',
    if (problem != null) 'part ${problem.part} ${stateLabel(s, problem)}',
  ].join(', ');
}

/// A sent text's state in words, one part's or the parts'.
String textLabel(Session s, List<Outgoing> outs) =>
    partsAgree(outs) ? stateLabel(s, outs.first) : partsLabel(s, outs);

/// Rebuilds on every change of the model, the session and the contacts.
class Watch extends StatelessWidget {
  final AppModel app;
  final WidgetBuilder builder;
  const Watch(this.app, this.builder, {super.key});

  @override
  Widget build(BuildContext context) =>
      ListenableBuilder(listenable: app.changes, builder: (c, _) => builder(c));
}

/// The session's last notice, dismissible.
class NoticeCard extends StatelessWidget {
  final AppModel app;
  const NoticeCard(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    final n = app.session.notice;
    if (n == null) return const SizedBox.shrink();
    return Card(
      color: Theme.of(context).colorScheme.errorContainer,
      child: ListTile(
        title: Text(n),
        trailing: IconButton(
          icon: const Icon(Icons.close),
          onPressed: app.session.dismissNotice,
        ),
      ),
    );
  }
}

/// "Fetch my mail", in an app bar, while the person has a home node; it
/// says what was done, or why nothing was sent. While a fetch waits for
/// its answer the button is off and says so; "No new mail", or how many
/// more the home holds, is told once, and the count stays on the button
/// until a fetch brings them.
class FetchButton extends StatelessWidget {
  final AppModel app;
  const FetchButton(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    if (app.me.home == null) return const SizedBox.shrink();
    final s = app.session;
    final news = s.fetchNews;
    if (news != null && ModalRoute.of(context)?.isCurrent != false) {
      s.fetchNews = null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (context.mounted) say(context, news);
      });
    }
    final waiting = s.fetchWaiting;
    if (waiting != null) {
      return IconButton(
        tooltip:
            'Waiting for your home node to answer, '
            '${clock(s.fetchWaitLeftS(waiting))} at most',
        icon: const Icon(Icons.hourglass_top),
        onPressed: null,
      );
    }
    final more = app.me.remaining;
    return IconButton(
      tooltip: 'Fetch my mail',
      icon: Badge(
        label: Text('$more'),
        isLabelVisible: more > 0,
        child: const Icon(Icons.move_to_inbox),
      ),
      onPressed: () async {
        try {
          final r = await app.fetchNow();
          if (context.mounted) say(context, r);
        } catch (e) {
          if (context.mounted) say(context, 'Fetching failed: $e');
        }
      },
    );
  }
}

/// While the card has changed and not gone to the contacts, says why, with
/// **Send my card**: the app sends no card by itself (roadmap D126).
class SendCardLine extends StatelessWidget {
  final AppModel app;
  final EdgeInsetsGeometry? padding;
  const SendCardLine(this.app, {this.padding, super.key});

  @override
  Widget build(BuildContext context) {
    if (!app.cardUnsent) return const SizedBox.shrink();
    final n = app.contacts.list.length;
    return ListTile(
      contentPadding: padding,
      leading: const Icon(Icons.badge_outlined),
      title: Text(app.cardChange),
      subtitle: Text(
        n == 1
            ? 'Your contact has the old one.'
            : 'Your $n contacts have the old one; one message each.',
      ),
      trailing: FilledButton(
        onPressed: () async {
          try {
            await app.messenger.sendCardToAll();
            if (context.mounted) say(context, 'Your card is queued');
          } catch (e) {
            if (context.mounted) say(context, 'Sending the card failed: $e');
          }
        },
        child: const Text('Send my card'),
      ),
    );
  }
}

/// Asks for one line of text; null if the person cancels.
Future<String?> askText(BuildContext context, String label, String initial) {
  final c = TextEditingController(text: initial);
  return showDialog<String>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(label),
      content: TextField(
        controller: c,
        autofocus: true,
        textCapitalization: TextCapitalization.words,
        onSubmitted: (v) => Navigator.pop(context, v.trim()),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, c.text.trim()),
          child: const Text('Save'),
        ),
      ],
    ),
  );
}

/// The search for the paired node, while it scans or waits for
/// Bluetooth; null when there is none.
String? lookingLine(AppModel app) {
  final n = app.searchFor;
  if (n == null || app.connecting) return null;
  if (!app.bluetoothOn) return 'Bluetooth is off: turn it on to reach $n.';
  return app.scanning ? null : 'Looking for $n…';
}

/// The phone's pairing as a person reads it while not connected (spec
/// "Phone link", Pairing).
String pairedLine(Session s) {
  final n = s.pairedWith;
  if (n == null) {
    return 'Not paired with a node yet: the first connection pairs, by a '
        'code on the node\'s screen.';
  }
  return s.pairingLost
      ? '$n is no longer yours: another phone has paired with it.'
      : 'Paired with $n.';
}
