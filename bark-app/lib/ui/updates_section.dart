// Settings' Updates section: the check the person asks for, the app's
// version and the way to a newer one, the node's image and the way to
// send it a newer one (`README.md`, "Newer releases").

import 'package:flutter/material.dart';

import '../app.dart';
import '../releases.dart';
import 'settings_parts.dart';
import 'update_screen.dart';

class UpdatesSection extends StatefulWidget {
  final AppModel app;
  const UpdatesSection(this.app, {super.key});

  @override
  State<UpdatesSection> createState() => _UpdatesSectionState();
}

class _UpdatesSectionState extends State<UpdatesSection> {
  /// The outcome of the last check the person asked for: null before
  /// one, else whether the release list was reached.
  bool? _reached;

  AppModel get app => widget.app;

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: _updates(),
  );

  Future<void> _check() async {
    final ok = await app.releases.check(now: true);
    if (context.mounted) setState(() => _reached = ok);
  }

  /// Updates: the check the person asks for, with its outcome; the app's
  /// version and the way to a newer one; the node's image and the way to
  /// send it a newer one. A firmware file for the bench is picked in
  /// Debug.
  List<Widget> _updates() {
    final r = app.releases;
    final s = app.session;
    final n = s.node;
    final latest = r.latest;
    final newerApp =
        latest != null &&
        latest.apk != null &&
        r.app != 0 &&
        latest.app > r.app;
    final owner = n != null && !s.guest;
    final newerImage =
        owner &&
        latest != null &&
        latest.image != null &&
        n.image != 0 &&
        latest.firmware > n.image;
    final reached = _reached;
    return [
      SettingsHeading('Updates'),
      Row(
        children: [
          OutlinedButton(
            onPressed: r.checking || r.fetch == null ? null : _check,
            child: const Text('Check for updates'),
          ),
          const SizedBox(width: 16),
          if (r.checking)
            const SizedBox.square(
              dimension: 20,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
        ],
      ),
      if (reached != null && !r.checking)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(
            reached
                ? updatesLine(r, n, guest: s.guest)
                : 'Could not reach the release list',
          ),
        ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('App'),
        subtitle: Text(
          (r.app == 0
                  ? 'A development build'
                  : newerApp
                  ? 'Version ${r.app}; ${latest.app} is out. Tap to download'
                  : 'Version ${r.app}') +
              (s.nodeNewer ? '. Update it for the node\'s new features' : ''),
        ),
        onTap: newerApp ? () => app.notifier.openLink(latest.apk!) : null,
      ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: const Text('Node firmware'),
        subtitle: Text(
          n == null
              ? 'Not connected'
              : s.guest
              ? 'Only its paired phones update it'
              : newerImage
              ? 'Version ${n.image}; ${latest.firmware} is out'
              : 'Version ${n.image}',
        ),
        trailing: newerImage
            ? FilledButton(
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => UpdateScreen(app, release: latest),
                  ),
                ),
                child: const Text('Send to the node'),
              )
            : null,
      ),
    ];
  }
}
