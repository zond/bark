// The archived conversations and contacts, and the pieces the lists
// share to archive a row and to lead here.

import 'package:flutter/material.dart';

import '../app.dart';
import '../conversations.dart';
import 'common.dart';
import 'conversations_screen.dart';
import 'thread_screen.dart';

class ArchivedScreen extends StatelessWidget {
  final AppModel app;
  const ArchivedScreen(this.app, {super.key});

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    final cs = app.contacts;
    final threads = [
      for (final c in conversations(cs.list, s.outbox, s.received))
        if (cs.isArchived(c.key)) c,
    ];
    final contacts = [
      for (final c in cs.list)
        if (c.archived) c,
    ];
    final heading = Theme.of(context).textTheme.titleSmall;
    Widget title(String t) => Padding(
      padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
      child: Text(t, style: heading),
    );
    return Scaffold(
      appBar: AppBar(title: const Text('Archived')),
      body: ListView(
        children: [
          if (threads.isEmpty && contacts.isEmpty)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Text(
                'Nothing archived. Swipe a conversation or contact aside to '
                'archive it.',
              ),
            ),
          if (threads.isNotEmpty) title('Conversations'),
          for (final c in threads)
            ConversationRow(app, c, trailing: _unarchive(c.key)),
          if (contacts.isNotEmpty) title('Contacts'),
          for (final c in contacts)
            ListTile(
              leading: const CircleAvatar(child: Icon(Icons.person)),
              title: Text(c.name),
              trailing: _unarchive(c.key),
              onTap: () => Navigator.of(context).push(
                MaterialPageRoute(builder: (_) => ThreadScreen(app, c.key)),
              ),
            ),
        ],
      ),
    );
  });

  Widget _unarchive(String key) => IconButton(
    tooltip: 'Unarchive',
    icon: const Icon(Icons.unarchive),
    onPressed: () => app.contacts.unarchive(key),
  );
}

/// What shows under a row as it is swiped aside: that it goes to the
/// archive.
class SwipeBackground extends StatelessWidget {
  final Alignment alignment;
  const SwipeBackground(this.alignment, {super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      color: scheme.secondaryContainer,
      alignment: alignment,
      padding: const EdgeInsets.symmetric(horizontal: 24),
      child: Icon(Icons.archive, color: scheme.onSecondaryContainer),
    );
  }
}

/// The line at the bottom of a list that says how much of it is
/// archived and leads there.
class ArchivedLine extends StatelessWidget {
  final String text;
  final VoidCallback onTap;
  const ArchivedLine(this.text, {required this.onTap, super.key});

  @override
  Widget build(BuildContext context) => ListTile(
    dense: true,
    leading: const Icon(Icons.archive_outlined),
    title: Text(text),
    onTap: onTap,
  );
}

/// Says `what` was archived, with an undo: a swipe is easily made by
/// mistake. Takes the messenger, so that a screen that closes as it
/// archives can still say it.
void archivedSay(
  ScaffoldMessengerState m,
  AppModel app,
  String key,
  String what,
) => m.showSnackBar(
  SnackBar(
    content: Text(what),
    // A bar with an action stays until dismissed by default; this one
    // would cover the next screen's field.
    persist: false,
    action: SnackBarAction(
      label: 'Undo',
      onPressed: () => app.contacts.unarchive(key),
    ),
  ),
);
