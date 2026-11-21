// A newer app, downloaded in the app and handed to Android's installer,
// which asks the person before it installs it over this one (README.md,
// "Newer releases"). The APK is kept whole for its version, so that a tap
// after the installer was left, or after Android stopped the app while
// the person allowed it to install apps, installs at once.

import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import 'releases.dart';

/// Where the APKs are kept.
abstract interface class Apks {
  /// The path of the whole APK of app version `app`, if one is kept.
  Future<String?> kept(int app);

  /// Downloads `url`, the APK of app version `app`, telling `progress`
  /// the part come so far, null while the whole length is unknown; the
  /// path it is kept at. Nothing is kept of a download that fails.
  Future<String> download(
    String url,
    int app,
    void Function(double? done) progress,
  );
}

/// No place for files, as in tests that do not download.
class NoApks implements Apks {
  const NoApks();
  @override
  Future<String?> kept(int app) async => null;
  @override
  Future<String> download(String url, int app, void Function(double?) p) =>
      Future.error(UnsupportedError('no place for an APK'));
}

/// The APKs in the folder `apk` of the directory `dir` gives: on a phone
/// the app's cache, which the app's file provider hands to the installer
/// (`res/xml/provided_files.xml`). Only the latest download is kept.
class ApkDir implements Apks {
  final Future<Directory> Function() dir;
  final Future<void> Function(
    String url,
    File to,
    void Function(int got, int? total) progress,
  )
  get;
  ApkDir({this.dir = getTemporaryDirectory, this.get = httpGetTo});

  Future<Directory> get _apks async =>
      Directory('${(await dir()).path}/apk').create(recursive: true);

  static String _name(int app) => 'bark-$app.apk';

  @override
  Future<String?> kept(int app) async {
    final f = File('${(await _apks).path}/${_name(app)}');
    return await f.exists() ? f.path : null;
  }

  @override
  Future<String> download(
    String url,
    int app,
    void Function(double? done) progress,
  ) async {
    final d = await _apks;
    await for (final f in d.list()) {
      await f.delete(recursive: true);
    }
    final whole = File('${d.path}/${_name(app)}');
    // Named for its version only when whole.
    final part = File('${whole.path}.part');
    try {
      await get(
        url,
        part,
        (got, total) => progress(total == null ? null : got / total),
      );
    } catch (_) {
      if (await part.exists()) await part.delete();
      rethrow;
    }
    return (await part.rename(whole.path)).path;
  }
}

/// How far the newer app has come.
enum AppGet { idle, downloading, ready, failed }

/// The download and the start of the installer of a newer app.
class AppUpdate extends ChangeNotifier {
  final Apks apks;

  /// Starts Android's installer for the APK at a path.
  final Future<void> Function(String path) install;

  /// Writes a line to the app's log.
  void Function(String) noted = (_) {};

  AppUpdate(this.apks, this.install);

  /// The app version [state] and [done] are of.
  int? _app;
  AppGet _state = AppGet.idle;
  double? _done;
  bool _busy = false;

  /// How far app version `app` has come.
  AppGet state(int app) => app == _app ? _state : AppGet.idle;

  /// The part of the download come so far, in whole percent; null while
  /// the whole length is unknown.
  int? get percent => _done == null ? null : (_done! * 100).floor();

  /// Downloads the APK of `r`, unless one is kept whole, and starts the
  /// installer for it. A tap while one runs does nothing.
  Future<void> get(Release r) async {
    final url = r.apk;
    if (_busy || url == null) return;
    _busy = true;
    _app = r.app;
    try {
      String? path;
      try {
        path = await apks.kept(r.app);
        if (path == null) {
          _set(AppGet.downloading, null);
          path = await apks.download(url, r.app, _progress);
        }
      } catch (e) {
        noted('Downloading app ${r.app} failed: $e');
        _set(AppGet.failed, null);
        return;
      }
      _set(AppGet.ready, null);
      try {
        await install(path);
      } catch (e) {
        noted('Starting the installer failed: $e');
      }
    } finally {
      _busy = false;
    }
  }

  void _set(AppGet s, double? done) {
    _state = s;
    _done = done;
    notifyListeners();
  }

  // Told only when the whole percent changes, not at each piece.
  void _progress(double? done) {
    final was = percent;
    _done = done;
    if (percent != was) notifyListeners();
  }
}
