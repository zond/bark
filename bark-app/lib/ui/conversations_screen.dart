// The first screen: one row a conversation, newest first, under a bar for
// each newer release of the app or the node's firmware, and a line to
// the archived ones.

import 'package:flutter/material.dart';

import '../app.dart';
import 'archived_screen.dart';
import '../conversations.dart';
import '../link.dart';
import 'card_screen.dart';
import 'common.dart';
import 'contacts_screen.dart';
import 'map_screen.dart';
import 'settings_screen.dart';
import 'thread_screen.dart';
import 'update_screen.dart';

class ConversationsScreen extends StatefulWidget {
  final AppModel app;
  const ConversationsScreen(this.app, {super.key});

  @override
  State<ConversationsScreen> createState() => _ConversationsScreenState();
}

class _ConversationsScreenState extends State<ConversationsScreen> {
  AppModel get app => widget.app;

  @override
  void initState() {
    super.initState();
    // Without Bluetooth there is no node, so the app asks at once.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) app.askForBluetooth(() => whyBluetooth(context));
    });
  }

  void _open(BuildContext context, Widget screen) =>
      Navigator.of(context).push(MaterialPageRoute(builder: (_) => screen));

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    final all = conversations(app.contacts.list, s.outbox, s.received);
    final list = [
      for (final c in all)
        if (!app.contacts.isArchived(c.key)) c,
    ];
    final archived = all.length - list.length;
    return Scaffold(
      appBar: AppBar(
        title: const Text('bark'),
        actions: [
          FetchButton(app),
          IconButton(
            tooltip: 'Contacts',
            // A head, as a contact is a person; account_circle would read
            // as the person's own account, which is My card.
            icon: const Icon(Icons.person, semanticLabel: 'Contacts'),
            onPressed: () => _open(context, ContactsScreen(app)),
          ),
          IconButton(
            tooltip: 'My card',
            icon: const Icon(Icons.qr_code),
            onPressed: () => _open(context, CardScreen(app)),
          ),
          PopupMenuButton<Widget Function()>(
            onSelected: (f) => _open(context, f()),
            itemBuilder: (_) => [
              PopupMenuItem(
                value: () => ArchivedScreen(app),
                child: const Text('Archived'),
              ),
              PopupMenuItem(
                value: () => MapScreen(app),
                child: const Text('Nearby nodes'),
              ),
              PopupMenuItem(
                value: () => SettingsScreen(app),
                child: const Text('Settings'),
              ),
            ],
          ),
        ],
      ),
      body: ListView(
        children: [
          ..._releases(context),
          NoticeCard(app),
          BondKeptLine(app),
          HomeOfferLine(app),
          ..._gettingStarted(context),
          // Once there is a contact, there is someone to lose.
          if (!app.me.backedUp && app.contacts.list.isNotEmpty)
            Card(
              child: ListTile(
                leading: const Icon(Icons.key),
                title: const Text('Write down your 12 backup words'),
                subtitle: const Text(
                  'If you lose this phone, they are the only way back to your '
                  'contacts.',
                ),
                onTap: () =>
                    _open(context, SettingsScreen(app, toBackup: true)),
              ),
            ),
          _connection(context),
          SendCardLine(app),
          if (all.isEmpty)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Text(
                'No conversations yet. Add a contact, or show your card.',
              ),
            ),
          for (final c in list) _row(context, c),
          if (archived > 0)
            ArchivedLine(
              '$archived archived',
              onTap: () => _open(context, ArchivedScreen(app)),
            ),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        tooltip: 'New conversation',
        onPressed: () => _open(context, ContactsScreen(app)),
        child: const Icon(Icons.edit),
      ),
    );
  });

  /// The steps to a first text, each ticked once done, until all are.
  List<Widget> _gettingStarted(BuildContext context) {
    final g = app.gettingStarted;
    if (g.name && g.node && g.contact) return [];
    Widget step(bool done, String title, String how, VoidCallback onTap) =>
        ListTile(
          dense: true,
          leading: Icon(
            done ? Icons.check_circle : Icons.radio_button_unchecked,
            semanticLabel: done ? 'Done' : 'To do',
          ),
          title: Text(title),
          subtitle: done ? null : Text(how),
          onTap: done ? null : onTap,
        );
    return [
      Card(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const ListTile(title: Text('Get started')),
            step(
              g.name,
              'Your name',
              'So people know who you are.',
              () => askMyName(context, app),
            ),
            step(
              g.node,
              'A node',
              'Found by itself when one is near.',
              () => _open(context, SettingsScreen(app)),
            ),
            step(
              g.contact,
              'Your first contact',
              'Scan their card, or show them yours.',
              () => _open(context, ContactsScreen(app)),
            ),
          ],
        ),
      ),
    ];
  }

  /// A bar for a newer app, and one for a newer image than the connected
  /// node runs; each says what a tap does and can be dismissed for that
  /// version.
  List<Widget> _releases(BuildContext context) {
    final r = app.releases;
    final a = r.newerApp;
    final node = app.session.node;
    // Only the paired phone updates a node.
    final i = app.session.guest ? null : r.newerImage(node);
    final updated = app.updater.updated;
    return [
      if (updated != null && app.updater.hearsNoOne && node != null)
        _bar(
          context,
          title: '${nodeName(node.id)} hears no one since its update',
          subtitle:
              'Its new firmware may have broken its radio. Tap for the '
              'way back.',
          onTap: () => _open(context, UpdateScreen(app)),
          dismiss: app.updater.dismissHearsNoOne,
          warning: true,
        ),
      if (a != null)
        _bar(
          context,
          title: 'App version ${a.app} is out',
          subtitle: 'Tap to download.',
          onTap: () => app.notifier.openLink(a.apk!),
          dismiss: () => r.dismissApp(a.app),
        ),
      if (i != null && node != null)
        _bar(
          context,
          title: 'Node firmware ${i.firmware} is out',
          subtitle: 'Tap to update the node.',
          onTap: () => _open(context, UpdateScreen(app, release: i)),
          dismiss: () => r.dismissImage(node, i.firmware),
        ),
    ];
  }

  Widget _bar(
    BuildContext context, {
    required String title,
    required String subtitle,
    required VoidCallback? onTap,
    required VoidCallback dismiss,
    bool warning = false,
  }) => Card(
    color: warning
        ? Theme.of(context).colorScheme.errorContainer
        : Theme.of(context).colorScheme.secondaryContainer,
    child: ListTile(
      leading: Icon(warning ? Icons.portable_wifi_off : Icons.system_update),
      title: Text(title),
      subtitle: Text(subtitle),
      onTap: onTap,
      trailing: IconButton(
        tooltip: 'Not for this version',
        icon: const Icon(Icons.close),
        onPressed: dismiss,
      ),
    ),
  );

  /// Which node the phone is on, a guest's between errands too.
  Widget _connection(BuildContext context) => ListTile(
    dense: true,
    leading: Icon(
      app.nodeLine != null
          ? Icons.bluetooth_connected
          : Icons.bluetooth_disabled,
    ),
    title: Text(connectionLine(app)),
    trailing: ConnectionHelpButton(app),
    onTap: () => _open(context, SettingsScreen(app)),
  );

  /// A swipe either way archives, with an undo, as the row is gone at
  /// once.
  Widget _row(BuildContext context, Conversation c) => Dismissible(
    key: ValueKey(c.key),
    background: const SwipeBackground(Alignment.centerLeft),
    secondaryBackground: const SwipeBackground(Alignment.centerRight),
    onDismissed: (_) async {
      await app.contacts.archiveConversation(c.key);
      if (!context.mounted) return;
      archivedSay(
        ScaffoldMessenger.of(context),
        app,
        c.key,
        '${c.title}: conversation archived',
      );
    },
    child: ConversationRow(app, c),
  );
}

/// One conversation: who, the last text and, for a sent one, its state.
class ConversationRow extends StatelessWidget {
  final AppModel app;
  final Conversation c;
  final Widget? trailing;
  const ConversationRow(this.app, this.c, {this.trailing, super.key});

  @override
  Widget build(BuildContext context) {
    final last = c.last!;
    return ListTile(
      leading: CircleAvatar(
        child: Icon(c.contact == null ? Icons.person_outline : Icons.person),
      ),
      title: Text(c.title, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        last.mine
            ? 'You: ${last.text}, ${textLabel(app.session, last.outs)}'
            : last.text,
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
      ),
      trailing: trailing ?? Text(when(last.at)),
      onTap: () => Navigator.of(context)
          .push(MaterialPageRoute(builder: (_) => ThreadScreen(app, c.key))),
    );
  }
}
