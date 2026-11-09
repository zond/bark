// The contacts, and adding one from a card. Archived ones are behind a
// line at the bottom.

import 'package:flutter/material.dart';

import '../app.dart';
import '../card.dart';
import '../contacts.dart';
import 'archived_screen.dart';
import 'common.dart';
import 'scan_screen.dart';
import 'thread_screen.dart';

class ContactsScreen extends StatelessWidget {
  final AppModel app;
  const ContactsScreen(this.app, {super.key});

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final cs = app.contacts;
    final archived = cs.list.length - cs.active.length;
    return Scaffold(
      appBar: AppBar(title: const Text('Contacts')),
      body: ListView(
        children: [
          if (cs.list.isEmpty)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Text(
                'No contacts yet. Ask them to open bark and tap the QR '
                'button, then tap Add contact and scan their card.',
              ),
            ),
          for (final c in cs.active) _row(context, c),
          if (archived > 0)
            ArchivedLine(
              '$archived archived',
              onTap: () => Navigator.of(context)
                  .push(MaterialPageRoute(builder: (_) => ArchivedScreen(app))),
            ),
        ],
      ),
      floatingActionButton: FloatingActionButton.extended(
        icon: const Icon(Icons.person_add),
        label: const Text('Add contact'),
        onPressed: () =>
            Navigator.of(context)
                .push(MaterialPageRoute(builder: (_) => AddContactScreen(app))),
      ),
    );
  });

  /// A swipe either way archives the contact and its conversation, with
  /// an undo, as the row is gone at once.
  Widget _row(BuildContext context, Contact c) => Dismissible(
    key: ValueKey(c.key),
    background: const SwipeBackground(Alignment.centerLeft),
    secondaryBackground: const SwipeBackground(Alignment.centerRight),
    onDismissed: (_) async {
      await app.contacts.archiveContact(c);
      if (!context.mounted) return;
      archivedSay(
        ScaffoldMessenger.of(context),
        app,
        c.key,
        '${c.name}: contact archived',
      );
    },
    child: ListTile(
      leading: const CircleAvatar(child: Icon(Icons.person)),
      title: Text(c.name),
      subtitle: c.card.temporary
          ? const TemporaryLine()
          : const Text('Has a home node'),
      onTap: () => Navigator.of(context).pushReplacement(
        MaterialPageRoute(builder: (_) => ThreadScreen(app, c.key)),
      ),
    ),
  );
}

/// Adds the contact of a scanned card and opens its thread at once, so
/// that the person sees where it went and can write; back from it leads
/// to the conversations.
/// The mark of a temporary card in a contact's row (roadmap D162).
class TemporaryLine extends StatelessWidget {
  const TemporaryLine({super.key});

  @override
  Widget build(BuildContext context) => const Row(
    children: [
      Icon(Icons.hourglass_empty, size: 16, semanticLabel: 'Temporary card'),
      SizedBox(width: 4),
      Flexible(child: Text('temporary: reach them at the node they last used')),
    ],
  );
}

Future<void> openScanned(
  BuildContext context,
  AppModel app,
  PersonCard card,
) async {
  await app.contacts.add(card);
  if (!context.mounted) return;
  Navigator.of(context)
    ..popUntil((r) => r.isFirst)
    ..push(MaterialPageRoute(builder: (_) => ThreadScreen(app, card.key)));
}

class AddContactScreen extends StatefulWidget {
  final AppModel app;
  const AddContactScreen(this.app, {super.key});

  @override
  State<AddContactScreen> createState() => _AddContactScreenState();
}

class _AddContactScreenState extends State<AddContactScreen> {
  final pasted = TextEditingController();
  final name = TextEditingController();
  PersonCard? card;
  String? error;

  @override
  void dispose() {
    pasted.dispose();
    name.dispose();
    super.dispose();
  }

  /// A pasted card: its identity is what the app seals texts to, so an
  /// address alone is not enough.
  void _read(String s) {
    if (pasted.text != s) pasted.text = s;
    setState(() {
      card = null;
      error = null;
      if (s.trim().isEmpty) return;
      try {
        card = PersonCard.parse(s);
        if (name.text.isEmpty) name.text = card!.name;
      } on BadCard catch (e) {
        error = '$e. Paste a card: it starts with ${PersonCard.prefix}';
      }
    });
  }

  Future<void> _scan() async {
    final c = await scanCard(context);
    if (c != null && mounted) await openScanned(context, widget.app, c);
  }

  Future<void> _save() async {
    final c = card;
    if (c == null) return;
    // The card's own name is not chosen here, so a later card's shows.
    final n = name.text.trim();
    await widget.app.contacts.add(c, name: n == c.name ? '' : n);
    if (!mounted) return;
    // Back from the new thread leads to the conversations.
    Navigator.of(context)
      ..popUntil((r) => r.isFirst)
      ..push(
        MaterialPageRoute(builder: (_) => ThreadScreen(widget.app, c.key)),
      );
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('Add contact')),
    body: SafeArea(
      top: false,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton.icon(
              icon: const Icon(Icons.qr_code_scanner),
              label: const Text('Scan their card'),
              onPressed: _scan,
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            controller: pasted,
            onChanged: _read,
            maxLines: null,
            decoration: InputDecoration(
              labelText: 'Card',
              helperText: 'Or paste the card they sent you',
              errorText: error,
              errorMaxLines: 3,
            ),
          ),
          if (card != null)
            ListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('Home node'),
              subtitle: Text(
                card!.home?.toString() ??
                    'none: you can write to them once they have written to '
                        'you',
                style: const TextStyle(fontFamily: 'monospace'),
              ),
            ),
          const SizedBox(height: 16),
          TextField(
            controller: name,
            onChanged: (_) => setState(() {}),
            textCapitalization: TextCapitalization.words,
            decoration: const InputDecoration(labelText: 'Name'),
          ),
          const SizedBox(height: 16),
          Align(
            alignment: Alignment.centerLeft,
            child: FilledButton(
              onPressed: card == null ? null : _save,
              child: const Text('Save'),
            ),
          ),
        ],
      ),
    ),
  );
}
