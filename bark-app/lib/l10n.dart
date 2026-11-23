// The app's words in the phone's language (README.md, "Languages").
//
// One object holds them for the screens and for the parts with no screen,
// a notification or a status line made while the app is in the
// background. It is English until the app is told the phone's languages,
// so a test reads English.

import 'dart:ui';

import 'l10n/app_localizations.dart';
import 'l10n/app_localizations_en.dart';

export 'l10n/app_localizations.dart' show AppLocalizations;

/// The app's words in the language it now speaks.
AppLocalizations tr = AppLocalizationsEn();

/// The first of the phone's languages the app has, else English.
Locale languageOf(List<Locale> phone) {
  for (final want in phone) {
    for (final has in AppLocalizations.supportedLocales) {
      if (has.languageCode == want.languageCode) return has;
    }
  }
  return const Locale('en');
}

/// Speaks `language`, one of [AppLocalizations.supportedLocales].
void speak(Locale language) => tr = lookupAppLocalizations(language);
