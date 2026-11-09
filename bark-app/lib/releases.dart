// The latest release on GitHub, checked at start and once a day after:
// whether it carries a newer app than this one, or a newer image than the
// connected node runs. A release names the two versions apart, as one may
// carry on the other's files unchanged (roadmap D164). The answer and what the person dismissed are kept
// on disk, so that the home screen shows them at once at the next start.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'link.dart';
import 'store.dart';

/// This app's version: the number of the release that built it, given to
/// the build with `--dart-define=BARK_VERSION=<N>`
/// (`.github/workflows/release.yml`), and named `app` by that release's
/// manifest and by the later ones that carry it on. 0 is a development
/// build, which never asks to be updated.
const appVersion = int.fromEnvironment('BARK_VERSION');

/// The latest release of the project, from GitHub's releases API.
const releasesUrl = 'https://api.github.com/repos/zond/bark/releases/latest';

/// How often a running app checks again. The API allows 60 requests an
/// hour from an address without a token (GitHub REST API documentation,
/// "Rate limits"); once a day is plenty for releases made by hand.
const checkEvery = Duration(days: 1);

/// The body of a GET of `url`; an HTTP status other than 200 throws.
Future<List<int>> httpGet(String url, {bool json = false}) async {
  final client = HttpClient();
  try {
    final rq = await client.getUrl(Uri.parse(url));
    // GitHub's API refuses a request without a user agent.
    rq.headers.set(HttpHeaders.userAgentHeader, 'bark-app');
    if (json) rq.headers.set(HttpHeaders.acceptHeader, 'application/json');
    final rs = await rq.close();
    if (rs.statusCode != HttpStatus.ok) {
      throw HttpException('$url: HTTP ${rs.statusCode}');
    }
    return [for (final chunk in await rs.toList()) ...chunk];
  } finally {
    client.close();
  }
}

/// The latest release as the API gives it.
Future<Map<String, dynamic>> fetchLatest() async =>
    jsonDecode(utf8.decode(await httpGet(releasesUrl, json: true)))
        as Map<String, dynamic>;

/// The name of a release's manifest: `{"release": N, "app": A,
/// "firmware": F}`, the release's number and the versions of the app and
/// the node firmware it carries.
const manifestName = 'release.json';

/// The download address of the file `name` in the API's answer `j`.
String? assetUrl(Map<String, dynamic> j, String name) {
  for (final a in (j['assets'] as List? ?? const [])) {
    final asset = a as Map<String, dynamic>;
    if (asset['name'] == name) return asset['browser_download_url'] as String?;
  }
  return null;
}

/// A release `v<N>`: its N, the app's version A and the node firmware's F
/// it carries, each N if built for it or an earlier release's carried on,
/// and where to download its APK and its node image, when it carries
/// them.
class Release {
  final int n;
  final int app;
  final int firmware;
  final String? apk;
  final String? image;
  const Release(this.n, {int? app, int? firmware, this.apk, this.image})
    : app = app ?? n,
      firmware = firmware ?? n;

  /// The image's file name.
  String get imageName => 'bark-fw-$firmware.img';

  /// The release in the API's answer `j`, with `manifest`, the release's
  /// [manifestName], when it carries one; null if its tag is not `v<N>`.
  /// A release without a manifest, v21 and older, carried one version,
  /// N. The files are those named for the versions, as the workflow
  /// names them. A manifest that does not fit the tag throws.
  static Release? parse(
    Map<String, dynamic> j, [
    Map<String, dynamic>? manifest,
  ]) {
    final m = RegExp(r'^v([1-9][0-9]*)$').firstMatch('${j['tag_name']}');
    if (m == null) return null;
    final n = int.parse(m[1]!);
    var app = n, firmware = n;
    if (manifest != null) {
      // A version carried on is an earlier release's, never a later one.
      int version(String key) {
        final v = manifest[key];
        if (v is! int || v < 1 || v > n) {
          throw FormatException('release $n: $manifestName $key $v');
        }
        return v;
      }

      if (manifest['release'] != n) {
        throw FormatException(
          'release $n: $manifestName names ${manifest['release']}',
        );
      }
      app = version('app');
      firmware = version('firmware');
    }
    return Release(
      n,
      app: app,
      firmware: firmware,
      apk: assetUrl(j, 'bark-$app.apk'),
      image: assetUrl(j, 'bark-fw-$firmware.img'),
    );
  }

  Map<String, dynamic> toJson() => {
    'n': n,
    'app': app,
    'firmware': firmware,
    'apk': apk,
    'image': image,
  };

  /// A release kept before the manifest has no `app` or `firmware`: both
  /// were its N.
  static Release fromJson(Map<String, dynamic> j) => Release(
    j['n'] as int,
    app: j['app'] as int?,
    firmware: j['firmware'] as int?,
    apk: j['apk'] as String?,
    image: j['image'] as String?,
  );
}

/// The latest release and what the person dismissed of it.
class Releases extends ChangeNotifier {
  final Store store;
  final DateTime Function() now;

  /// Fetches the latest release; null checks nothing, as in tests that
  /// do not drive it.
  final Future<Map<String, dynamic>> Function()? fetch;

  /// Downloads a release's file, its manifest among them.
  final Future<List<int>> Function(String url) download;

  /// This app's release, [appVersion] on a phone.
  final int app;

  /// Writes a line to the app's log.
  void Function(String) noted = (_) {};

  Releases(
    this.store, {
    required this.now,
    this.fetch,
    this.download = httpGet,
    this.app = 0,
  });

  /// The latest release as last fetched, and when.
  Release? latest;
  DateTime? checked;

  /// The newest app version the person dismissed.
  int dismissedApp = 0;

  /// The newest image version the person dismissed, by node name.
  final Map<String, int> dismissedNodes = {};

  /// When this run of the app last tried to fetch, whatever came of it:
  /// a failed fetch waits as long as a good one, and the next start tries
  /// at once.
  DateTime? _tried;
  bool _loaded = false;
  bool _fetching = false;

  static const _format = 1;

  Future<void> load() async {
    await store.load(_format, (j) {
      final r = j['release'];
      final c = j['checked'];
      final nodes = (j['dismissedNodes'] as Map? ?? const {}).map(
        (k, v) => MapEntry(k as String, v as int),
      );
      latest = r == null ? null : Release.fromJson(r as Map<String, dynamic>);
      checked = c == null ? null : DateTime.parse(c as String);
      dismissedApp = j['dismissedApp'] as int? ?? 0;
      dismissedNodes
        ..clear()
        ..addAll(nodes);
    });
    _loaded = true;
    notifyListeners();
  }

  Future<void> _save() => store.save(_format, {
    'release': latest?.toJson(),
    'checked': checked?.toUtc().toIso8601String(),
    'dismissedApp': dismissedApp,
    'dismissedNodes': dismissedNodes,
  });

  /// A fetch is due: none yet in this run, or the last a day ago.
  bool get due =>
      fetch != null &&
      _loaded &&
      !_fetching &&
      (_tried == null || !now().isBefore(_tried!.add(checkEvery)));

  /// Fetches the latest release if [due], or at once when the person
  /// asks, `now`. A failure, such as no network, keeps what was known and
  /// shows nothing but a line in the log. Whether it fetched one.
  Future<bool> check({bool now = false}) async {
    if (!(now ? fetch != null && _loaded && !_fetching : due)) return false;
    _fetching = true;
    notifyListeners();
    _tried = this.now();
    try {
      final j = await fetch!();
      final m = assetUrl(j, manifestName);
      final manifest = m == null
          ? null
          : jsonDecode(utf8.decode(await download(m))) as Map<String, dynamic>;
      final r = Release.parse(j, manifest);
      if (r == null) throw FormatException('tag ${j['tag_name']}');
      latest = r;
      checked = this.now();
      await _save();
      return true;
    } catch (e) {
      noted('Checking for a new release failed: $e');
      return false;
    } finally {
      _fetching = false;
      notifyListeners();
    }
  }

  /// A check runs.
  bool get checking => _fetching;

  /// The latest release if it carries a newer app than this one, and the
  /// person has not dismissed it. A development build is never older.
  Release? get newerApp {
    final r = latest;
    if (app == 0 || r == null || r.apk == null) return null;
    return r.app > app && r.app > dismissedApp ? r : null;
  }

  /// The latest release if it carries a newer image than `node` runs, and
  /// the person has not dismissed it for that node. A node of version 0,
  /// a bench build flashed by USB, is never older.
  Release? newerImage(NodeInfo? node) {
    final r = latest;
    if (node == null || node.image == 0 || r == null || r.image == null) {
      return null;
    }
    final dismissed = dismissedNodes[nodeName(node.id)] ?? 0;
    return r.firmware > node.image && r.firmware > dismissed ? r : null;
  }

  /// No more word of an app of version `n` or older.
  Future<void> dismissApp(int n) async {
    dismissedApp = n;
    notifyListeners();
    await _save();
  }

  /// No more word, for `node`, of an image of version `n` or older.
  Future<void> dismissImage(NodeInfo node, int n) async {
    dismissedNodes[nodeName(node.id)] = n;
    notifyListeners();
    await _save();
  }
}

/// What a check the person asked for found, in one line: this app's
/// version against the latest release's app, and the node's image
/// against the release's firmware, or that no node is linked.
String updatesLine(Releases r, NodeInfo? node, {bool guest = false}) {
  final l = r.latest;
  final app = r.app == 0
      ? 'app is a development build'
      : l != null && l.apk != null && l.app > r.app
      ? 'a newer app, version ${l.app}, is available: get it under App below'
      : 'app up to date (${r.app})';
  final String image;
  if (node == null) {
    image = 'no node linked';
  } else if (guest) {
    image = 'the node is not yours to update';
  } else if (node.image == 0) {
    image = 'node firmware is a bench build';
  } else if (l == null || l.image == null) {
    image = 'node firmware ${node.image}; the release carries none';
  } else if (l.firmware > node.image) {
    image =
        'node firmware ${node.image}; version ${l.firmware} is newer: send '
        'it under Node firmware below';
  } else {
    image = 'node firmware ${node.image}, up to date';
  }
  final line = '$app; $image';
  return line[0].toUpperCase() + line.substring(1);
}
