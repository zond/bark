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
    // The first screen is under every other, so it asks over them.
    app.askOwner = (n) => askIsYours(context, n);
    // Without Bluetooth there is no node, so the app asks at once.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) app.askForBluetooth(() => whyBluetooth(context));
    });
  }

  @override
  void dispose() {
    app.askOwner = null;
    super.dispose();
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
          if (app.fake)
            Card(
              color: Theme.of(context).colorScheme.tertiaryContainer,
              child: const ListTile(
                leading: Icon(Icons.science_outlined),
                title: Text('Demo: nothing leaves this phone'),
              ),
            ),
          NoticeCard(app),
          ..._gettingStarted(context),
          // Once there is a contact, there is someone to lose.
          if (!app.me.backedUp && app.contacts.list.isNotEmpty)
            Card(
              child: ListTile(
                leading: const Icon(Icons.key),
                title: const Text('Write down your 12 backup words'),
                subtitle: const Text(
                  'If you lose this phone, these words are the only way to '
                  'keep your contacts able to reach you.',
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
                'No conversations yet. Add a contact, or show your card '
                'to someone who has the app.',
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
              'On your card, so that people know who you are.',
              () => askMyName(context, app),
            ),
            step(
              g.node,
              'A node',
              'Found by itself when one is near, or pair your own in '
                  'Settings.',
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
          subtitle: app.updater.hearsNoOneWarning(updated),
          onTap: () => _open(context, UpdateScreen(app)),
          dismiss: app.updater.dismissHearsNoOne,
          warning: true,
        ),
      if (a != null)
        _bar(
          context,
          title: 'A newer app, version ${a.n}, is available',
          subtitle:
              'Tap to download it in the browser; Android asks before it '
              'installs it over this one.',
          onTap: () => app.notifier.openLink(a.apk!),
          dismiss: () => r.dismissApp(a.n),
        ),
      if (i != null && node != null)
        _bar(
          context,
          title:
              'Your node runs version ${node.image}; version ${i.n} is '
              'available',
          subtitle: node.version < updateVersion
              ? 'Its firmware is from before updates over Bluetooth: '
                    'update it by USB once.'
              : 'Tap to update it from this phone.',
          onTap: node.version < updateVersion
              ? null
              : () => _open(context, UpdateScreen(app, release: i)),
          dismiss: () => r.dismissImage(node, i.n),
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

  Widget _connection(BuildContext context) {
    final s = app.session;
    final looking = lookingLine(app);
    final text = s.connected && s.guest
        ? 'Connected to ${nodeLabel(s.node!.id)} as a guest: a public node, '
              'not yours'
        : s.connected
        ? 'Connected to ${nodeLabel(s.node!.id)}, paired'
        : looking != null
        ? '$looking Texts wait on the phone until it is in reach.'
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
