// Settings' My identity: the identity file, saved to move to a new phone
// and restored there (`identity_file.dart`).

import 'package:flutter/foundation.dart' show Uint8List;
import 'package:flutter/material.dart';

import '../app.dart';
import '../identity_file.dart';
import 'common.dart';
import 'settings_parts.dart';

class IdentitySection extends StatelessWidget {
  final AppModel app;
  const IdentitySection(this.app, {super.key});

  /// What the file is and what moving takes, for whoever asks.
  static const help =
      'The file holds your identity, name, home, contacts and the nodes you '
      'paired with, and your messages if you tick the box. Your password '
      'locks it: without it nobody can open the file, not even you.\n\n'
      'On the new phone, tap Restore from a file, then pair with your nodes '
      'again: a pairing stays on the phone it was made on. Your home keeps '
      'your mail meanwhile, and you can fetch it from any other node.\n\n'
      'Use one phone at a time: two phones with one identity would each '
      'take mail the other never sees.\n\n'
      'Your 12 words bring back your identity alone.';

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      const SettingsHeading('My identity', help: help),
      const Text('Save it to a file to move to a new phone.'),
      const SizedBox(height: 8),
      Wrap(
        spacing: 8,
        children: [
          FilledButton(
            onPressed: () => _save(context),
            child: const Text('Save to a file'),
          ),
          OutlinedButton(
            onPressed: () => _restore(context),
            child: const Text('Restore from a file'),
          ),
        ],
      ),
    ],
  );

  /// The file's name as offered, dated, so that a later save is told
  /// apart from an earlier.
  String _fileName() {
    final d = app.now().toIso8601String().substring(0, 10);
    return 'bark-identity-$d.bark';
  }

  Future<void> _save(BuildContext context) async {
    final file = await showDialog<Uint8List>(
      context: context,
      builder: (_) => _PasswordDialog(
        title: 'Save my identity',
        action: 'Save',
        twice: true,
        run: (password, messages) =>
            app.identityFile(password, messages: messages),
      ),
    );
    if (file == null || !context.mounted) return;
    try {
      final saved = await app.notifier.saveFile(_fileName(), file);
      if (context.mounted) say(context, saved ? 'Saved' : 'Not saved');
    } catch (e) {
      if (context.mounted) say(context, 'Saving failed: $e');
    }
  }

  Future<void> _restore(BuildContext context) async {
    final go = await confirm(
      context,
      'Restore from a file?',
      'This replaces the identity, contacts and messages on this phone.',
      'Choose file',
    );
    if (!go || !context.mounted) return;
    final Uint8List? file;
    try {
      file = await app.notifier.openFile();
    } catch (e) {
      if (context.mounted) say(context, 'Opening failed: $e');
      return;
    }
    if (file == null || !context.mounted) return;
    final f = file;
    final done = await showDialog<bool>(
      context: context,
      builder: (_) => _PasswordDialog(
        title: 'The file\'s password',
        action: 'Restore',
        twice: false,
        run: (password, _) async {
          await app.restoreFile(f, password);
          return true;
        },
      ),
    );
    if (done != true || !context.mounted) return;
    say(
      context,
      app.nodes.lost.isEmpty
          ? 'Identity restored'
          : 'Identity restored. Pair again with your nodes.',
    );
  }
}

/// Takes a password, `twice` when making a file, with the box that adds
/// the messages, and runs `run` with it while the dialog shows a
/// spinner; closes with its result. A file refused says why and keeps the
/// dialog open for another try.
class _PasswordDialog<T> extends StatefulWidget {
  final String title;
  final String action;
  final bool twice;
  final Future<T> Function(String password, bool messages) run;
  const _PasswordDialog({
    required this.title,
    required this.action,
    required this.twice,
    required this.run,
  });

  @override
  State<_PasswordDialog<T>> createState() => _PasswordDialogState<T>();
}

class _PasswordDialogState<T> extends State<_PasswordDialog<T>> {
  final _password = TextEditingController();
  final _again = TextEditingController();
  bool _messages = false;
  bool _running = false;
  String? _refused;

  @override
  void dispose() {
    _password.dispose();
    _again.dispose();
    super.dispose();
  }

  bool get _differ => widget.twice && _again.text != _password.text;
  bool get _ready => _password.text.isNotEmpty && !_differ && !_running;

  Future<void> _go() async {
    setState(() {
      _running = true;
      _refused = null;
    });
    try {
      final v = await widget.run(_password.text, _messages);
      if (mounted) Navigator.pop(context, v);
    } on FileRefused catch (e) {
      if (mounted) setState(() => _refused = '$e Nothing changed.');
    } catch (e) {
      if (mounted) setState(() => _refused = 'Failed: $e');
    } finally {
      if (mounted) setState(() => _running = false);
    }
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: Text(widget.title),
    // Its fields stay in reach above the keyboard on a phone held sideways.
    scrollable: true,
    content: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          key: const Key('password'),
          controller: _password,
          autofocus: true,
          obscureText: true,
          enabled: !_running,
          decoration: const InputDecoration(labelText: 'Password'),
          onChanged: (_) => setState(() {}),
        ),
        if (widget.twice) ...[
          TextField(
            key: const Key('password again'),
            controller: _again,
            obscureText: true,
            enabled: !_running,
            decoration: InputDecoration(
              labelText: 'Password again',
              errorText: _differ && _again.text.isNotEmpty
                  ? 'Not the same'
                  : null,
            ),
            onChanged: (_) => setState(() {}),
          ),
          CheckboxListTile(
            contentPadding: EdgeInsets.zero,
            controlAffinity: ListTileControlAffinity.leading,
            title: const Text('Add my messages'),
            value: _messages,
            onChanged: _running
                ? null
                : (v) => setState(() => _messages = v ?? false),
          ),
          Text(
            'Use it on one phone at a time.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
        if (_refused case final r?)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              r,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
      ],
    ),
    actions: [
      TextButton(
        onPressed: _running ? null : () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      FilledButton(
        onPressed: _ready ? _go : null,
        child: _running
            ? const SizedBox.square(
                dimension: 20,
                child: CircularProgressIndicator(strokeWidth: 2),
              )
            : Text(widget.action),
      ),
    ],
  );
}
