// The latest release on GitHub, checked at start and once a day after:
// whether it carries a newer app than this one, or a newer image than the
// connected node runs. The answer and what the person dismissed are kept
// on disk, so that the home screen shows them at once at the next start.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'link.dart';
import 'store.dart';

/// The release this app was built as: the workflow's N, given to the build
/// with `--dart-define=BARK_VERSION=<N>` (`.github/workflows/release.yml`).
/// 0 is a development build, which never asks to be updated.
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

/// A release `v<N>`: its N, and where to download its APK and its node
/// image, when it carries them.
class Release {
  final int n;
  final String? apk;
  final String? image;
  const Release(this.n, {this.apk, this.image});

  /// The image's file name.
  String get imageName => 'bark-fw-$n.img';

  /// The release in the API's answer; null if its tag is not `v<N>`. The
  /// files are those named for N, as the workflow names them.
  static Release? parse(Map<String, dynamic> j) {
    final m = RegExp(r'^v([1-9][0-9]*)$').firstMatch('${j['tag_name']}');
    if (m == null) return null;
    final n = int.parse(m[1]!);
    String? url(String name) {
      for (final a in (j['assets'] as List? ?? const [])) {
        final asset = a as Map<String, dynamic>;
        if (asset['name'] == name) {
          return asset['browser_download_url'] as String?;
        }
      }
      return null;
    }

    return Release(n, apk: url('bark-$n.apk'), image: url('bark-fw-$n.img'));
  }

  Map<String, dynamic> toJson() => {'n': n, 'apk': apk, 'image': image};

  static Release fromJson(Map<String, dynamic> j) => Release(
    j['n'] as int,
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

  /// Downloads a release's file.
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

  /// Fetches the latest release if [due]. A failure, such as no network,
  /// keeps what was known and shows nothing but a line in the log.
  Future<void> check() async {
    if (!due) return;
    _fetching = true;
    _tried = now();
    try {
      final j = await fetch!();
      final r = Release.parse(j);
      if (r == null) throw FormatException('tag ${j['tag_name']}');
      latest = r;
      checked = now();
      await _save();
      notifyListeners();
    } catch (e) {
      noted('Checking for a new release failed: $e');
    } finally {
      _fetching = false;
    }
  }

  /// The latest release if it carries a newer app than this one, and the
  /// person has not dismissed it. A development build is never older.
  Release? get newerApp {
    final r = latest;
    if (app == 0 || r == null || r.apk == null) return null;
    return r.n > app && r.n > dismissedApp ? r : null;
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
    return r.n > node.image && r.n > dismissed ? r : null;
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
