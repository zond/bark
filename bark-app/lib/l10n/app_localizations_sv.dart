// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Swedish (`sv`).
class AppLocalizationsSv extends AppLocalizations {
  AppLocalizationsSv([String locale = 'sv']) : super(locale);

  @override
  String get contacts => 'Kontakter';

  @override
  String get myCard => 'Mitt kort';

  @override
  String get archived => 'Arkiverat';

  @override
  String get nearbyNodes => 'Noder i närheten';

  @override
  String get settings => 'Inställningar';

  @override
  String get manual => 'Handbok';

  @override
  String get undo => 'Ångra';

  @override
  String get conversationsNoBrowser => 'Ingen webbläsare att öppna handboken i';

  @override
  String get conversationsBackupTitle => 'Skriv ner dina 12 reservord';

  @override
  String get conversationsBackupWhy =>
      'Tappar du telefonen är de enda vägen tillbaka till dina kontakter.';

  @override
  String get conversationsNone =>
      'Inga samtal än. Lägg till en kontakt, eller visa ditt kort.';

  @override
  String conversationsArchived(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count arkiverade',
      one: '$count arkiverad',
    );
    return '$_temp0';
  }

  @override
  String get conversationsNew => 'Nytt samtal';

  @override
  String conversationsWasArchived(String name) {
    return '$name: samtalet arkiverat';
  }

  @override
  String conversationsYou(String text, String state) {
    return 'Du: $text, $state';
  }

  @override
  String get startTitle => 'Kom igång';

  @override
  String get startDone => 'Klart';

  @override
  String get startToDo => 'Att göra';

  @override
  String get startName => 'Ditt namn';

  @override
  String get startNameHow => 'Så att folk vet vem du är.';

  @override
  String get startNode => 'En nod';

  @override
  String get startNodeHow => 'Hittas av sig själv när en är nära.';

  @override
  String get startContact => 'Din första kontakt';

  @override
  String get startContactHow => 'Skanna deras kort, eller visa dem ditt.';

  @override
  String barHearsNoOne(String node) {
    return '$node hör ingen sedan uppdateringen';
  }

  @override
  String get barHearsNoOneWhy =>
      'Den nya programvaran kan ha förstört radion. Tryck för vägen tillbaka.';

  @override
  String barAppOut(int version) {
    return 'Appversion $version är ute';
  }

  @override
  String get barTapToInstall => 'Tryck för att installera.';

  @override
  String get barDownloading => 'Laddar ner…';

  @override
  String barDownloadingPercent(int percent) {
    return 'Laddar ner… $percent %';
  }

  @override
  String get barDownloaded => 'Nedladdad. Tryck för att installera.';

  @override
  String get barDownloadFailed =>
      'Nedladdningen misslyckades. Tryck för att försöka igen.';

  @override
  String barFirmwareOut(int version) {
    return 'Nodprogramvara $version är ute';
  }

  @override
  String get barTapToUpdate => 'Tryck för att uppdatera noden.';

  @override
  String get barNotThisVersion => 'Inte för den här versionen';

  @override
  String get archivedNothing =>
      'Inget arkiverat. Svep ett samtal eller en kontakt åt sidan för att arkivera.';

  @override
  String get archivedConversations => 'Samtal';

  @override
  String get archivedUnarchive => 'Ta ur arkivet';
}
