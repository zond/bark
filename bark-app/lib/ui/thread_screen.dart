// One conversation: its texts in order and a field to write the next;
// an archived one read-only, with a button to bring it back.

import 'package:flutter/material.dart' hide Form;
import 'package:flutter/services.dart';

import '../app.dart';
import '../app_layer.dart';
import '../contacts.dart';
import '../conversations.dart';
import '../emojis.dart';
import '../link.dart';
import '../session.dart';
import 'archived_screen.dart';
import 'common.dart';

/// The mark on a received text that came as a first message: only a box
/// proves its sender, so a verified thread may still hold such a text.
const unverifiedText = 'Came as a first message, which anyone could have made';

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
    Form.boxed => 'for them alone',
    Form.introduction => 'as a first message, with your card',
    _ => null,
  };
  return [
    ('To node', nodeLabel(o.dst.id)),
    ('At', _at(o.dst.pos)),
    if (o.of > 1) ('Parts', '${o.of}'),
    if (sealed != null) ('Sealed', sealed),
    (
      outs.length > 1 ? 'Message IDs' : 'Message ID',
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
    Form.boxed => 'by them, for you alone',
    Form.introduction => 'to you, as a first message',
    _ => null,
  };
  return [
    if (first != null) ('Written', when(timeOfSent(first).toLocal())),
    if (r.of > 1) ('Parts here', '${e.arrived.length} of ${r.of}'),
    if (sealed != null) ('Sealed', sealed),
    (
      e.arrived.length > 1 ? 'Message IDs' : 'Message ID',
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
              title: const Text('Send to'),
              children: [
                if (to.card.home == null && stale != null)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
                    child: Text(
                      'Last seen there ${ago(stale)}: it keeps their mail '
                      'only while they come back',
                    ),
                  ),
                for (final t in options)
                  SimpleDialogOption(
                    onPressed: () => Navigator.pop(context, t),
                    child: ListTile(
                      title: Text(_option(to, t)),
                      subtitle: Text(switch (t) {
                        Toward.there => 'Fast if they are still near there',
                        Toward.home =>
                          'Waits at their home until they fetch it',
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
      if (mounted) say(context, 'Not sent: $e');
    }
  }

  String _option(Contact c, Toward t) => switch (t) {
    Toward.there =>
      'Where ${c.name} was ${ago(app.now().difference(c.placeAt!))}',
    Toward.home => 'Their home node',
  };

  /// When they last wrote, and whether from their home.
  String _heard(Contact c) {
    final at = c.heardAt;
    if (at == null) return 'Has not written yet';
    final from = c.place;
    final home = c.card.home;
    final where = home == null || from == null
        ? ''
        : from.toString() == home.toString()
        ? ', from their home'
        : ', away from home';
    return 'Last wrote ${ago(app.now().difference(at))}$where';
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
              Text(
                _heard(contact),
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
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
                    child: const Text('Rename'),
                  ),
                  PopupMenuItem(
                    value: () => app.messenger.sendCard(contact),
                    child: const Text('Send my card'),
                  ),
                ],
                PopupMenuItem(
                  value: () => _archive(
                    app.contacts.archiveConversation(widget.keyHex),
                    '${c.title}: conversation archived',
                  ),
                  child: const Text('Archive conversation'),
                ),
                if (contact != null) ...[
                  PopupMenuItem(
                    value: () => _archive(
                      app.contacts.archiveContact(contact),
                      '${contact.name}: contact archived',
                    ),
                    child: const Text('Archive contact'),
                  ),
                  PopupMenuItem(
                    value: () async {
                      if (await confirm(
                        context,
                        'Remove ${contact.name}?',
                        'Their card goes from this phone. Their texts stay, '
                            'but you cannot write to them until you add '
                            'their card again.',
                        'Remove',
                      )) {
                        await app.contacts.remove(contact);
                      }
                    },
                    child: const Text('Remove contact'),
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
            if (contact != null && contact.unverified)
              const Padding(
                padding: EdgeInsets.fromLTRB(16, 8, 16, 0),
                child: Text(
                  'Not verified yet: a first message can come from anyone. '
                  'Their first reply to you proves it is them.',
                ),
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
                    const Expanded(
                      child: Text('This conversation is archived.'),
                    ),
                    FilledButton.icon(
                      icon: const Icon(Icons.unarchive),
                      label: const Text('Unarchive'),
                      onPressed: () => app.contacts.unarchive(widget.keyHex),
                    ),
                  ],
                ),
              )
            else if (contact == null)
              const Padding(
                padding: EdgeInsets.all(16),
                child: Text('This contact was removed.'),
              )
            else if (contact.towards(app.now()).isEmpty)
              // A card made before its phone used any node names none.
              Padding(
                padding: const EdgeInsets.all(16),
                child: Text(
                  '${contact.name}\'s card names no node, as their phone had '
                  'not used one when they shared it: ask them for a new card '
                  'once it has.',
                ),
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
    final name = await askText(context, 'Name', c.given);
    if (name == null || name.isEmpty) return;
    await app.contacts.put(c.copyWith(name: name));
  }

  /// How many messages the text takes, once it takes more than one.
  String? _parts(Contact to, String t) {
    final int n;
    try {
      n = app.messenger.partsOf(to, t).length;
    } on BadPayload catch (e) {
      return e.why;
    }
    if (n <= 1) return null;
    return n > maxParts ? '$n parts, $maxParts at most' : '$n parts';
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
                hintText: 'Text',
                border: const OutlineInputBorder(),
                helperText: _parts(to, v.text),
              ),
            ),
          ),
        ),
        IconButton(
          tooltip: 'Send',
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
        ? 'They reacted: ${emojiName(r)}'
        : 'You reacted: ${emojiName(r)}';
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
      title: const Text('Copy text'),
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
      if (mounted) say(context, 'Not sent: $e');
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
        Flexible(child: Text(partsLabel(app.session, outs), style: small)),
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
      _mark(Icons.markunread_mailbox, 'From your mailbox', small, color),
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
    Toward.there => _mark(Icons.place, 'To where they were', small, color),
    Toward.home => _mark(Icons.home, 'To their home', small, color),
  };

  /// A sent text's state, each its own icon (common.dart
  /// [sentStates]). Not yet with the node shows a clock rather than
  /// nothing, so a missing mark never means two things. A state that is a
  /// problem keeps its words, which say why.
  List<Widget> _state(
    Outgoing o,
    TextStyle small,
    Color color,
  ) => switch (o.state) {
    OutState.queued => [
      _stateMark([o], Icons.schedule, 'Waiting for a node', small, color),
    ],
    OutState.writing => [
      _stateMark([o], Icons.sync, 'Being handed to the node', small, color),
    ],
    OutState.taken => [
      _stateMark(
        [o],
        Icons.check,
        'Sent into the network, which does not say when it arrives',
        small,
        color,
      ),
    ],
    OutState.givenBack => [
      _stateMark(
        [o],
        Icons.hourglass_bottom,
        'The network is busy',
        small,
        color,
      ),
      const SizedBox(width: 4),
      Flexible(child: Text(capital(stateLabel(app.session, o)), style: small)),
    ],
    OutState.refused => [
      const SizedBox(width: 4),
      Flexible(child: Text(capital(stateLabel(app.session, o)), style: small)),
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
      child: const Text('Cancel'),
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
            child: Text(
              'React: ${widget.to.name} sees it on their text. Pick another '
              'to change it.',
            ),
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
              decoration: const InputDecoration(
                prefixIcon: Icon(Icons.search),
                hintText: 'Search emoji by name',
              ),
            ),
          ),
          Expanded(
            child: CustomScrollView(
              slivers: [
                if (found == null)
                  for (final (title, group) in emojiGroups) ...[
                    SliverPadding(
                      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                      sliver: SliverToBoxAdapter(
                        child: Text(title, style: style),
                      ),
                    ),
                    _grid([for (final e in group) e.emoji]),
                  ]
                else if (found.isEmpty)
                  const SliverPadding(
                    padding: EdgeInsets.all(16),
                    sliver: SliverToBoxAdapter(
                      child: Text('No emoji by that name.'),
                    ),
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
