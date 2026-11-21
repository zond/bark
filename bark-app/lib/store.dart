// Where the app keeps its queue, the messages it has received, the
// contacts and the person: the node forgets a message once the app
// confirms it, so the app must have it on disk first.
//
// Each file carries the version of its format. One the app cannot read,
// of another version or not parsing, is set aside under another name and
// the app starts that file afresh: nothing is overwritten, and the app
// always starts. A small file is one JSON object, written whole; one that
// grows with the messages is a log ([Store.loadLog]), a line a change.

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
  Future<T?> load<T>(
    int version,
    T Function(Map<String, dynamic>) parse,
  ) async {
    final text = await read();
    if (text == null) return null;
    Object? v;
    try {
      final j = jsonDecode(text) as Map<String, dynamic>;
      v = j['version'];
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

  /// Adds `text` to the end of the file, making it if there is none.
  Future<void> append(String text);

  /// The file read as a log: a first line `{"version": v}`, then one JSON
  /// value a line, each ended by a newline and each one change, so that a
  /// change costs a line and not the file. `parse` reads the lines after
  /// the first; a last line without its newline is a write cut off, and
  /// is left out. A file of a version in `older` is a whole JSON object,
  /// as files were before they were logs, which its function reads. Null,
  /// with the file set aside, as [load] does, for any other version or a
  /// fault; else what was read and whether to write the file afresh: it
  /// is of an older version or its last line was cut off.
  Future<({T value, bool afresh})?> loadLog<T>(
    int version,
    T Function(List<Object?> lines) parse, {
    Map<int, T Function(Map<String, dynamic>)> older = const {},
  }) async {
    final text = await read();
    if (text == null) return null;
    Object? v;
    try {
      final nl = text.indexOf('\n');
      final head = jsonDecode(
        nl < 0 ? text : text.substring(0, nl),
      ) as Map<String, dynamic>;
      v = head['version'];
      if (v is! int) throw FormatException('version $v');
      if (older[v] case final read?) return (value: read(head), afresh: true);
      if (v == version) {
        final lines = nl < 0 ? <String>[] : text.substring(nl + 1).split('\n');
        // What follows the last newline: nothing, or a line cut off.
        final cut = lines.isNotEmpty && lines.removeLast().isNotEmpty;
        return (
          value: parse([for (final l in lines) jsonDecode(l)]),
          afresh: cut,
        );
      }
    } catch (e) {
      await _setAside('bad', 'did not read: $e');
      return null;
    }
    await _setAside('$v', 'is of version $v, this app reads $version');
    return null;
  }

  /// The first line of a log of `version`, with `lines` after it.
  static String logText(int version, Iterable<Object?> lines) => [
    jsonEncode({'version': version}),
    for (final l in lines) jsonEncode(l),
    '',
  ].join('\n');

  /// One more line of a log.
  static String logLine(Object? line) => '${jsonEncode(line)}\n';
}

class FileStore extends Store {
  @override
  final String name;
  final Future<Directory> Function() _dir;
  FileStore(this.name, {Future<Directory> Function()? dir})
    : _dir = dir ?? getApplicationSupportDirectory;

  Future<File> _file() async => File('${(await _dir()).path}/$name');

  /// The write before, done or failed: two of one file run in turn, as
  /// each is written aside under one name, and the later one stays; an
  /// append waits for the write before it, so it lands in the new file.
  Future<void> _written = Future.value();

  @override
  Future<String?> read() async {
    final f = await _file();
    if (!await f.exists()) return null;
    return f.readAsString();
  }

  @override
  Future<void> write(String text) {
    final w = _written.then((_) => _write(text));
    _written = w.then((_) {}, onError: (_) {});
    return w;
  }

  @override
  Future<void> append(String text) {
    final w = _written.then((_) => _append(text));
    _written = w.then((_) {}, onError: (_) {});
    return w;
  }

  Future<void> _append(String text) async {
    final f = await (await _file()).open(mode: FileMode.append);
    try {
      await f.writeString(text);
      await f.flush();
    } finally {
      await f.close();
    }
  }

  Future<void> _write(String text) async {
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
  Future<void> append(String t) async => text = (text ?? '') + t;

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
