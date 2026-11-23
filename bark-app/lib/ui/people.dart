// The node's records as a person sees them, a row each: the phones paired
// with it and the people whose home it is; and the changes to them, at the
// node by the code on its screen, from away by the owner's command (spec
// "Phone link", Phones, The node's owner; "Mailbox", The owner's
// commands).

import 'package:flutter/material.dart';

import '../app.dart';
import '../l10n.dart';
import '../link.dart';
import '../node_link.dart';
import 'common.dart';
import 'settings_parts.dart';

/// What a person can do to a record.
enum RecordAction { remove, makeOwner }

String actionLabel(RecordAction a) => switch (a) {
  RecordAction.remove => tr.remove,
  RecordAction.makeOwner => tr.peopleMakeOwner,
};

/// A record's row: in the paired list, or with `home` in the homes list;
/// `atNode` for the records of the node the phone is paired with and on,
/// else those of the owner's list of their home. `ownAction`, on the
/// person's own paired row, is the one action there.
class RecordRow extends StatelessWidget {
  final AppModel app;
  final PhoneRecord r;
  final bool home;
  final bool atNode;
  final Widget? ownAction;
  const RecordRow(
    this.app,
    this.r, {
    required this.home,
    required this.atNode,
    this.ownAction,
    super.key,
  });

  @override
  Widget build(BuildContext context) {
    final both = r.paired && r.home;
    final words = [
      if (r.mine) atNode ? tr.peopleThisPhone : tr.peopleYou,
      if (r.owner) tr.peopleOwner,
      if (both && atNode) home ? tr.peoplePairedToo : tr.peopleHomeToo,
      if (r.linked)
        tr.peopleLinked
      else if (dayOf(r.day) == null)
        tr.peopleNeverSeen
      else
        tr.peopleSeen(lastSeen(r.day, app.now())),
    ];
    final acts = actionsOn(app, r, home: home, atNode: atNode);
    final Widget? trailing = r.mine && !home
        ? ownAction
        : switch (acts) {
            [] => null,
            [final a] => TextButton(
              onPressed: () => act(context, app, r, a, atNode: atNode),
              child: Text(actionLabel(a)),
            ),
            _ => PopupMenuButton<RecordAction>(
              tooltip: tr.peopleChoices,
              onSelected: (a) => act(context, app, r, a, atNode: atNode),
              itemBuilder: (_) => [
                for (final a in acts)
                  PopupMenuItem(value: a, child: Text(actionLabel(a))),
              ],
            ),
          };
    return ListTile(
      contentPadding: EdgeInsets.zero,
      leading: Icon(home ? Icons.home_outlined : Icons.phone_android),
      title: Text(r.name.isEmpty ? tr.peopleNoName : r.name),
      subtitle: Text(capital(words.join(', '))),
      trailing: trailing,
    );
  }
}

/// What the person may do to `r`, in the paired list or with `home` in the
/// homes list. At the node any paired phone removes another's record and
/// moves the owner's mark by the code on its screen; from away the owner
/// alone removes another's home and hands the mark on.
List<RecordAction> actionsOn(
  AppModel app,
  PhoneRecord r, {
  required bool home,
  required bool atNode,
}) {
  if (!atNode && !app.mailbox.isOwner) return const [];
  return [
    if (!r.mine && (home || atNode)) RecordAction.remove,
    if (home && !r.owner && (atNode || !r.mine)) RecordAction.makeOwner,
  ];
}

/// Does `a` to `r`, after a dialog that says what follows: at the node by
/// the code its screen then shows, from away by a command to the home.
Future<void> act(
  BuildContext context,
  AppModel app,
  PhoneRecord r,
  RecordAction a, {
  required bool atNode,
}) async {
  final name = r.name, named = name.isNotEmpty;
  final String title;
  final List<String> body;
  switch (a) {
    case RecordAction.remove when atNode:
      title = r.paired && r.home
          ? (named
                ? tr.peopleRemovePhoneAndHome(name)
                : tr.peopleRemoveUnnamedAndHome)
          : r.paired
          ? (named ? tr.peopleRemovePhone(name) : tr.peopleRemoveUnnamed)
          : tr.peopleRemoveThisHome;
      body = [
        if (r.paired) named ? tr.peoplePhoneGoes(name) : tr.peopleUnnamedGoes,
        if (r.home) tr.peopleNodeStopsMail,
      ];
    case RecordAction.remove:
      title = named ? tr.peopleRemoveHome(name) : tr.peopleRemoveTheirHome;
      body = [tr.peopleHomeStopsMail];
    case RecordAction.makeOwner:
      title = named ? tr.peopleMakeOwnerTitle(name) : tr.peopleMakeThemOwner;
      body = [
        !r.mine && app.mailbox.isOwner && !atNode
            ? tr.peopleOwnerChoosesNotYou
            : tr.peopleOwnerChooses,
      ];
  }
  final label = actionLabel(a);
  final (ok, gone) = await _whileListed<bool>(
    context,
    app,
    r,
    atNode,
    (context) => AlertDialog(
      title: Text(title),
      content: Text([...body, if (atNode) tr.peopleTypeCodeNext].join(' ')),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: Text(tr.cancel),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, true),
          child: Text(atNode ? tr.peopleShowCode : label),
        ),
      ],
    ),
  );
  if (!context.mounted) return;
  if (gone) {
    say(context, tr.peopleGone);
    return;
  }
  if (ok != true) return;
  if (!atNode) {
    await trySay(context, () async {
      final s = await (a == RecordAction.remove
          ? app.mailbox.removeHome(r)
          : app.mailbox.handOver(r));
      if (context.mounted) say(context, s);
    }, tr.sendFailed);
    return;
  }
  await _byCode(
    context,
    app,
    r,
    a == RecordAction.remove ? RecordChange.removal(r) : RecordChange.owner,
    label,
  );
}

/// Whether the list, as last read, still holds `r` with what a change
/// takes: the node's at it, else the owner's list; with no list, as once
/// the link ended, it is not known to have gone.
bool _listed(AppModel app, PhoneRecord r, bool atNode) =>
    (atNode ? app.session.phones : app.mailbox.listed)?.any(
      (x) =>
          x.number == r.number &&
          (x.paired || !r.paired) &&
          (x.home || !r.home),
    ) ??
    true;

/// Shows a dialog about `r`, which closes by itself once the list, read
/// again as it changes, no longer holds `r`: another phone removed it
/// first. Its answer, and whether `r` went.
Future<(T?, bool)> _whileListed<T>(
  BuildContext context,
  AppModel app,
  PhoneRecord r,
  bool atNode,
  WidgetBuilder builder,
) async {
  BuildContext? shown;
  var gone = false;
  void check() {
    if (gone || _listed(app, r, atNode)) return;
    gone = true;
    final c = shown;
    if (c != null && c.mounted) Navigator.pop(c);
  }

  app.mailbox.addListener(check);
  app.session.addListener(check);
  try {
    final v = await showDialog<T>(
      context: context,
      builder: (c) {
        shown = c;
        return builder(c);
      },
    );
    return (v, gone);
  } finally {
    app.mailbox.removeListener(check);
    app.session.removeListener(check);
  }
}

/// Asks the node for a code, takes the six digits, and writes `change`; a
/// wrong or late code offers a new one, as each code takes one try.
Future<void> _byCode(
  BuildContext context,
  AppModel app,
  PhoneRecord r,
  RecordChange change,
  String label,
) async {
  final s = app.session;
  final owner = change == RecordChange.owner;
  while (context.mounted) {
    try {
      await s.askCode();
    } catch (e) {
      if (context.mounted) {
        say(context, tr.peopleCodeFailed(said(e)));
      }
      return;
    }
    if (!context.mounted) return;
    final (code, gone) = await _whileListed<String>(
      context,
      app,
      r,
      true,
      (context) => _CodeDialog(label),
    );
    if (!context.mounted) return;
    if (gone) {
      say(context, tr.peopleGone);
      return;
    }
    if (code == null) return;
    try {
      await s.byCode(r, change, int.parse(code));
      if (context.mounted) {
        say(
          context,
          !owner
              ? tr.peopleRemoved
              : r.name.isEmpty
              ? tr.peopleThemOwner
              : tr.peopleIsOwner(r.name),
        );
      }
      return;
    } on AttException catch (e) {
      if (!context.mounted) return;
      if (e.code != AttError.refused) {
        say(context, tr.peopleGone);
        return;
      }
      final again = await confirm(
        context,
        owner ? tr.peopleNotChanged : tr.peopleNotRemoved,
        tr.peopleWrongCode,
        tr.peopleNewCode,
      );
      if (!again) return;
    } catch (e) {
      if (context.mounted) say(context, tr.peopleItFailed(said(e)));
      return;
    }
  }
}

/// Takes the six digits the node's screen shows.
class _CodeDialog extends StatefulWidget {
  final String action;
  const _CodeDialog(this.action);

  @override
  State<_CodeDialog> createState() => _CodeDialogState();
}

class _CodeDialogState extends State<_CodeDialog> {
  final _code = TextEditingController();

  @override
  void dispose() {
    _code.dispose();
    super.dispose();
  }

  bool get _whole => RegExp(r'^\d{6}$').hasMatch(_code.text);

  @override
  Widget build(BuildContext context) => AlertDialog(
    // Its field stays in reach above the keyboard on a phone held sideways.
    scrollable: true,
    title: Text(tr.peopleCodeTitle),
    content: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(tr.peopleCodeHow('CONFIRM')),
        TextField(
          key: const Key('node code'),
          controller: _code,
          autofocus: true,
          keyboardType: TextInputType.number,
          maxLength: 6,
          decoration: InputDecoration(labelText: tr.peopleCode),
          onChanged: (_) => setState(() {}),
        ),
      ],
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: Text(tr.cancel),
      ),
      FilledButton(
        onPressed: _whole ? () => Navigator.pop(context, _code.text) : null,
        child: Text(widget.action),
      ),
    ],
  );
}
