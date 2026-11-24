// Update firmware: an image from the latest release or a file, sent to
// the connected node with its progress, the node's verdict, the reboot,
// and the node back on the new version (`updater.dart`, which holds what
// is shown when).

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../app.dart';
import '../l10n.dart';
import '../link.dart';
import '../releases.dart';
import '../updater.dart';
import 'common.dart';

class UpdateScreen extends StatefulWidget {
  final AppModel app;

  /// A release whose image the screen downloads and chooses as it opens.
  final Release? release;

  /// Asks for an image file as it opens, for the bench.
  final bool pick;
  const UpdateScreen(this.app, {this.release, this.pick = false, super.key});

  @override
  State<UpdateScreen> createState() => _UpdateScreenState();
}

class _UpdateScreenState extends State<UpdateScreen> {
  AppModel get app => widget.app;
  Updater get u => app.updater;

  /// A download or a file read under way.
  bool _getting = false;

  @override
  void initState() {
    super.initState();
    final r = widget.release;
    // Not over an update under way, nor again for an image chosen.
    final idle = u.phase == Phase.idle || u.phase == Phase.failed;
    if (r != null && idle && u.image?.version != r.firmware) {
      _get(() => downloadImage(r, app.session.note, app.releases.download));
    } else if (widget.pick && idle) {
      _get(_pick);
    }
  }

  Future<void> _get(Future<(FirmwareImage, String?)> Function() f) async {
    setState(() => _getting = true);
    try {
      final (image, from) = await f();
      u.choose(image, from);
    } catch (e) {
      if (mounted) say(context, tr.updateNoFirmware(said(e)));
    } finally {
      if (mounted) setState(() => _getting = false);
    }
  }

  Future<(FirmwareImage, String?)> _pick() async {
    final f = await openFile();
    if (f == null) throw const _NoFile();
    return (parseImage(await f.readAsBytes()), pickedName(f.name));
  }

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    final node = s.node;
    final image = u.image;
    final latest = u.latest;
    final noLatest = u.noLatest;
    final updated = u.updated;
    final busy = u.underWay;
    return Scaffold(
      appBar: AppBar(title: Text(tr.updateTitle)),
      body: SafeArea(
        top: false,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Ticking(
              when: node != null && updated != null,
              (context) => Text(
                node == null
                    ? tr.updateNotConnected
                    : updated == null
                    ? tr.updateRuns(nodeLabel(node.id), node.image)
                    : tr.updateRunsSince(
                        nodeLabel(node.id),
                        node.image,
                        updated.from,
                        ago(app.now().difference(updated.at)),
                      ),
                style: Theme.of(context).textTheme.titleMedium,
              ),
            ),
            const SizedBox(height: 16),
            Wrap(
              spacing: 8,
              children: [
                // Not beside the release's own image, already chosen.
                if (image == null || image.version != latest?.firmware)
                  FilledButton(
                    onPressed: _getting || busy || latest == null
                        ? null
                        : () => _get(
                            () => downloadImage(
                              latest,
                              app.session.note,
                              app.releases.download,
                            ),
                          ),
                    child: Text(tr.updateLatest),
                  ),
                // Only for the bench, which comes here from Debug.
                if (widget.pick)
                  OutlinedButton(
                    onPressed: _getting || busy ? null : () => _get(_pick),
                    child: Text(tr.updatePickFile),
                  ),
                if (_getting)
                  const Padding(
                    padding: EdgeInsets.all(8),
                    child: SizedBox.square(
                      dimension: 20,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    ),
                  ),
              ],
            ),
            if (updated != null && u.hearsNoOne)
              Card(
                color: Theme.of(context).colorScheme.errorContainer,
                child: ListTile(
                  leading: const Icon(Icons.portable_wifi_off),
                  title: Text(tr.updateHearsNoOne),
                  subtitle: Text(u.hearsNoOneWarning(updated)),
                ),
              ),
            if (noLatest != null)
              Padding(
                padding: const EdgeInsets.only(top: 8),
                child: Text(noLatest),
              ),
            if (u.showsImage) ..._image(image!),
          ],
        ),
      ),
    );
  });

  List<Widget> _image(FirmwareImage image) {
    final newer = u.newer;
    final source = u.from ?? tr.updatePickedFile;
    final fraction = image.length == 0 ? 0.0 : u.held / image.length;
    return [
      const SizedBox(height: 16),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: Text(tr.version(image.version)),
        subtitle: Text(
          newer
              ? tr.updateImage(source, _kib(image.length))
              : tr.updateImageNotNewer(source, _kib(image.length)),
        ),
      ),
      if (u.phase != Phase.idle) ...[
        LinearProgressIndicator(value: fraction),
        const SizedBox(height: 4),
        Text(tr.updateOnNode(_kib(u.held), _kib(image.length))),
      ],
      if (u.message != null)
        Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Text(u.message!),
        ),
      const SizedBox(height: 16),
      Wrap(spacing: 8, children: _actions(newer)),
    ];
  }

  /// `bytes` in whole kibibytes, as the screen shows a size.
  static int _kib(int bytes) => (bytes / 1024).round();

  List<Widget> _actions(bool newer) {
    final connected = app.session.connected;
    // What goes on with an image goes on with the node that holds it.
    final at = u.atNode;
    return switch (u.phase) {
      Phase.idle || Phase.failed => [
        FilledButton(
          onPressed: connected && newer ? u.send : null,
          child: Text(tr.sendToNode),
        ),
      ],
      Phase.sending || Phase.committing => [
        Text(u.phase == Phase.sending ? tr.updateSending : tr.updateChecking),
      ],
      Phase.paused => [
        OutlinedButton(
          onPressed: at ? u.send : null,
          child: Text(tr.updateGoOn),
        ),
      ],
      Phase.staged => [
        FilledButton(
          onPressed: at ? u.commit : null,
          child: Text(tr.updateTryAgain),
        ),
      ],
      Phase.committed => [
        FilledButton(
          onPressed: at ? u.reboot : null,
          child: Text(tr.updateReboot(u.image!.version)),
        ),
      ],
      Phase.rebooting => [
        const SizedBox.square(
          dimension: 20,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
      ],
      // Not shown: [Updater.showsImage].
      Phase.done => [],
    };
  }
}

/// The person closed the file picker without a file.
class _NoFile implements Exception, Spoken {
  const _NoFile();
  @override
  String words(AppLocalizations w) => w.updateNoFile;
  @override
  String toString() => words(english);
}
