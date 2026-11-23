// Settings' Home node section: the home, the way to make the node the
// phone is on the home or to have none, and what the node keeps of the
// person's mail (spec "Mailbox"; `README.md`, "Making a node the home").

import 'package:flutter/material.dart';

import '../app.dart';
import '../card.dart';
import '../l10n.dart';
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
    final guest = app.links.guestHere?.asGuest;
    final here = paired ? s.address : guest?.address;
    final atHome = paired ? app.mailbox.atHome : app.mailbox.guestAtHome;
    final homeable = paired ? canBeHome(st) : guest?.homeable ?? false;
    final full = guest?.room == 0;
    return [
      SettingsHeading(tr.homeNode, help: tr.homeHelp),
      Text(home == null ? tr.homeNone : tr.homeAt('$home')),
      if (here == null && (paired ? st != null : guest != null))
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(paired ? tr.homeNoPlaceSetIt : tr.homeNoPlace),
        ),
      if (here != null && !atHome && !homeable)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(tr.homeNeedsPower),
        ),
      if (app.mailbox.homeKeyGone)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(tr.homeKeyGone),
        ),
      if (app.mailbox.homeSilent)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(tr.homeSilent),
        ),
      if (app.mailbox.holdsOtherKey)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(tr.homeHoldsOtherKey),
        ),
      if (app.mailbox.othersHome)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(tr.homeOthers),
        ),
      if (full)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(tr.fullNode),
        ),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
          if ((paired || guest != null) &&
              here != null &&
              (!atHome || app.mailbox.homeKeyGone))
            FilledButton(
              onPressed: homeable && !app.links.connecting && (paired || !full)
                  ? () => _setHome(context, here, pair: !paired)
                  : null,
              child: Text(tr.setHome),
            ),
          if (home != null)
            OutlinedButton(
              onPressed: () => _setHome(context, null),
              child: Text(tr.homeRemove),
            ),
          if (paired && app.mailbox.holdsOtherKey)
            OutlinedButton(
              onPressed: () => trySay(context, () async {
                final r = await app.mailbox.clearKey();
                if (context.mounted) say(context, r);
              }, tr.failed),
              child: Text(tr.homeForgetMe),
            ),
          if (home == null && app.mailbox.tried?.over != false)
            OutlinedButton(
              onPressed: () => _tryHome(context),
              child: Text(tr.homeUseExisting),
            ),
        ],
      ),
      if (app.mailbox.tried case final t?)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            t.noWay
                ? tr.homeNoWay(nodeLabel(t.home.id))
                : t.over
                ? tr.homeNoAnswer(nodeLabel(t.home.id))
                : tr.homeAsking(nodeLabel(t.home.id)),
          ),
        ),
      if (app.me.leaving case final l?)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(tr.homeLeaving(nodeLabel(l.home.id))),
        ),
      SendCardLine(app, padding: EdgeInsets.zero),
      ..._people(context, atHome: atHome && paired),
    ];
  }

  /// The owner's list of the people with a home there, away from it; at
  /// the home, This node lists them as the node's phones.
  List<Widget> _people(BuildContext context, {required bool atHome}) {
    if (app.me.home == null || !app.mailbox.isOwner || atHome) return [];
    final rs = app.mailbox.listed?.where((r) => r.home).toList();
    return [
      Padding(
        padding: const EdgeInsets.only(top: 16),
        child: Row(
          children: [
            Flexible(
              child: Text(
                tr.homePeople,
                style: Theme.of(context).textTheme.titleSmall,
              ),
            ),
            HelpButton(tr.homePeople, tr.homePeopleHelp),
          ],
        ),
      ),
      Text(tr.homeYouOwner),
      for (final r in rs ?? const <PhoneRecord>[])
        RecordRow(app, r, home: true, atNode: false),
      Align(
        alignment: Alignment.centerLeft,
        child: OutlinedButton(
          onPressed: () => trySay(context, () async {
            final r = await app.mailbox.askList();
            if (context.mounted) say(context, r);
          }, tr.homeAskFailed),
          child: Text(rs == null ? tr.homeShowThem : tr.askAgain),
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
      final r = await app.mailbox.tryHome(a);
      if (context.mounted) say(context, r);
    }, tr.homeAskFailed);
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
        title: Text(a == null ? tr.homeRemoveTitle : tr.homeSetTitle),
        // Whole sentences, joined where one ends.
        content: Text(
          [
            if (pair) tr.homePairFirst,
            !changes && app.cardUnsent
                ? tr.homeCardWaits
                : !changes
                ? tr.homeCardSame
                : n == 0
                ? tr.homeCardChanges
                : tr.homeCardGoes(n),
          ].join(' '),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(tr.cancel),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(a == null ? tr.remove : tr.homeSet),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    if (pair) {
      await connect(
        context,
        () async {
          final r = await app.mailbox.homeHere();
          if (context.mounted) say(context, r);
        },
        pairs: true,
        asGuest: true,
      );
      return;
    }
    await trySay(context, () async {
      final r = await (a == null
          ? app.mailbox.noHome()
          : app.mailbox.makeHome(a));
      if (context.mounted) say(context, r);
    }, tr.homeSetFailed);
  }
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
      if (_home == null) _error = tr.homeCardNoHome;
    } on BadCard {
      _error = tr.homeNotAddress;
    }
  });

  Future<void> _scan() async {
    final c = await scanCard(context);
    if (c == null || !mounted) return;
    setState(() {
      _text.text = c.toText();
      _home = c.home;
      _error = c.home == null ? tr.homeCardNoHome : null;
    });
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    // Its field stays in reach above the keyboard on a phone held sideways.
    scrollable: true,
    title: Text(tr.homeAddressTitle),
    content: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(tr.homeAddressHow),
        TextField(
          key: const Key('home address'),
          controller: _text,
          maxLines: null,
          onChanged: _read,
          decoration: InputDecoration(
            labelText: tr.homeAddressLabel,
            errorText: _error,
            helperText: _home == null ? null : nodeLabel(_home!.id),
          ),
        ),
        const SizedBox(height: 8),
        OutlinedButton.icon(
          icon: const Icon(Icons.qr_code_scanner),
          label: Text(tr.homeScanCard),
          onPressed: _scan,
        ),
      ],
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: Text(tr.cancel),
      ),
      FilledButton(
        onPressed: _home == null ? null : () => Navigator.pop(context, _home),
        child: Text(tr.homeAskIt),
      ),
    ],
  );
}
