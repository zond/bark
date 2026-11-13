// The phone test on a phone against a board (README.md, "The phone
// test"): the app as installed, with the person's own files, runs the
// five steps of `phone_steps.dart` and says each in the Android log, where
// `bark-fw/scripts/phonetest.py` reads it.

import 'dart:io';

import 'package:bark_app/app.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:path_provider/path_provider.dart';
import 'package:sodium/sodium.dart';

import 'phone_steps.dart';

/// The image the host puts in the app's files for the update, if any.
const imageFile = 'bark-phone-test.img';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  final host = PrintedHost(debugPrint);

  testWidgets('the phone test', (t) async {
    final Bench bench;
    try {
      bench = Bench(
        node: const String.fromEnvironment('BARK_TEST_NODE'),
        to: const String.fromEnvironment('BARK_TEST_TO'),
        pairing: Pairing.values.byName(
          const String.fromEnvironment('BARK_TEST_PAIR', defaultValue: 'any'),
        ),
        receiveWait: const Duration(
          seconds: int.fromEnvironment(
            'BARK_TEST_RECEIVE_S',
            defaultValue: 300,
          ),
        ),
      );
    } catch (e) {
      host.ended(0, 'setup', Outcome.fail, '$e');
      host.done(0, 5);
      fail('$e');
    }
    final app = await AppModel.onPhone(await SodiumInit.init());
    await app.load();
    final ok = await runSteps(
      t,
      app,
      bench,
      host,
      image: () async {
        final dir = await getApplicationSupportDirectory();
        final f = File('${dir.path}/$imageFile');
        return await f.exists() ? await f.readAsBytes() : null;
      },
    );
    await app.session.disconnect();
    expect(ok, isTrue);
  });
}
