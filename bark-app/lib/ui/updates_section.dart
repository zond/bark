// Settings' Updates section: the check the person asks for, the app's
// version and the way to a newer one, the node's image and the way to
// send it a newer one (`README.md`, "Newer releases").

import 'package:flutter/material.dart';

import '../app.dart';
import '../app_update.dart';
import '../l10n.dart';
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
    final paired = n != null && !s.guest;
    final newerImage =
        paired &&
        latest != null &&
        latest.image != null &&
        n.image != 0 &&
        latest.firmware > n.image;
    final reached = _reached;
    final get = newerApp ? app.appUpdate.state(latest.app) : AppGet.idle;
    final percent = app.appUpdate.percent;
    final appLine = r.app == 0
        ? tr.updatesDevBuild
        : !newerApp
        ? tr.version(r.app)
        : switch (get) {
            AppGet.idle => tr.updatesAppOut(r.app, latest.app),
            AppGet.downloading =>
              percent == null
                  ? tr.updatesAppDownloading(r.app, latest.app)
                  : tr.updatesAppDownloadingPercent(r.app, latest.app, percent),
            AppGet.ready => tr.updatesAppReady(r.app, latest.app),
            AppGet.failed => tr.updatesAppFailed(r.app, latest.app),
          };
    return [
      SettingsHeading(tr.updatesHeading),
      Row(
        children: [
          OutlinedButton(
            onPressed: r.checking || r.fetch == null ? null : _check,
            child: Text(tr.updatesCheck),
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
            reached ? updatesLine(r, n, guest: s.guest) : tr.updatesUnreached,
          ),
        ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: Text(tr.updatesApp),
        // A sentence of its own after the app's line.
        subtitle: Text(
          s.nodeNewer ? '$appLine. ${tr.updatesNodeNewer}' : appLine,
        ),
        trailing: newerApp && get != AppGet.downloading
            ? FilledButton(
                onPressed: () => app.appUpdate.get(latest),
                child: Text(
                  get == AppGet.failed ? tr.updateTryAgain : tr.updatesInstall,
                ),
              )
            : null,
      ),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: Text(tr.updatesNodeFirmware),
        subtitle: Text(
          n == null
              ? tr.nodeNotConnected
              : s.guest
              ? tr.updatesGuest
              : newerImage
              ? tr.updatesFirmwareOut(n.image, latest.firmware)
              : tr.version(n.image),
        ),
        trailing: newerImage
            ? FilledButton(
                onPressed: () => Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => UpdateScreen(app, release: latest),
                  ),
                ),
                child: Text(tr.sendToNode),
              )
            : null,
      ),
    ];
  }
}
