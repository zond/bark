// The contacts, and adding one from a card.

import 'package:flutter/material.dart';

import '../app.dart';
import '../card.dart';
import '../contacts.dart';
import 'common.dart';
import 'scan_screen.dart';
import 'thread_screen.dart';

class ContactsScreen extends StatelessWidget {
  final AppModel app;
  const ContactsScreen(this.app, {super.key});

  @override
  Widget build(BuildContext context) => Watch(
    app,
    (context) => Scaffold(
      appBar: AppBar(title: const Text('Contacts')),
      body: ListView(
        children: [
          if (app.contacts.list.isEmpty)
            const Padding(
              padding: EdgeInsets.all(16),
              child: Text('No contacts yet.'),
            ),
          for (final c in app.contacts.list)
            ListTile(
              leading: const CircleAvatar(child: Icon(Icons.person)),
              title: Text(c.name),
              subtitle: Text(
                !c.verified
                    ? 'not verified yet'
                    : c.card.home?.toString() ?? 'no home node',
                style: const TextStyle(fontFamily: 'monospace'),
              ),
              onTap: () => Navigator.of(context).pushReplacement(
                MaterialPageRoute(builder: (_) => ThreadScreen(app, c.key)),
              ),
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
    ),
  );
}

class AddContactScreen extends StatefulWidget {
  final AppModel app;

  /// A card already scanned, to name and save.
  final PersonCard? card;
  const AddContactScreen(this.app, {this.card, super.key});

  @override
  State<AddContactScreen> createState() => _AddContactScreenState();
}

class _AddContactScreenState extends State<AddContactScreen> {
  final pasted = TextEditingController();
  final name = TextEditingController();
  PersonCard? card;
  String? error;

  @override
  void initState() {
    super.initState();
    final c = widget.card;
    if (c != null) _read(c.toText());
  }

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
    if (c != null) _read(c.toText());
  }

  Future<void> _save() async {
    final c = card;
    final n = name.text.trim();
    if (c == null || n.isEmpty) return;
    final cs = widget.app.contacts;
    final known = cs.byKey(c.key);
    await cs.put(known?.copyWith(card: c, name: n) ?? Contact(c, n));
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
              onPressed: card == null || name.text.trim().isEmpty
                  ? null
                  : _save,
              child: const Text('Save'),
            ),
          ),
        ],
      ),
    ),
  );
}
