// The first screen: one row a conversation, newest first, under a bar for
// each newer release of the app or the node's firmware, and a line to
// the archived ones.

import 'package:flutter/material.dart';

import '../app.dart';
import '../app_update.dart';
import 'archived_screen.dart';
import '../conversations.dart';
import '../l10n.dart';
import '../link.dart';
import '../session.dart' show OutState;
import 'card_screen.dart';
import 'common.dart';
import 'contacts_screen.dart';
import 'map_screen.dart';
import 'settings_screen.dart';
import 'thread_screen.dart';
import 'update_screen.dart';

/// The manual, a page of the project's site (`site/manual.html`), in the
/// language spoken: the page itself would choose by the browser's
/// language, which need not be the app's.
String get manualPage => switch (tr.localeName) {
  'sv' => 'https://zond.github.io/bark/sv/manual.html',
  'uk' => 'https://zond.github.io/bark/uk/manual.html',
  _ => 'https://zond.github.io/bark/manual.html',
};

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
      if (mounted) app.links.askForBluetooth(() => whyBluetooth(context));
    });
  }

  Future<void> _manual(BuildContext context) async {
    try {
      await app.notifier.openPage(manualPage);
    } catch (e) {
      if (context.mounted) say(context, tr.conversationsNoBrowser);
    }
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
            tooltip: tr.contacts,
            // A head, as a contact is a person; account_circle would read
            // as the person's own account, which is My card.
            icon: Icon(Icons.person, semanticLabel: tr.contacts),
            onPressed: () => _open(context, ContactsScreen(app)),
          ),
          IconButton(
            tooltip: tr.myCard,
            icon: const Icon(Icons.qr_code),
            onPressed: () => _open(context, CardScreen(app)),
          ),
          PopupMenuButton<Widget Function()>(
            onSelected: (f) => _open(context, f()),
            itemBuilder: (_) => [
              PopupMenuItem(
                value: () => ArchivedScreen(app),
                child: Text(tr.archived),
              ),
              PopupMenuItem(
                value: () => MapScreen(app),
                child: Text(tr.nearbyNodes),
              ),
              PopupMenuItem(
                value: () => SettingsScreen(app),
                child: Text(tr.settings),
              ),
              // No screen of the app's: it opens in the browser.
              PopupMenuItem(
                onTap: () => _manual(context),
                child: Text(tr.manual),
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
          GiftLines(app),
          ..._gettingStarted(context),
          // Once there is a contact, there is someone to lose.
          if (!app.me.backedUp && app.contacts.list.isNotEmpty)
            Card(
              child: ListTile(
                leading: const Icon(Icons.key),
                title: Text(tr.conversationsBackupTitle),
                subtitle: Text(tr.conversationsBackupWhy),
                onTap: () =>
                    _open(context, SettingsScreen(app, toBackup: true)),
              ),
            ),
          _connection(context),
          SendCardLine(app),
          if (all.isEmpty)
            Padding(
              padding: const EdgeInsets.all(16),
              child: Text(tr.conversationsNone),
            ),
          for (final c in list) _row(context, c),
          if (archived > 0)
            ArchivedLine(
              tr.conversationsArchived(archived),
              onTap: () => _open(context, ArchivedScreen(app)),
            ),
        ],
      ),
      floatingActionButton: FloatingActionButton(
        tooltip: tr.conversationsNew,
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
            semanticLabel: done ? tr.startDone : tr.startToDo,
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
            ListTile(title: Text(tr.startTitle)),
            step(
              g.name,
              tr.startName,
              tr.startNameHow,
              () => askMyName(context, app),
            ),
            step(
              g.node,
              tr.startNode,
              tr.startNodeHow,
              () => _open(context, SettingsScreen(app)),
            ),
            step(
              g.contact,
              tr.startContact,
              tr.startContactHow,
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
          title: tr.barHearsNoOne(nodeName(node.id)),
          subtitle: tr.barHearsNoOneWhy,
          onTap: () => _open(context, UpdateScreen(app)),
          dismiss: app.updater.dismissHearsNoOne,
          warning: true,
        ),
      if (a != null)
        _bar(
          context,
          title: tr.barAppOut(a.app),
          subtitle: switch (app.appUpdate.state(a.app)) {
            AppGet.idle => tr.barTapToInstall,
            AppGet.downloading => switch (app.appUpdate.percent) {
              null => tr.barDownloading,
              final p => tr.barDownloadingPercent(p),
            },
            AppGet.ready => tr.barDownloaded,
            AppGet.failed => tr.barDownloadFailed,
          },
          onTap: app.appUpdate.state(a.app) == AppGet.downloading
              ? null
              : () => app.appUpdate.get(a),
          dismiss: () => r.dismissApp(a.app),
        ),
      if (i != null && node != null)
        _bar(
          context,
          title: tr.barFirmwareOut(i.firmware),
          subtitle: tr.barTapToUpdate,
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
        tooltip: tr.barNotThisVersion,
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
        tr.conversationsWasArchived(c.title),
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
      subtitle: Ticking(
        when: last.mine && last.outs.any((o) => o.state == OutState.givenBack),
        (_) => Text(
          last.mine
              ? tr.conversationsYou(
                  last.text,
                  textLabel(app.session, last.outs),
                )
              : last.text,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
        ),
      ),
      trailing: trailing ?? Text(when(last.at)),
      onTap: () => Navigator.of(context)
          .push(MaterialPageRoute(builder: (_) => ThreadScreen(app, c.key))),
    );
  }
}
