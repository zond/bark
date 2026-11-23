// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get contacts => 'Contacts';

  @override
  String get myCard => 'My card';

  @override
  String get archived => 'Archived';

  @override
  String get nearbyNodes => 'Nearby nodes';

  @override
  String get settings => 'Settings';

  @override
  String get manual => 'Manual';

  @override
  String get undo => 'Undo';

  @override
  String get conversationsNoBrowser => 'No browser to open the manual';

  @override
  String get conversationsBackupTitle => 'Write down your 12 backup words';

  @override
  String get conversationsBackupWhy =>
      'If you lose this phone, they are the only way back to your contacts.';

  @override
  String get conversationsNone =>
      'No conversations yet. Add a contact, or show your card.';

  @override
  String conversationsArchived(int count) {
    return '$count archived';
  }

  @override
  String get conversationsNew => 'New conversation';

  @override
  String conversationsWasArchived(String name) {
    return '$name: conversation archived';
  }

  @override
  String conversationsYou(String text, String state) {
    return 'You: $text, $state';
  }

  @override
  String get startTitle => 'Get started';

  @override
  String get startDone => 'Done';

  @override
  String get startToDo => 'To do';

  @override
  String get startName => 'Your name';

  @override
  String get startNameHow => 'So people know who you are.';

  @override
  String get startNode => 'A node';

  @override
  String get startNodeHow => 'Found by itself when one is near.';

  @override
  String get startContact => 'Your first contact';

  @override
  String get startContactHow => 'Scan their card, or show them yours.';

  @override
  String barHearsNoOne(String node) {
    return '$node hears no one since its update';
  }

  @override
  String get barHearsNoOneWhy =>
      'Its new firmware may have broken its radio. Tap for the way back.';

  @override
  String barAppOut(int version) {
    return 'App version $version is out';
  }

  @override
  String get barTapToInstall => 'Tap to install.';

  @override
  String get barDownloading => 'Downloading…';

  @override
  String barDownloadingPercent(int percent) {
    return 'Downloading… $percent%';
  }

  @override
  String get barDownloaded => 'Downloaded. Tap to install.';

  @override
  String get barDownloadFailed => 'Download failed. Tap to try again.';

  @override
  String barFirmwareOut(int version) {
    return 'Node firmware $version is out';
  }

  @override
  String get barTapToUpdate => 'Tap to update the node.';

  @override
  String get barNotThisVersion => 'Not for this version';

  @override
  String get archivedNothing =>
      'Nothing archived. Swipe a conversation or contact aside to archive it.';

  @override
  String get archivedConversations => 'Conversations';

  @override
  String get archivedUnarchive => 'Unarchive';
}
