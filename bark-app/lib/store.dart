// Where the app keeps its queue, the messages it has received, the
// contacts and the person: the node forgets a message once the app
// confirms it, so the app must have it on disk first.
//
// Each file carries the version of its format. One the app cannot read,
// of another version or not parsing, is set aside under another name and
// the app starts that file afresh: nothing is overwritten, and the app
// always starts.

import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

abstract class Store {
  /// The file's name, for the log.
  String get name;

  /// The file's text; null if there is none.
  Future<String?> read();

  /// Replaces the file's text.
  Future<void> write(String text);

  /// Moves the file to `<name>.old-<suffix>`, or to `-2`, `-3` and on
  /// after it if that is taken: an older file set aside may hold an
  /// identity. Returns the name it took.
  Future<String> setAside(String suffix);

  /// What happened to a file set aside at the last load, for the log.
  String? fault;

  /// The file read by `parse` if it is of `version`; null if there is
  /// none. A file of another version, or one that does not parse,
  /// is set aside and null returned. `parse` reads the whole file before
  /// it changes anything, so a fault leaves nothing half loaded.
  ///
  /// Files written before versions were written have no version and are
  /// read as version 1.
  Future<T?> load<T>(
    int version,
    T Function(Map<String, dynamic>) parse,
  ) async {
    final text = await read();
    if (text == null) return null;
    Object? v;
    try {
      final j = jsonDecode(text) as Map<String, dynamic>;
      v = j['version'] ?? 1;
      if (v is! int) throw FormatException('version $v');
      if (v == version) return parse(j);
    } catch (e) {
      // A parse fault of any kind: a cast, a missing field, bad JSON.
      await _setAside('bad', 'did not read: $e');
      return null;
    }
    await _setAside('$v', 'is of version $v, this app reads $version');
    return null;
  }

  Future<void> _setAside(String suffix, String why) async {
    final aside = await setAside(suffix);
    fault = 'Stored $name $why; kept as $aside, started afresh';
  }

  Future<void> save(int version, Map<String, dynamic> state) =>
      write(jsonEncode({'version': version, ...state}));
}

class FileStore extends Store {
  @override
  final String name;
  FileStore(this.name);

  Future<File> _file() async =>
      File('${(await getApplicationSupportDirectory()).path}/$name');

  @override
  Future<String?> read() async {
    final f = await _file();
    if (!await f.exists()) return null;
    return f.readAsString();
  }

  @override
  Future<void> write(String text) async {
    final f = await _file();
    // Written aside and renamed, so a crash leaves the old or the new.
    final tmp = File('${f.path}.tmp');
    await tmp.writeAsString(text, flush: true);
    await tmp.rename(f.path);
  }

  @override
  Future<String> setAside(String suffix) async {
    final f = await _file();
    final dir = f.parent.path;
    var aside = '$name.old-$suffix';
    for (var n = 2; await File('$dir/$aside').exists(); n++) {
      aside = '$name.old-$suffix-$n';
    }
    await f.rename('$dir/$aside');
    return aside;
  }
}

class MemoryStore extends Store {
  @override
  final String name;
  MemoryStore([this.name = 'memory']);

  String? text;
  final Map<String, String> aside = {};

  /// The stored state, decoded.
  Map<String, dynamic>? get state =>
      text == null ? null : jsonDecode(text!) as Map<String, dynamic>;

  @override
  Future<String?> read() async => text;

  @override
  Future<void> write(String t) async => text = t;

  @override
  Future<String> setAside(String suffix) async {
    var a = '$name.old-$suffix';
    for (var n = 2; aside.containsKey(a); n++) {
      a = '$name.old-$suffix-$n';
    }
    aside[a] = text!;
    text = null;
    return a;
  }
}
