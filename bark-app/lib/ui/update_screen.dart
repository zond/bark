// Update firmware: an image from the latest release or a file, sent to
// the connected node with its progress, the node's verdict, the reboot,
// and the node back on the new version (`updater.dart`).

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

import '../app.dart';
import '../link.dart';
import '../releases.dart';
import '../updater.dart';
import 'common.dart';

class UpdateScreen extends StatefulWidget {
  final AppModel app;

  /// A release whose image the screen downloads and chooses as it opens.
  final Release? release;
  const UpdateScreen(this.app, {this.release, super.key});

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
    if (r != null && idle && u.image?.version != r.n) {
      _get(() => downloadImage(r, app.releases.download));
    }
  }

  Future<void> _get(Future<(FirmwareImage, String)> Function() f) async {
    setState(() => _getting = true);
    try {
      final (image, from) = await f();
      u.choose(image, from);
    } catch (e) {
      if (mounted) say(context, 'No image: $e');
    } finally {
      if (mounted) setState(() => _getting = false);
    }
  }

  Future<(FirmwareImage, String)> _pick() async {
    final f = await openFile();
    if (f == null) throw StateError('no file chosen');
    return (parseImage(await f.readAsBytes()), f.name);
  }

  @override
  Widget build(BuildContext context) => Watch(app, (context) {
    final s = app.session;
    final node = s.node;
    final image = u.image;
    final busy =
        u.phase == Phase.sending ||
        u.phase == Phase.committing ||
        u.phase == Phase.rebooting;
    return Scaffold(
      appBar: AppBar(title: const Text('Update firmware')),
      body: SafeArea(
        top: false,
        child: ListView(
          padding: const EdgeInsets.all(16),
          children: [
            Text(
              node == null
                  ? 'Not connected to a node.'
                  : '${nodeName(node.id)} runs version ${node.image}.',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 16),
            Wrap(
              spacing: 8,
              children: [
                FilledButton(
                  onPressed: _getting || busy
                      ? null
                      : () => _get(latestRelease),
                  child: const Text('Latest release'),
                ),
                OutlinedButton(
                  onPressed: _getting || busy ? null : () => _get(_pick),
                  child: const Text('Pick a file'),
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
            if (image != null) ..._image(image, node?.image),
          ],
        ),
      ),
    );
  });

  List<Widget> _image(FirmwareImage image, int? running) {
    final newer = running == null || image.version > running;
    final fraction = image.length == 0 ? 0.0 : u.held / image.length;
    return [
      const SizedBox(height: 16),
      ListTile(
        contentPadding: EdgeInsets.zero,
        title: Text('Version ${image.version}'),
        subtitle: Text(
          '${u.from}, ${(image.length / 1024).toStringAsFixed(0)} KiB'
          '${newer ? '' : ': not newer than the node\'s'}',
        ),
      ),
      if (u.phase != Phase.idle) ...[
        LinearProgressIndicator(value: fraction),
        const SizedBox(height: 4),
        Text(
          '${(u.held / 1024).toStringAsFixed(0)} of '
          '${(image.length / 1024).toStringAsFixed(0)} KiB on the node',
        ),
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

  List<Widget> _actions(bool newer) {
    final connected = app.session.connected;
    return switch (u.phase) {
      Phase.idle || Phase.failed => [
        FilledButton(
          onPressed: connected && newer ? u.send : null,
          child: const Text('Send to the node'),
        ),
      ],
      Phase.sending || Phase.committing => [
        Text(
          u.phase == Phase.sending ? 'Sending…' : 'The node checks the image…',
        ),
      ],
      Phase.paused => [
        OutlinedButton(
          onPressed: connected ? u.send : null,
          child: const Text('Go on'),
        ),
      ],
      Phase.staged => [
        FilledButton(
          onPressed: connected ? u.commit : null,
          child: const Text('Commit'),
        ),
      ],
      Phase.committed => [
        FilledButton(
          onPressed: connected ? u.reboot : null,
          child: Text('Reboot into version ${u.image!.version}'),
        ),
      ],
      Phase.rebooting => [
        const SizedBox.square(
          dimension: 20,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
      ],
      Phase.done => [const Text('Done.')],
    };
  }
}
