// The first screen: one row a conversation, newest first.

import 'package:flutter/material.dart';

import '../app.dart';
import '../conversations.dart';
import '../link.dart';
import 'card_screen.dart';
import 'common.dart';
import 'contacts_screen.dart';
import 'debug_screen.dart';
import 'settings_screen.dart';
import 'thread_screen.dart';

class ConversationsScreen extends StatelessWidget {
  final AppModel app;
  const ConversationsScreen(this.app, {super.key});

  void _open(BuildContext context, Widget screen) =>
      Navigator.of(context).push(MaterialPageRoute(builder: (_) => screen));

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    final list = conversations(app.contacts.list, s.outbox, s.received);
    return Scaffold(
      appBar: AppBar(
        title: const Text('bark'),
        actions: [
          FetchButton(app),
          IconButton(
            tooltip: 'My card',
            icon: const Icon(Icons.qr_code),
            onPressed: () => _open(context, CardScreen(app)),
          ),
          PopupMenuButton<Widget Function()>(
            onSelected: (f) => _open(context, f()),
            itemBuilder: (_) => [
              PopupMenuItem(
                value: () => ContactsScreen(app),
                child: const Text('Contacts'),
              ),
              PopupMenuItem(
                value: () => SettingsScreen(app),
                child: const Text('Settings'),
              ),
              PopupMenuItem(
                value: () => DebugScreen(app),
                child: const Text('Debug'),
              ),
            ],
          ),
        ],
      ),
      body: ListView(
        children: [
          NoticeCard(app),
          if (!app.me.backedUp)
            Card(
              child: ListTile(
                leading: const Icon(Icons.key),
                title: const Text('Write down your 12 backup words'),
                subtitle: const Text(
                  'They are your identity; without them a lost phone '
                  'loses it. In Settings.',
                ),
                onTap: () => _open(context, SettingsScreen(app)),
              ),
            ),
          _connection(context),
          SendCardLine(app),
          if (list.isEmpty)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Text(
                'No conversations yet. Add a contact, or show your card '
                'to someone who has the app.',
              ),
            ),
          for (final c in list) _row(context, c),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        tooltip: 'New conversation',
        onPressed: () => _open(context, ContactsScreen(app)),
        child: const Icon(Icons.edit),
      ),
    );
  });

  Widget _connection(BuildContext context) {
    final s = app.session;
    final text = s.connected
        ? 'Connected to ${nodeName(s.node!.id)}, paired'
        : 'Not connected to a node: texts wait on the phone. '
              '${pairedLine(s)}';
    return ListTile(
      dense: true,
      leading: Icon(
        s.connected ? Icons.bluetooth_connected : Icons.bluetooth_disabled,
      ),
      title: Text(text),
      onTap: () => _open(context, SettingsScreen(app)),
    );
  }

  Widget _row(BuildContext context, Conversation c) {
    final last = c.last!;
    final o = last.sent;
    return ListTile(
      leading: CircleAvatar(
        child: Icon(c.contact == null ? Icons.person_outline : Icons.person),
      ),
      title: Text(c.title, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        o == null
            ? last.text
            : 'You: ${last.text}, ${stateLabel(app.session, o)}',
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
      ),
      trailing: Text(when(last.at)),
      onTap: () => _open(context, ThreadScreen(app, c.key)),
    );
  }
}
