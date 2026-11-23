import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_sv.dart';
import 'app_localizations_uk.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('sv'),
    Locale('uk'),
  ];

  /// No description provided for @contacts.
  ///
  /// In en, this message translates to:
  /// **'Contacts'**
  String get contacts;

  /// No description provided for @myCard.
  ///
  /// In en, this message translates to:
  /// **'My card'**
  String get myCard;

  /// No description provided for @archived.
  ///
  /// In en, this message translates to:
  /// **'Archived'**
  String get archived;

  /// No description provided for @nearbyNodes.
  ///
  /// In en, this message translates to:
  /// **'Nearby nodes'**
  String get nearbyNodes;

  /// No description provided for @settings.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get settings;

  /// No description provided for @manual.
  ///
  /// In en, this message translates to:
  /// **'Manual'**
  String get manual;

  /// No description provided for @undo.
  ///
  /// In en, this message translates to:
  /// **'Undo'**
  String get undo;

  /// No description provided for @conversationsNoBrowser.
  ///
  /// In en, this message translates to:
  /// **'No browser to open the manual'**
  String get conversationsNoBrowser;

  /// No description provided for @conversationsBackupTitle.
  ///
  /// In en, this message translates to:
  /// **'Write down your 12 backup words'**
  String get conversationsBackupTitle;

  /// No description provided for @conversationsBackupWhy.
  ///
  /// In en, this message translates to:
  /// **'If you lose this phone, they are the only way back to your contacts.'**
  String get conversationsBackupWhy;

  /// No description provided for @conversationsNone.
  ///
  /// In en, this message translates to:
  /// **'No conversations yet. Add a contact, or show your card.'**
  String get conversationsNone;

  /// The line under the conversations that leads to the archived ones.
  ///
  /// In en, this message translates to:
  /// **'{count} archived'**
  String conversationsArchived(int count);

  /// No description provided for @conversationsNew.
  ///
  /// In en, this message translates to:
  /// **'New conversation'**
  String get conversationsNew;

  /// No description provided for @conversationsWasArchived.
  ///
  /// In en, this message translates to:
  /// **'{name}: conversation archived'**
  String conversationsWasArchived(String name);

  /// A conversation's last text when the person sent it, with what became of it.
  ///
  /// In en, this message translates to:
  /// **'You: {text}, {state}'**
  String conversationsYou(String text, String state);

  /// No description provided for @startTitle.
  ///
  /// In en, this message translates to:
  /// **'Get started'**
  String get startTitle;

  /// No description provided for @startDone.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get startDone;

  /// No description provided for @startToDo.
  ///
  /// In en, this message translates to:
  /// **'To do'**
  String get startToDo;

  /// No description provided for @startName.
  ///
  /// In en, this message translates to:
  /// **'Your name'**
  String get startName;

  /// No description provided for @startNameHow.
  ///
  /// In en, this message translates to:
  /// **'So people know who you are.'**
  String get startNameHow;

  /// No description provided for @startNode.
  ///
  /// In en, this message translates to:
  /// **'A node'**
  String get startNode;

  /// No description provided for @startNodeHow.
  ///
  /// In en, this message translates to:
  /// **'Found by itself when one is near.'**
  String get startNodeHow;

  /// No description provided for @startContact.
  ///
  /// In en, this message translates to:
  /// **'Your first contact'**
  String get startContact;

  /// No description provided for @startContactHow.
  ///
  /// In en, this message translates to:
  /// **'Scan their card, or show them yours.'**
  String get startContactHow;

  /// No description provided for @barHearsNoOne.
  ///
  /// In en, this message translates to:
  /// **'{node} hears no one since its update'**
  String barHearsNoOne(String node);

  /// No description provided for @barHearsNoOneWhy.
  ///
  /// In en, this message translates to:
  /// **'Its new firmware may have broken its radio. Tap for the way back.'**
  String get barHearsNoOneWhy;

  /// No description provided for @barAppOut.
  ///
  /// In en, this message translates to:
  /// **'App version {version} is out'**
  String barAppOut(int version);

  /// No description provided for @barTapToInstall.
  ///
  /// In en, this message translates to:
  /// **'Tap to install.'**
  String get barTapToInstall;

  /// No description provided for @barDownloading.
  ///
  /// In en, this message translates to:
  /// **'Downloading…'**
  String get barDownloading;

  /// No description provided for @barDownloadingPercent.
  ///
  /// In en, this message translates to:
  /// **'Downloading… {percent}%'**
  String barDownloadingPercent(int percent);

  /// No description provided for @barDownloaded.
  ///
  /// In en, this message translates to:
  /// **'Downloaded. Tap to install.'**
  String get barDownloaded;

  /// No description provided for @barDownloadFailed.
  ///
  /// In en, this message translates to:
  /// **'Download failed. Tap to try again.'**
  String get barDownloadFailed;

  /// No description provided for @barFirmwareOut.
  ///
  /// In en, this message translates to:
  /// **'Node firmware {version} is out'**
  String barFirmwareOut(int version);

  /// No description provided for @barTapToUpdate.
  ///
  /// In en, this message translates to:
  /// **'Tap to update the node.'**
  String get barTapToUpdate;

  /// No description provided for @barNotThisVersion.
  ///
  /// In en, this message translates to:
  /// **'Not for this version'**
  String get barNotThisVersion;

  /// No description provided for @archivedNothing.
  ///
  /// In en, this message translates to:
  /// **'Nothing archived. Swipe a conversation or contact aside to archive it.'**
  String get archivedNothing;

  /// No description provided for @archivedConversations.
  ///
  /// In en, this message translates to:
  /// **'Conversations'**
  String get archivedConversations;

  /// No description provided for @archivedUnarchive.
  ///
  /// In en, this message translates to:
  /// **'Unarchive'**
  String get archivedUnarchive;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'sv', 'uk'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'sv':
      return AppLocalizationsSv();
    case 'uk':
      return AppLocalizationsUk();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
