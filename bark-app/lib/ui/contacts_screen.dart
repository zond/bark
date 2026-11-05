// The contacts, and adding one from a card or an address.

import 'package:flutter/material.dart';

import '../app.dart';
import '../card.dart';
import '../contacts.dart';
import '../link.dart';
import 'common.dart';
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
                c.address.toString(),
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

  /// A pasted card, or an address alone, which makes a card with no name.
  void _read(String s) {
    setState(() {
      card = null;
      error = null;
      if (s.trim().isEmpty) return;
      final a = Address.parse(s);
      if (a != null) {
        card = PersonCard(a, '');
        return;
      }
      try {
        card = PersonCard.parse(s);
        if (name.text.isEmpty) name.text = card!.name;
      } on BadCard catch (e) {
        error =
            '$e. Paste a card, or an address of '
            '${Address.len * 2} hex digits.';
      }
    });
  }

  Future<void> _save() async {
    final c = card;
    final n = name.text.trim();
    if (c == null || n.isEmpty) return;
    await widget.app.contacts.put(Contact(c, n));
    if (!mounted) return;
    // Back from the new thread leads to the conversations.
    Navigator.of(context)
      ..popUntil((r) => r.isFirst)
      ..push(
        MaterialPageRoute(
          builder: (_) => ThreadScreen(widget.app, nodeKey(c.address)),
        ),
      );
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('Add contact')),
    body: ListView(
      padding: const EdgeInsets.all(16),
      children: [
        TextField(
          controller: pasted,
          onChanged: _read,
          maxLines: null,
          decoration: InputDecoration(
            labelText: 'Card or address',
            helperText: 'Paste the card or the address they sent you',
            errorText: error,
            errorMaxLines: 3,
          ),
        ),
        if (card != null)
          ListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Address'),
            subtitle: Text(
              card!.address.toString(),
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
            onPressed: card == null || name.text.trim().isEmpty ? null : _save,
            child: const Text('Save'),
          ),
        ),
      ],
    ),
  );
}
