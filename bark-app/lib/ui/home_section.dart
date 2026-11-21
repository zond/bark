// Settings' Home node section: the home, the way to make the node the
// phone is on the home or to have none, and what the node keeps of the
// person's mail (spec "Mailbox"; `README.md`, "Making a node the home").

import 'package:flutter/material.dart';

import '../app.dart';
import '../card.dart';
import '../link.dart';
import '../nodes.dart';
import 'common.dart';
import 'people.dart';
import 'scan_screen.dart';
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
    final paired = s.connected && !s.guest;
    final guest = app.guestHere?.asGuest;
    final here = paired ? s.address : guest?.address;
    final atHome = paired ? app.atHome : app.guestAtHome;
    final homeable = paired ? canBeHome(st) : guest?.homeable ?? false;
    final full = guest?.room == 0;
    return [
      SettingsHeading('Home node', help: _homeHelp),
      Text(home == null ? 'None: your card is temporary.' : 'At $home.'),
      if (here == null && (paired ? st != null : guest != null))
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            'This node has no place yet, so it cannot be a home.'
            '${paired ? ' Set it under This node.' : ''}',
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
      if (app.homeSilent)
        const Padding(
          padding: EdgeInsets.only(top: 8),
          child: Text('Your home did not answer your last fetch.'),
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
          if ((paired || guest != null) &&
              here != null &&
              (!atHome || app.homeKeyGone))
            FilledButton(
              onPressed: homeable && !app.connecting && (paired || !full)
                  ? () => _setHome(context, here, pair: !paired)
                  : null,
              child: const Text('Make this node my home'),
            ),
          if (home != null)
            OutlinedButton(
              onPressed: () => _setHome(context, null),
              child: const Text('No home node'),
            ),
          if (paired && app.holdsOtherKey)
            OutlinedButton(
              onPressed: () => trySay(context, () async {
                final r = await app.clearKey();
                if (context.mounted) say(context, r);
              }, 'Failed'),
              child: const Text('Stop keeping my mail'),
            ),
          if (home == null && app.tried?.over != false)
            OutlinedButton(
              onPressed: () => _tryHome(context),
              child: const Text('Use my existing home'),
            ),
        ],
      ),
      if (app.tried case final t?)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            t.over
                ? '${nodeLabel(t.home.id)} did not answer. Nothing changed.'
                : 'Asking ${nodeLabel(t.home.id)} for your mail…',
          ),
        ),
      if (app.me.leaving case final l?)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            'Leaving ${nodeLabel(l.home.id)}: it sends you its mail, then '
            'forgets you.',
          ),
        ),
      SendCardLine(app, padding: EdgeInsets.zero),
      ..._people(context, atHome: atHome && paired),
    ];
  }

  /// The owner's list of the people with a home there, away from it; at
  /// the home, This node lists them as the node's phones.
  List<Widget> _people(BuildContext context, {required bool atHome}) {
    if (app.me.home == null || !app.isOwner || atHome) return [];
    final rs = app.listed?.where((r) => r.home).toList();
    return [
      Padding(
        padding: const EdgeInsets.only(top: 16),
        child: Row(
          children: [
            Flexible(
              child: Text(
                'People with a home here',
                style: Theme.of(context).textTheme.titleSmall,
              ),
            ),
            const HelpButton('People with a home here', _peopleHelp),
          ],
        ),
      ),
      const Text('You are its owner.'),
      for (final r in rs ?? const <PhoneRecord>[])
        RecordRow(app, r, home: true, atNode: false),
      Align(
        alignment: Alignment.centerLeft,
        child: OutlinedButton(
          onPressed: () => trySay(context, () async {
            final r = await app.askList();
            if (context.mounted) say(context, r);
          }, 'Asking failed'),
          child: Text(rs == null ? 'Show them' : 'Ask again'),
        ),
      ),
    ];
  }

  /// Asks for the home's address, typed or from a card, and asks it.
  Future<void> _tryHome(BuildContext context) async {
    final a = await showDialog<Address>(
      context: context,
      builder: (_) => const _HomeAddressDialog(),
    );
    if (a == null || !context.mounted) return;
    await trySay(context, () async {
      final r = await app.tryHome(a);
      if (context.mounted) say(context, r);
    }, 'Asking failed');
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
      'To make a node your home, the phone pairs with it. A friend who owns '
      'a home node can also give you a home there from afar.\n\n'
      'When you choose another home, or none, your old home sends you the '
      'mail it keeps, then forgets you.\n\n'
      'A home that stops answering may be off or out of reach, or may have '
      'lost your place there. Fetch again later; if it still does not '
      'answer, make another node your home, or choose No home node.\n\n'
      'Use my existing home is for a phone that got your identity back from '
      'your 12 words: give your home\'s address, and once your home answers '
      'it is your home again.';

  static const _peopleHelp =
      'As the owner you choose who has a home here, from anywhere: give a '
      'contact a home here from their conversation\'s menu, remove a '
      'person\'s home, or make someone else the owner.\n\n'
      'Each goes to your home with a fetch of your mail, and its answer '
      'comes back with it.';
}

/// Takes the home's address: its 26 hex digits, or a card of the person's
/// that names it, pasted or scanned.
class _HomeAddressDialog extends StatefulWidget {
  const _HomeAddressDialog();

  @override
  State<_HomeAddressDialog> createState() => _HomeAddressDialogState();
}

class _HomeAddressDialogState extends State<_HomeAddressDialog> {
  final _text = TextEditingController();
  Address? _home;
  String? _error;

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  void _read(String s) => setState(() {
    final t = s.trim();
    _home = Address.parse(t);
    _error = null;
    if (_home != null || t.isEmpty) return;
    try {
      _home = PersonCard.parse(t).home;
      if (_home == null) _error = 'That card names no home';
    } on BadCard {
      _error = 'Not an address or a card';
    }
  });

  Future<void> _scan() async {
    final c = await scanCard(context);
    if (c == null || !mounted) return;
    setState(() {
      _text.text = c.toText();
      _home = c.home;
      _error = c.home == null ? 'That card names no home' : null;
    });
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    // Its field stays in reach above the keyboard on a phone held sideways.
    scrollable: true,
    title: const Text('Your home\'s address'),
    content: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text('Its 26 hex digits, or a card of yours that names it.'),
        TextField(
          key: const Key('home address'),
          controller: _text,
          maxLines: null,
          onChanged: _read,
          decoration: InputDecoration(
            labelText: 'Address or card',
            errorText: _error,
            helperText: _home == null ? null : nodeLabel(_home!.id),
          ),
        ),
        const SizedBox(height: 8),
        OutlinedButton.icon(
          icon: const Icon(Icons.qr_code_scanner),
          label: const Text('Scan a card'),
          onPressed: _scan,
        ),
      ],
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      FilledButton(
        onPressed: _home == null ? null : () => Navigator.pop(context, _home),
        child: const Text('Ask it'),
      ),
    ],
  );
}
