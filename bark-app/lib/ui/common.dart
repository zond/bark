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
/// says what was done, or why nothing was sent.
class FetchButton extends StatelessWidget {
  final AppModel app;
  const FetchButton(this.app, {super.key});

  @override
  Widget build(BuildContext context) {
    if (app.me.home == null) return const SizedBox.shrink();
    return IconButton(
      tooltip: 'Fetch my mail',
      icon: const Icon(Icons.move_to_inbox),
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
