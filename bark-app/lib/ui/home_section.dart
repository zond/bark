// Settings' Home node section: the home, the way to make the node the
// phone is on the home or to have none, and what the node keeps of the
// person's mail (spec "Mailbox"; `README.md`, "Making a node the home").

import 'package:flutter/material.dart';

import '../app.dart';
import '../link.dart';
import '../nodes.dart';
import 'common.dart';
import 'settings_parts.dart';

class HomeSection extends StatelessWidget {
  final AppModel app;
  final ConnectAsk connect;
  const HomeSection(this.app, this.connect, {super.key});

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: _home(context),
  );

  List<Widget> _home(BuildContext context) {
    final s = app.session;
    final home = app.me.home;
    final st = s.status;
    // A guest can clear no key, and makes the node its home by pairing
    // first; between its errands only the last one's reading is known.
    final owned = s.connected && !s.guest;
    final guest = app.guestHere?.asGuest;
    final here = owned ? s.address : guest?.address;
    final atHome = owned ? app.atHome : app.guestAtHome;
    final homeable = owned ? canBeHome(st) : guest?.homeable ?? false;
    final full = guest?.room == 0;
    return [
      SettingsHeading('Home node', help: _homeHelp),
      Text(home == null ? 'None: your card is temporary.' : 'At $home.'),
      if (here == null && (owned ? st != null : guest != null))
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            'This node has no place yet, so it cannot be a home.'
            '${owned ? ' Set it under This node.' : ''}',
          ),
        ),
      if (here != null && !atHome && !homeable)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text('A home must stay on power in one place.'),
        ),
      if (app.homeKeyGone)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text(
            'Your home no longer keeps your mail. Make it your home again, '
            'or choose another.',
          ),
        ),
      if (app.holdsOtherKey)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: const Text(
            'This node still keeps your mail from when it was your home.',
          ),
        ),
      if (app.othersHome)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text('This node is someone else\'s home.'),
        ),
      if (full)
        const Padding(padding: EdgeInsets.only(top: 8), child: Text(fullNode)),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
          if ((owned || guest != null) &&
              here != null &&
              (!atHome || app.homeKeyGone))
            FilledButton(
              onPressed: homeable && !app.connecting && (owned || !full)
                  ? () => _setHome(context, here, pair: !owned)
                  : null,
              child: const Text('Make this node my home'),
            ),
          if (home != null)
            OutlinedButton(
              onPressed: () => _setHome(context, null),
              child: const Text('No home node'),
            ),
          if (owned && app.holdsOtherKey)
            OutlinedButton(
              onPressed: () => trySay(context, () async {
                final r = await app.clearKey();
                if (context.mounted) say(context, r);
              }, 'Failed'),
              child: const Text('Stop keeping my mail'),
            ),
        ],
      ),
      SendCardLine(app, padding: EdgeInsets.zero),
    ];
  }

  /// Makes the node at `a` the home, or none; `pair` for a node the phone
  /// uses as a guest, which it pairs with first.
  Future<void> _setHome(
    BuildContext context,
    Address? a, {
    bool pair = false,
  }) async {
    final n = app.contacts.list.length;
    // The card carries the home; the new card goes out only if it
    // changes.
    final changes = a?.toString() != app.me.home?.toString();
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(a == null ? 'No home node?' : 'Make this node my home?'),
        content: Text(
          (pair ? 'First you pair: type the code on its screen. ' : '') +
              (!changes && app.cardUnsent
                  ? 'Your card stays; its last change still waits for Send '
                        'my card.'
                  : !changes
                  ? 'Your card stays the same.'
                  : n == 0
                  ? 'Your card changes.'
                  : n == 1
                  ? 'Your new card goes to your contact.'
                  : 'Your new card goes to your $n contacts.'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(a == null ? 'No home' : 'Make it my home'),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    if (pair) {
      await connect(
        context,
        () async {
          final r = await app.homeHere();
          if (context.mounted) say(context, r);
        },
        pairs: true,
        asGuest: true,
      );
      return;
    }
    await trySay(context, () async {
      final r = await (a == null ? app.noHome() : app.makeHome(a));
      if (context.mounted) say(context, r);
    }, 'Setting the home failed');
  }

  /// What a home is, for whoever asks.
  static const _homeHelp =
      'Your home is a node that stays on power in one place. It keeps texts '
      'to you while your phone is away, and your card carries its address, '
      'so the card stays good wherever you go. The inbox button on the '
      'first screen, Fetch my mail, brings them to the node you are on.\n\n'
      'Without a home your card is temporary: it names the node you use '
      'now, which keeps your mail only while you come back to it.\n\n'
      'To make a node your home, the phone pairs with it.';
}
