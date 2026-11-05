// Where the app keeps its queue and the messages it has received: the
// node forgets a message once the app confirms it, so the app must have
// it on disk first.

import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

abstract class Store {
  Future<Map<String, dynamic>?> load();
  Future<void> save(Map<String, dynamic> state);
}

class FileStore implements Store {
  final String name;
  FileStore(this.name);

  Future<File> _file() async =>
      File('${(await getApplicationSupportDirectory()).path}/$name');

  @override
  Future<Map<String, dynamic>?> load() async {
    final f = await _file();
    if (!await f.exists()) return null;
    return jsonDecode(await f.readAsString()) as Map<String, dynamic>;
  }

  @override
  Future<void> save(Map<String, dynamic> state) async {
    final f = await _file();
    // Written aside and renamed, so a crash leaves the old or the new.
    final tmp = File('${f.path}.tmp');
    await tmp.writeAsString(jsonEncode(state), flush: true);
    await tmp.rename(f.path);
  }
}

class MemoryStore implements Store {
  Map<String, dynamic>? state;

  @override
  Future<Map<String, dynamic>?> load() async => state;

  @override
  Future<void> save(Map<String, dynamic> s) async =>
      state = jsonDecode(jsonEncode(s)) as Map<String, dynamic>;
}
