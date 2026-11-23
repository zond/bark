// One conversation: its texts in order and a field to write the next;
// an archived one read-only, with a button to bring it back.

import 'package:flutter/material.dart' hide Form;
import 'package:flutter/services.dart';

import '../app.dart';
import '../app_layer.dart';
import '../contacts.dart';
import '../conversations.dart';
import '../emojis.dart';
import '../l10n.dart';
import '../link.dart';
import '../session.dart';
import 'archived_screen.dart';
import 'common.dart';

/// The mark on a received text that came as a first message: only a box
/// proves its sender, so a verified thread may still hold such a text.
String get unverifiedText => tr.threadUnverifiedMark;

/// The height of the reaction chip, which sits half over the bubble's
/// lower edge; its other half is room below the bubble.
const double _chipHeight = 28;

/// A card's margin, Material's default, kept here as the chip is placed
/// against the card's edge.
const double _cardMargin = 4;

/// A position to five decimals, about a metre.
String _at(Position p) =>
    '${p.latDeg.toStringAsFixed(5)}, ${p.lonDeg.toStringAsFixed(5)}';

/// What the app keeps of a sent text that its bubble does not say: the
/// node it went to and where that is, its parts, how it was sealed (spec
/// "Payload", Forms), and each part's message ID.
List<(String, String)> sentDetails(List<Outgoing> outs) {
  final o = outs.first;
  final sealed = switch (formOf(o.payload)) {
    Form.boxed => tr.threadSealedForThem,
    Form.introduction => tr.threadSealedIntroOut,
    _ => null,
  };
  return [
    (tr.threadToNode, nodeLabel(o.dst.id)),
    (tr.threadAt, _at(o.dst.pos)),
    if (o.of > 1) (tr.receivedParts, '${o.of}'),
    if (sealed != null) (tr.threadSealed, sealed),
    (
      tr.threadMessageIds(outs.length),
      outs.map((o) => msgIdHex(o.id)).join(', '),
    ),
  ];
}

/// What the app keeps of a received text that its bubble does not say:
/// when the sender wrote it, by their phone's clock, how many of its
/// parts have come, how it was sealed, and each part's message ID.
List<(String, String)> receivedDetails(Entry e) {
  final r = e.arrived.first;
  final first = r.first;
  final sealed = switch (formOf(r.payload)) {
    Form.boxed => tr.threadSealedForYou,
    Form.introduction => tr.threadSealedIntroIn,
    _ => null,
  };
  return [
    if (first != null) (tr.threadWritten, when(timeOfSent(first).toLocal())),
    if (r.of > 1)
      (tr.threadPartsHere, tr.threadPartsOf(e.arrived.length, r.of)),
    if (sealed != null) (tr.threadSealed, sealed),
    (
      tr.threadMessageIds(e.arrived.length),
      e.arrived.map((r) => msgIdHex(r.id)).join(', '),
    ),
  ];
}

class ThreadScreen extends StatefulWidget {
  final AppModel app;

  /// The other person's identity in hex.
  final String keyHex;
  const ThreadScreen(this.app, this.keyHex, {super.key});

  @override
  State<ThreadScreen> createState() => _ThreadScreenState();
}

class _ThreadScreenState extends State<ThreadScreen> {
  final text = TextEditingController();

  AppModel get app => widget.app;

  @override
  void initState() {
    super.initState();
    app.alerts.opened(widget.keyHex);
  }

  @override
  void dispose() {
    app.alerts.closed(widget.keyHex);
    text.dispose();
    super.dispose();
  }

  /// Sends the text where the contact is while that is fresh; when it
  /// may be stale, with a home, where the person chooses, this once;
  /// without one, there after saying it is old, as nothing else is
  /// (roadmap D132, D162).
  Future<void> _send(Contact to) async {
    if (text.text.isEmpty) return;
    final now = app.now();
    final options = to.towards(now);
    final stale = to.stale(now);
    final ask =
        options.length > 1 ||
        (options.length == 1 && stale != null && to.card.home == null);
    final toward = !ask
        ? options.firstOrNull
        : await showDialog<Toward>(
            context: context,
            builder: (context) => SimpleDialog(
              title: Text(tr.threadSendTo),
              children: [
                if (to.card.home == null && stale != null)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
                    child: Text(tr.threadLastSeenThere(ago(stale))),
                  ),
                for (final t in options)
                  SimpleDialogOption(
                    onPressed: () => Navigator.pop(context, t),
                    child: ListTile(
                      title: Text(_option(to, t)),
                      subtitle: Text(switch (t) {
                        Toward.there => tr.threadThereHow,
                        Toward.home => tr.threadHomeHow,
                      }),
                    ),
                  ),
              ],
            ),
          );
    if (toward == null) return;
    try {
      await app.messenger.sendText(to, text.text, toward: toward);
      text.clear();
    } on BadPayload catch (e) {
      if (mounted) say(context, tr.threadNotSent(said(e)));
    }
  }

  String _option(Contact c, Toward t) => switch (t) {
    Toward.there => tr.threadWhereWas(
      c.name,
      ago(app.now().difference(c.placeAt!)),
    ),
    Toward.home => tr.threadTheirHome,
  };

  /// When they last wrote, and whether from their home.
  String _heard(Contact c) {
    final at = c.heardAt;
    if (at == null) return tr.threadNotWritten;
    final from = c.place;
    final home = c.card.home;
    final since = ago(app.now().difference(at));
    if (home == null || from == null) return tr.threadLastWrote(since);
    return from.toString() == home.toString()
        ? tr.threadLastWroteHome(since)
        : tr.threadLastWroteAway(since);
  }

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    final c = thread(widget.keyHex, app.contacts.list, s.outbox, s.received);
    final contact = c.contact;
    final archived = app.contacts.isArchived(widget.keyHex);
    return Scaffold(
      appBar: AppBar(
        title: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(c.title, overflow: TextOverflow.ellipsis),
            if (contact != null)
              Ticking(
                when: contact.heardAt != null,
                (context) => Text(
                  _heard(contact),
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
          ],
        ),
        actions: [
          FetchButton(app),
          if (!archived)
            PopupMenuButton<void Function()>(
              onSelected: (f) => f(),
              itemBuilder: (_) => [
                if (contact != null) ...[
                  PopupMenuItem(
                    value: () => _name(contact),
                    child: Text(tr.threadRename),
                  ),
                  PopupMenuItem(
                    value: () => app.messenger.sendCard(contact),
                    child: Text(tr.commonSendCard),
                  ),
                  if (app.mailbox.isOwner)
                    PopupMenuItem(
                      value: () async {
                        try {
                          final r = await app.mailbox.shareHome(contact);
                          if (context.mounted) say(context, r);
                        } catch (e) {
                          if (context.mounted) {
                            say(context, tr.commonFailed(said(e)));
                          }
                        }
                      },
                      child: Text(tr.threadShareHome),
                    ),
                ],
                PopupMenuItem(
                  value: () => _archive(
                    app.contacts.archiveConversation(widget.keyHex),
                    tr.conversationsWasArchived(c.title),
                  ),
                  child: Text(tr.threadArchiveConversation),
                ),
                if (contact != null) ...[
                  PopupMenuItem(
                    value: () => _archive(
                      app.contacts.archiveContact(contact),
                      tr.contactArchived(contact.name),
                    ),
                    child: Text(tr.threadArchiveContact),
                  ),
                  PopupMenuItem(
                    value: () async {
                      if (await confirm(
                        context,
                        tr.threadRemoveTitle(contact.name),
                        tr.threadRemoveHow,
                        tr.remove,
                      )) {
                        await app.contacts.remove(contact);
                      }
                    },
                    child: Text(tr.threadRemoveContact),
                  ),
                ],
              ],
            ),
        ],
      ),
      body: SafeArea(
        top: false,
        child: Column(
          children: [
            NoticeCard(app),
            GiftLines(app, of: widget.keyHex),
            if (contact != null && contact.unverified)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
                child: Text(tr.threadNotVerified),
              ),
            Expanded(
              child: ListView(
                reverse: true,
                padding: const EdgeInsets.all(8),
                children: [
                  for (final e in c.entries.reversed)
                    _bubble(
                      context,
                      e,
                      !(contact?.unverified ?? false),
                      archived,
                      archived ? null : contact,
                    ),
                ],
              ),
            ),
            if (archived)
              Padding(
                padding: const EdgeInsets.all(16),
                child: Row(
                  children: [
                    Expanded(child: Text(tr.threadIsArchived)),
                    FilledButton.icon(
                      icon: const Icon(Icons.unarchive),
                      label: Text(tr.archivedUnarchive),
                      onPressed: () => app.contacts.unarchive(widget.keyHex),
                    ),
                  ],
                ),
              )
            else if (contact == null)
              Padding(
                padding: const EdgeInsets.all(16),
                child: Text(tr.threadWasRemoved),
              )
            else if (contact.towards(app.now()).isEmpty)
              // A card made before its phone used any node names none.
              Padding(
                padding: const EdgeInsets.all(16),
                child: Text(tr.threadNoNode(contact.name)),
              )
            else
              _compose(contact),
          ],
        ),
      ),
    );
  });

  /// Archives, and leaves the thread, which the first screen no longer
  /// shows; the undo is said there.
  Future<void> _archive(Future<void> done, String what) async {
    final m = ScaffoldMessenger.of(context);
    await done;
    if (!mounted) return;
    Navigator.of(context).pop();
    archivedSay(m, app, widget.keyHex, what);
  }

  Future<void> _name(Contact c) async {
    final name = await askText(context, tr.name, c.given);
    if (name == null || name.isEmpty) return;
    await app.contacts.put(c.copyWith(name: name));
  }

  /// How many messages the text takes, once it takes more than one.
  String? _parts(Contact to, String t) {
    final int n;
    try {
      n = app.messenger.partsOf(to, t).length;
    } on BadPayload catch (e) {
      return said(e);
    }
    if (n <= 1) return null;
    return n > maxParts
        ? tr.threadTooManyParts(n, maxParts)
        : tr.threadParts(n);
  }

  Widget _compose(Contact to) => Padding(
    padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
    child: Row(
      children: [
        Expanded(
          child: ValueListenableBuilder(
            valueListenable: text,
            builder: (context, v, _) => TextField(
              controller: text,
              minLines: 1,
              maxLines: 5,
              textCapitalization: TextCapitalization.sentences,
              decoration: InputDecoration(
                hintText: tr.threadHint,
                border: const OutlineInputBorder(),
                helperText: _parts(to, v.text),
              ),
            ),
          ),
        ),
        IconButton(
          tooltip: tr.threadSend,
          icon: const Icon(Icons.send),
          onPressed: () => _send(to),
        ),
      ],
    ),
  );

  /// `verified`: the line above the thread does not already say that any
  /// text in it may be anyone's. `archived`: read-only, so nothing in it
  /// sends or drops. `to`: whom a reaction goes to; null when archived or
  /// the contact is removed.
  Widget _bubble(
    BuildContext context,
    Entry e,
    bool verified,
    bool archived,
    Contact? to,
  ) {
    final scheme = Theme.of(context).colorScheme;
    final small = Theme.of(context).textTheme.bodySmall!;
    final mine = e.mine;
    final color = scheme.onSurfaceVariant;
    // A tap anywhere on the bubble says what it shows; the buttons take
    // their own. A long press selects a sent text's words, and on a
    // received one opens the reactions, with a copy.
    void help() => mine
        ? explainMessage(
            context,
            details: sentDetails(e.outs),
            marks: sentStates,
          )
        : explainMessage(
            context,
            details: receivedDetails(e),
            marks: receivedMarks,
          );
    final reaction = mine
        ? e.outs.first.reaction
        : e.arrived.map((r) => r.reaction).nonNulls.firstOrNull;
    return Align(
      alignment: mine ? Alignment.centerRight : Alignment.centerLeft,
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: MediaQuery.of(context).size.width * 0.8,
        ),
        child: Stack(
          children: [
            Padding(
              padding: EdgeInsets.only(
                bottom: reaction == null ? 0 : _chipHeight / 2 - _cardMargin,
              ),
              child: Card(
                margin: const EdgeInsets.all(_cardMargin),
                color: mine
                    ? scheme.primaryContainer
                    : scheme.surfaceContainerHigh,
                clipBehavior: Clip.antiAlias,
                child: InkWell(
                  onTap: help,
                  onLongPress: mine ? null : () => _react(e, to),
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(12, 8, 12, 8),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (mine)
                          SelectableText(e.text, onTap: help)
                        else
                          Text.rich(
                            TextSpan(
                              children: [
                                for (final p in e.pieces)
                                  TextSpan(
                                    text: p.text,
                                    style: p.missing
                                        ? TextStyle(
                                            fontStyle: FontStyle.italic,
                                            color: color,
                                          )
                                        : null,
                                  ),
                              ],
                            ),
                          ),
                        const SizedBox(height: 4),
                        Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Text(when(e.at), style: small),
                            if (mine)
                              ..._sentMarks(e.outs, small, scheme)
                            else
                              ..._receivedMarks(e, verified, small, color),
                          ],
                        ),
                        if (mine && !archived) _actions(e.outs),
                      ],
                    ),
                  ),
                ),
              ),
            ),
            if (reaction != null)
              // On the corner toward the screen's middle.
              PositionedDirectional(
                bottom: 0,
                start: mine ? 12 : null,
                end: mine ? null : 12,
                child: _reaction(reaction, mine, help),
              ),
          ],
        ),
      ),
    );
  }

  /// A reaction on its bubble's lower edge: theirs on a sent text, the
  /// person's on a received one.
  Widget _reaction(String r, bool mine, VoidCallback help) {
    final scheme = Theme.of(context).colorScheme;
    final words = mine
        ? tr.threadTheyReacted(emojiName(r))
        : tr.threadYouReacted(emojiName(r));
    return Tooltip(
      message: words,
      excludeFromSemantics: true,
      child: Semantics(
        label: words,
        excludeSemantics: true,
        child: Material(
          key: const ValueKey('reaction'),
          color: scheme.surfaceContainerLowest,
          elevation: 1,
          shape: StadiumBorder(side: BorderSide(color: scheme.outlineVariant)),
          child: InkWell(
            onTap: help,
            customBorder: const StadiumBorder(),
            child: SizedBox(
              height: _chipHeight,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 6),
                child: Center(
                  widthFactor: 1,
                  child: Text(r, style: const TextStyle(fontSize: 16)),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }

  /// The reactions to a received text and a copy of its words, which a
  /// long press no longer selects. In an archived thread, only the copy.
  Future<void> _react(Entry e, Contact? to) async {
    final now = e.arrived.map((r) => r.reaction).nonNulls.firstOrNull;
    final copy = ListTile(
      leading: const Icon(Icons.copy),
      title: Text(tr.threadCopy),
      onTap: () {
        Clipboard.setData(ClipboardData(text: e.text));
        Navigator.pop(context);
      },
    );
    final picked = await showModalBottomSheet<String>(
      context: context,
      showDragHandle: true,
      isScrollControlled: to != null,
      builder: (context) => SafeArea(
        child: to == null
            ? Column(mainAxisSize: MainAxisSize.min, children: [copy])
            : _ReactionSheet(
                to: to,
                now: now,
                frequent: frequentReactions(app.session.reactionsUsed),
                copy: copy,
              ),
      ),
    );
    if (picked == null || to == null) return;
    try {
      await app.messenger.react(e.arrived.first, picked);
    } on BadPayload catch (e) {
      if (mounted) say(context, tr.threadNotSent(said(e)));
    }
  }

  /// Where a sent text went and how far it got: one part's marks when the
  /// parts agree, else their states in words.
  List<Widget> _sentMarks(
    List<Outgoing> outs,
    TextStyle small,
    ColorScheme scheme,
  ) {
    final o = outs.first;
    final color = scheme.onSurfaceVariant;
    return [
      if (o.toward != null) _where(o.toward!, small, color),
      if (!partsAgree(outs)) ...[
        const SizedBox(width: 4),
        Flexible(
          child: Ticking(
            when: outs.any((o) => o.state == OutState.givenBack),
            (_) => Text(partsLabel(app.session, outs), style: small),
          ),
        ),
      ] else
        ..._state(o, small, color),
    ];
  }

  /// Whether a received text came as a first message, where the thread
  /// is verified, and whether it came from the mailbox.
  List<Widget> _receivedMarks(
    Entry e,
    bool verified,
    TextStyle small,
    Color color,
  ) => [
    if (verified && e.arrived.any((r) => r.intro))
      _mark(Icons.gpp_maybe, unverifiedText, small, color),
    if (e.arrived.any((r) => r.mailbox))
      _mark(Icons.markunread_mailbox, tr.receivedMailbox, small, color),
  ];

  /// An icon the small text's size, with the words it stands for as its
  /// tooltip and semantic label, so nothing is lost to someone who does
  /// not know the icon.
  Widget _mark(IconData icon, String words, TextStyle small, Color color) =>
      Padding(
        padding: const EdgeInsets.only(left: 4),
        child: Tooltip(
          message: words,
          excludeFromSemantics: true,
          child: Icon(
            icon,
            size: small.fontSize,
            color: color,
            semanticLabel: words,
          ),
        ),
      );

  Widget _where(Toward t, TextStyle small, Color color) => switch (t) {
    Toward.there => _mark(Icons.place, tr.threadToWhere, small, color),
    Toward.home => _mark(Icons.home, tr.threadToHome, small, color),
  };

  /// A sent text's state, each its own icon (common.dart
  /// [sentStates]). Not yet with the node shows a clock rather than
  /// nothing, so a missing mark never means two things. A state that is a
  /// problem keeps its words, which say why.
  List<Widget> _state(Outgoing o, TextStyle small, Color color) =>
      switch (o.state) {
        OutState.queued => [
          _stateMark([o], Icons.schedule, capital(tr.sentQueued), small, color),
        ],
        OutState.writing => [
          _stateMark([o], Icons.sync, capital(tr.sentWriting), small, color),
        ],
        OutState.taken => [
          _stateMark([o], Icons.check, tr.threadTakenMark, small, color),
        ],
        OutState.givenBack => [
          _stateMark([o], Icons.hourglass_bottom, tr.sentBusy, small, color),
          const SizedBox(width: 4),
          Flexible(
            child: Ticking(
              (_) => Text(capital(stateLabel(app.session, o)), style: small),
            ),
          ),
        ],
        OutState.refused => [
          const SizedBox(width: 4),
          Flexible(
            child: Text(capital(stateLabel(app.session, o)), style: small),
          ),
        ],
      };

  /// A sent text's state mark, which a tap explains with the others,
  /// under the details of `outs`, the text's parts.
  Widget _stateMark(
    List<Outgoing> outs,
    IconData icon,
    String words,
    TextStyle small,
    Color color,
  ) => InkWell(
    onTap: () =>
        explainMessage(context, details: sentDetails(outs), marks: sentStates),
    child: _mark(icon, words, small, color),
  );

  /// Cancel, for the parts not yet with the node; a part given back goes
  /// again by itself once its wait is over.
  Widget _actions(List<Outgoing> outs) {
    final s = app.session;
    final droppable = [
      for (final o in outs)
        if (o.state != OutState.writing && o.state != OutState.taken) o,
    ];
    if (droppable.isEmpty) return const SizedBox.shrink();
    return TextButton(
      onPressed: () async {
        for (final o in droppable) {
          await s.drop(o);
        }
      },
      child: Text(tr.cancel),
    );
  }
}

/// The reaction picker: the emoji the person uses most in one row, a copy
/// of the text, then every emoji by Unicode's groups, with a search by
/// name. Pops with the emoji picked.
class _ReactionSheet extends StatefulWidget {
  final Contact to;

  /// The person's reaction to the text so far.
  final String? now;
  final List<String> frequent;
  final Widget copy;
  const _ReactionSheet({
    required this.to,
    required this.now,
    required this.frequent,
    required this.copy,
  });

  @override
  State<_ReactionSheet> createState() => _ReactionSheetState();
}

class _ReactionSheetState extends State<_ReactionSheet> {
  final search = TextEditingController();

  @override
  void dispose() {
    search.dispose();
    super.dispose();
  }

  Widget _button(String emoji, {double size = 24}) {
    final scheme = Theme.of(context).colorScheme;
    return IconButton(
      tooltip: emojiName(emoji),
      isSelected: emoji == widget.now,
      style: IconButton.styleFrom(
        backgroundColor: emoji == widget.now ? scheme.secondaryContainer : null,
      ),
      onPressed: () => Navigator.pop(context, emoji),
      icon: Text(emoji, style: TextStyle(fontSize: size)),
    );
  }

  SliverGrid _grid(List<String> emoji) => SliverGrid.builder(
    gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
      maxCrossAxisExtent: 48,
    ),
    itemCount: emoji.length,
    itemBuilder: (_, i) => _button(emoji[i]),
  );

  @override
  Widget build(BuildContext context) {
    final style = Theme.of(context).textTheme.titleSmall;
    final query = search.text.trim();
    final found = query.isEmpty ? null : searchEmoji(query);
    return SizedBox(
      height: MediaQuery.of(context).size.height * 0.75,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Text(tr.threadReactionSeen(widget.to.name)),
          ),
          Row(
            key: const ValueKey('frequent'),
            mainAxisAlignment: MainAxisAlignment.spaceEvenly,
            children: [for (final e in widget.frequent) _button(e, size: 26)],
          ),
          widget.copy,
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16),
            child: TextField(
              key: const ValueKey('emoji search'),
              controller: search,
              onChanged: (_) => setState(() {}),
              decoration: InputDecoration(
                prefixIcon: const Icon(Icons.search),
                hintText: tr.emojiSearch,
              ),
            ),
          ),
          Expanded(
            child: CustomScrollView(
              slivers: [
                if (found == null)
                  for (final (g, group) in emojiGroups) ...[
                    SliverPadding(
                      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                      sliver: SliverToBoxAdapter(
                        child: Text(emojiGroupName(g), style: style),
                      ),
                    ),
                    _grid([for (final e in group) e.emoji]),
                  ]
                else if (found.isEmpty)
                  SliverPadding(
                    padding: const EdgeInsets.all(16),
                    sliver: SliverToBoxAdapter(child: Text(tr.emojiNone)),
                  )
                else
                  _grid([for (final e in found) e.emoji]),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
