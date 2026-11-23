// The app's languages (README.md, "Languages"): every translation says
// what the English says, with the same values put in.

import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:bark_app/l10n.dart';
import 'package:flutter_test/flutter_test.dart';

Map<String, dynamic> _arb(String language) =>
    jsonDecode(File('lib/l10n/app_$language.arb').readAsStringSync())
        as Map<String, dynamic>;

/// The values a message takes, by name: `{count}`, `{count, plural, …}`.
Set<String> _values(String message) => {
  for (final m in RegExp(r'\{(\w+)[,}]').allMatches(message)) m.group(1)!,
};

void main() {
  final english = _arb('en');
  final keys = english.keys.where((k) => !k.startsWith('@')).toSet();

  for (final language in ['sv', 'uk']) {
    test('$language says every message, with the same values', () {
      final other = _arb(language);
      final theirs = other.keys.where((k) => !k.startsWith('@')).toSet();
      expect(keys.difference(theirs), isEmpty, reason: 'not translated');
      expect(theirs.difference(keys), isEmpty, reason: 'not in English');
      for (final k in keys) {
        final declared =
            ((english['@$k'] as Map<String, dynamic>?)?['placeholders']
                    as Map<String, dynamic>?)
                ?.keys
                .toSet() ??
            <String>{};
        expect(_values(other[k] as String), declared, reason: k);
        expect((other[k] as String).trim(), isNotEmpty, reason: k);
      }
    });
  }

  test('the phone\'s first language the app has is spoken, else English', () {
    expect(
      languageOf(const [Locale('de'), Locale('uk', 'UA')]).languageCode,
      'uk',
    );
    expect(languageOf(const [Locale('sv', 'FI')]).languageCode, 'sv');
    expect(languageOf(const [Locale('de')]).languageCode, 'en');
    expect(languageOf(const []).languageCode, 'en');
    addTearDown(() => speak(const Locale('en')));
    speak(const Locale('sv'));
    expect(tr.settings, 'Inställningar');
    speak(const Locale('uk'));
    expect(tr.conversationsArchived(3), '3 в архіві');
  });
}
