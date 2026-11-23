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

  /// No description provided for @cancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get cancel;

  /// No description provided for @save.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get save;

  /// No description provided for @notNow.
  ///
  /// In en, this message translates to:
  /// **'Not now'**
  String get notNow;

  /// No description provided for @timeJustNow.
  ///
  /// In en, this message translates to:
  /// **'just now'**
  String get timeJustNow;

  /// No description provided for @timeMinutesAgo.
  ///
  /// In en, this message translates to:
  /// **'{minutes} min ago'**
  String timeMinutesAgo(int minutes);

  /// No description provided for @timeHoursAgo.
  ///
  /// In en, this message translates to:
  /// **'{hours} h ago'**
  String timeHoursAgo(int hours);

  /// How long ago, in lower case, as it follows other words.
  ///
  /// In en, this message translates to:
  /// **'{days, plural, one{{days} day ago} other{{days} days ago}}'**
  String timeDaysAgo(int days);

  /// When a person was last seen: never. Lower case, as it follows other words.
  ///
  /// In en, this message translates to:
  /// **'never'**
  String get timeNever;

  /// No description provided for @timeToday.
  ///
  /// In en, this message translates to:
  /// **'today'**
  String get timeToday;

  /// No description provided for @timeYesterday.
  ///
  /// In en, this message translates to:
  /// **'yesterday'**
  String get timeYesterday;

  /// A sent text's state. Lower case, as it follows other words; the app makes its first letter a capital where it stands alone.
  ///
  /// In en, this message translates to:
  /// **'waiting for a node'**
  String get sentQueued;

  /// This and the states after it are lower case, as sentQueued is.
  ///
  /// In en, this message translates to:
  /// **'being handed to the node'**
  String get sentWriting;

  /// No description provided for @sentTaken.
  ///
  /// In en, this message translates to:
  /// **'sent into the network'**
  String get sentTaken;

  /// No description provided for @sentBusyNow.
  ///
  /// In en, this message translates to:
  /// **'the network is busy, sending again now'**
  String get sentBusyNow;

  /// No description provided for @sentBusyIn.
  ///
  /// In en, this message translates to:
  /// **'the network is busy, sending again in {wait}'**
  String sentBusyIn(String wait);

  /// No description provided for @sentRefused.
  ///
  /// In en, this message translates to:
  /// **'refused by the node'**
  String get sentRefused;

  /// No description provided for @sentNoNeighbour.
  ///
  /// In en, this message translates to:
  /// **'the node hears no other, sending when it does'**
  String get sentNoNeighbour;

  /// No description provided for @sentNoWay.
  ///
  /// In en, this message translates to:
  /// **'found no way there'**
  String get sentNoWay;

  /// How many of a long text's parts the node has taken.
  ///
  /// In en, this message translates to:
  /// **'{taken} of {total} sent'**
  String sentPartsSent(int taken, int total);

  /// The state of one part of a long text, such as 'refused by the node'.
  ///
  /// In en, this message translates to:
  /// **'part {part}: {state}'**
  String sentPartProblem(int part, String state);

  /// No description provided for @sentQueuedHelp.
  ///
  /// In en, this message translates to:
  /// **'On this phone, until a node is linked and ready for it.'**
  String get sentQueuedHelp;

  /// No description provided for @sentWritingHelp.
  ///
  /// In en, this message translates to:
  /// **'The phone is giving it to the node.'**
  String get sentWritingHelp;

  /// No description provided for @sentTakenHelp.
  ///
  /// In en, this message translates to:
  /// **'The node has it and passes it on. The network does not say when it arrives; it gives it back if it finds no way there.'**
  String get sentTakenHelp;

  /// No description provided for @sentBusy.
  ///
  /// In en, this message translates to:
  /// **'The network is busy'**
  String get sentBusy;

  /// No description provided for @sentBusyHelp.
  ///
  /// In en, this message translates to:
  /// **'The network gave it back for now. The app sends it again by itself once the wait is over.'**
  String get sentBusyHelp;

  /// No description provided for @sentNoNeighbourHelp.
  ///
  /// In en, this message translates to:
  /// **'The node hears no other node now. The app sends it again by itself once it does.'**
  String get sentNoNeighbourHelp;

  /// Send again and Cancel are the buttons under the text.
  ///
  /// In en, this message translates to:
  /// **'The network found no way to their address: their node may be off or gone, or their address old. Send again tries once more, as a new message; Cancel removes it.'**
  String get sentNoWayHelp;

  /// Cancel is the button under the text; Debug is the screen of the app's log.
  ///
  /// In en, this message translates to:
  /// **'The node will not take it as it is, a fault of the app or the node. Cancel removes it; Debug shows the reason.'**
  String get sentRefusedHelp;

  /// No description provided for @sentReaction.
  ///
  /// In en, this message translates to:
  /// **'Their reaction'**
  String get sentReaction;

  /// No description provided for @sentReactionHelp.
  ///
  /// In en, this message translates to:
  /// **'The emoji on the text\'s corner, if they picked one.'**
  String get sentReactionHelp;

  /// No description provided for @receivedYourReaction.
  ///
  /// In en, this message translates to:
  /// **'Your reaction'**
  String get receivedYourReaction;

  /// No description provided for @receivedYourReactionHelp.
  ///
  /// In en, this message translates to:
  /// **'Press and hold a text to react with an emoji, or to copy it. They see it once it arrives. Pick another to change it.'**
  String get receivedYourReactionHelp;

  /// No description provided for @receivedFirst.
  ///
  /// In en, this message translates to:
  /// **'First message'**
  String get receivedFirst;

  /// No description provided for @receivedFirstHelp.
  ///
  /// In en, this message translates to:
  /// **'Anyone with your card could have sent a first message, so this one is not proven to be theirs.'**
  String get receivedFirstHelp;

  /// No description provided for @receivedMailbox.
  ///
  /// In en, this message translates to:
  /// **'From your mailbox'**
  String get receivedMailbox;

  /// No description provided for @receivedMailboxHelp.
  ///
  /// In en, this message translates to:
  /// **'It waited at your home node and came when you fetched your mail.'**
  String get receivedMailboxHelp;

  /// No description provided for @receivedParts.
  ///
  /// In en, this message translates to:
  /// **'Parts'**
  String get receivedParts;

  /// No description provided for @receivedPartsHelp.
  ///
  /// In en, this message translates to:
  /// **'A long text comes in parts, in any order. Italics stand for a part not yet here.'**
  String get receivedPartsHelp;

  /// No description provided for @receivedTime.
  ///
  /// In en, this message translates to:
  /// **'The time'**
  String get receivedTime;

  /// This message is the heading of the text's details on the same sheet (commonThisMessage).
  ///
  /// In en, this message translates to:
  /// **'When it came. When it was written is under This message.'**
  String get receivedTimeHelp;

  /// No description provided for @commonThisMessage.
  ///
  /// In en, this message translates to:
  /// **'This message'**
  String get commonThisMessage;

  /// Pairing with a node failed; the reason is a sentence without its full stop.
  ///
  /// In en, this message translates to:
  /// **'{reason}. Still a guest here.'**
  String commonNotPaired(String reason);

  /// No description provided for @commonPairingFailed.
  ///
  /// In en, this message translates to:
  /// **'Pairing failed: {error}. Still a guest here.'**
  String commonPairingFailed(String error);

  /// A contact made the person a home at their node.
  ///
  /// In en, this message translates to:
  /// **'{name} gave you a home at {node}'**
  String commonGift(String name, String node);

  /// No description provided for @commonGiftTitle.
  ///
  /// In en, this message translates to:
  /// **'A home given'**
  String get commonGiftTitle;

  /// No description provided for @commonGiftHelp.
  ///
  /// In en, this message translates to:
  /// **'The owner of a home node added you there. Used, it is your home: it keeps texts to you while you are away, and your card carries its address, so your contacts get your new card.\n\nA home you had before sends you its mail, then forgets you.'**
  String get commonGiftHelp;

  /// The button that takes a home another person gave.
  ///
  /// In en, this message translates to:
  /// **'Use it'**
  String get commonUseGift;

  /// No description provided for @failed.
  ///
  /// In en, this message translates to:
  /// **'Failed: {error}'**
  String failed(String error);

  /// No description provided for @commonHomeOffer.
  ///
  /// In en, this message translates to:
  /// **'No home node yet. Make this one your home? It keeps texts to you while you are away. You pair first, with the code on its screen.'**
  String get commonHomeOffer;

  /// A button: make the node the person's home.
  ///
  /// In en, this message translates to:
  /// **'Set home'**
  String get setHome;

  /// No description provided for @commonBluetoothSettings.
  ///
  /// In en, this message translates to:
  /// **'Bluetooth settings'**
  String get commonBluetoothSettings;

  /// No description provided for @commonBluetoothSettingsFailed.
  ///
  /// In en, this message translates to:
  /// **'Opening Bluetooth settings failed: {error}'**
  String commonBluetoothSettingsFailed(String error);

  /// No description provided for @commonFetchQueued.
  ///
  /// In en, this message translates to:
  /// **'Fetch waits for a node'**
  String get commonFetchQueued;

  /// No description provided for @commonFetchWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting for your home node to answer, {wait} at most'**
  String commonFetchWaiting(String wait);

  /// No description provided for @commonFetch.
  ///
  /// In en, this message translates to:
  /// **'Fetch my mail'**
  String get commonFetch;

  /// No description provided for @commonFetchFailed.
  ///
  /// In en, this message translates to:
  /// **'Fetching failed: {error}'**
  String commonFetchFailed(String error);

  /// No description provided for @commonCardNoHome.
  ///
  /// In en, this message translates to:
  /// **'Your card changed: no home'**
  String get commonCardNoHome;

  /// No description provided for @commonCardNewHome.
  ///
  /// In en, this message translates to:
  /// **'Your card changed: new home address'**
  String get commonCardNewHome;

  /// Under 'Your card changed': who still has the old card.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{Your contact has the old one.} other{Your {count} contacts have the old one.}}'**
  String commonCardOld(int count);

  /// No description provided for @commonCardQueued.
  ///
  /// In en, this message translates to:
  /// **'Your card is queued'**
  String get commonCardQueued;

  /// No description provided for @commonCardFailed.
  ///
  /// In en, this message translates to:
  /// **'Sending the card failed: {error}'**
  String commonCardFailed(String error);

  /// No description provided for @commonSendCard.
  ///
  /// In en, this message translates to:
  /// **'Send my card'**
  String get commonSendCard;

  /// No description provided for @commonMyName.
  ///
  /// In en, this message translates to:
  /// **'My name'**
  String get commonMyName;

  /// The tooltip of a button that opens help on a thing.
  ///
  /// In en, this message translates to:
  /// **'About {title}'**
  String commonAbout(String title);

  /// No description provided for @commonWhyThisNode.
  ///
  /// In en, this message translates to:
  /// **'Why this node'**
  String get commonWhyThisNode;

  /// Shown before Android asks to allow Bluetooth. bark is the app's name, always in lower case.
  ///
  /// In en, this message translates to:
  /// **'bark texts through a node near you, over Bluetooth. Android asks next.'**
  String get commonWhyBluetooth;

  /// No description provided for @commonContinue.
  ///
  /// In en, this message translates to:
  /// **'Continue'**
  String get commonContinue;

  /// What the phone looks for, put into statusLooking and statusBluetoothOff as {node}.
  ///
  /// In en, this message translates to:
  /// **'a public node'**
  String get statusPublicNode;

  /// No description provided for @statusLooking.
  ///
  /// In en, this message translates to:
  /// **'Not connected. Looking for {node}; texts wait on the phone'**
  String statusLooking(String node);

  /// No description provided for @statusBluetoothOff.
  ///
  /// In en, this message translates to:
  /// **'Not connected. Bluetooth is off: turn it on to reach {node}'**
  String statusBluetoothOff(String node);

  /// No description provided for @statusNotLooking.
  ///
  /// In en, this message translates to:
  /// **'Not connected. Texts wait on the phone'**
  String get statusNotLooking;

  /// No description provided for @statusGuestFor30s.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest for 30 s'**
  String statusGuestFor30s(String node);

  /// As statusGuestFor30s, while the node is about to install a new version of its firmware. So are the other status…Installing messages.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest for 30 s, installing version {version}'**
  String statusGuestFor30sInstalling(String node, int version);

  /// As statusGuestFor30s, for a minute after the node came back on a new version of its firmware. So are the other status…Updated messages.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest for 30 s, updated to version {version}'**
  String statusGuestFor30sUpdated(String node, int version);

  /// No description provided for @statusGuest.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest'**
  String statusGuest(String node);

  /// No description provided for @statusGuestInstalling.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest, installing version {version}'**
  String statusGuestInstalling(String node, int version);

  /// No description provided for @statusGuestUpdated.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest, updated to version {version}'**
  String statusGuestUpdated(String node, int version);

  /// No description provided for @statusAtHome.
  ///
  /// In en, this message translates to:
  /// **'Connected to your home, {node}'**
  String statusAtHome(String node);

  /// No description provided for @statusAtHomeInstalling.
  ///
  /// In en, this message translates to:
  /// **'Connected to your home, {node}, installing version {version}'**
  String statusAtHomeInstalling(String node, int version);

  /// No description provided for @statusAtHomeUpdated.
  ///
  /// In en, this message translates to:
  /// **'Connected to your home, {node}, updated to version {version}'**
  String statusAtHomeUpdated(String node, int version);

  /// No description provided for @statusPaired.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node}, paired'**
  String statusPaired(String node);

  /// No description provided for @statusPairedInstalling.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node}, paired, installing version {version}'**
  String statusPairedInstalling(String node, int version);

  /// No description provided for @statusPairedUpdated.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node}, paired, updated to version {version}'**
  String statusPairedUpdated(String node, int version);

  /// A sentence after the line that says which node the phone is on.
  ///
  /// In en, this message translates to:
  /// **'Home at {node}'**
  String statusHomeAt(String node);

  /// The node has just started; how long until it is ready.
  ///
  /// In en, this message translates to:
  /// **'Starting up, {wait}'**
  String statusStarting(String wait);

  /// No description provided for @statusHearsNone.
  ///
  /// In en, this message translates to:
  /// **'Hears no other node yet: texts wait here'**
  String get statusHearsNone;

  /// How many other nodes the connected node hears.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{Hears 1 node} other{Hears {count} nodes}}'**
  String statusHears(int count);

  /// A found node's signal. Lower case, as it follows other words.
  ///
  /// In en, this message translates to:
  /// **'signal unknown'**
  String get signalUnknown;

  /// No description provided for @signalStrong.
  ///
  /// In en, this message translates to:
  /// **'strong signal'**
  String get signalStrong;

  /// No description provided for @signalFair.
  ///
  /// In en, this message translates to:
  /// **'fair signal'**
  String get signalFair;

  /// No description provided for @signalWeak.
  ///
  /// In en, this message translates to:
  /// **'weak signal'**
  String get signalWeak;

  /// No description provided for @connectionLostOne.
  ///
  /// In en, this message translates to:
  /// **'{node} does not know this phone: pair again with it in Settings.'**
  String connectionLostOne(String node);

  /// No description provided for @connectionLostMany.
  ///
  /// In en, this message translates to:
  /// **'{nodes} do not know this phone: pair again with them in Settings.'**
  String connectionLostMany(String nodes);

  /// No description provided for @connectionUnservedOne.
  ///
  /// In en, this message translates to:
  /// **'{node} cannot be used yet: it or this app needs an update. Tap it in Settings to try again.'**
  String connectionUnservedOne(String node);

  /// No description provided for @connectionUnservedMany.
  ///
  /// In en, this message translates to:
  /// **'{nodes} cannot be used yet: they or this app need an update. Tap one in Settings to try again.'**
  String connectionUnservedMany(String nodes);

  /// No description provided for @connectionAtHome.
  ///
  /// In en, this message translates to:
  /// **'Your home. The phone stays linked while it is in reach, so texts come at once, also with the screen off. It always goes to your home first.'**
  String get connectionAtHome;

  /// No description provided for @connectionPaired.
  ///
  /// In en, this message translates to:
  /// **'A node you are paired with. The phone stays linked while it is in reach.'**
  String get connectionPaired;

  /// No description provided for @connectionPairedNoHome.
  ///
  /// In en, this message translates to:
  /// **'With no home, your card names this node while you use it.'**
  String get connectionPairedNoHome;

  /// Fetch my mail is the button commonFetch.
  ///
  /// In en, this message translates to:
  /// **'Texts to you wait at your home; Fetch my mail brings them here.'**
  String get connectionPairedHome;

  /// No description provided for @connectionMovesHome.
  ///
  /// In en, this message translates to:
  /// **'When your home is in reach, the phone moves to it within a minute.'**
  String get connectionMovesHome;

  /// No description provided for @connectionTappedGuest.
  ///
  /// In en, this message translates to:
  /// **'Someone else\'s public node, which you chose. It serves one guest at a time and ends a guest\'s link after 30 seconds; then the phone chooses a node itself.'**
  String get connectionTappedGuest;

  /// No description provided for @connectionGuestWeak.
  ///
  /// In en, this message translates to:
  /// **'Your paired nodes are heard too weakly here, so the phone uses this public node until it hears one well.'**
  String get connectionGuestWeak;

  /// No description provided for @connectionGuestAway.
  ///
  /// In en, this message translates to:
  /// **'None of your paired nodes has been in reach for 30 seconds, so the phone uses this public node until it hears one.'**
  String get connectionGuestAway;

  /// No description provided for @connectionGuestNoneNear.
  ///
  /// In en, this message translates to:
  /// **'No node of yours is near, so the phone uses this public node, someone else\'s.'**
  String get connectionGuestNoneNear;

  /// No description provided for @connectionGuestVisits.
  ///
  /// In en, this message translates to:
  /// **'As a guest it visits about once a minute, and at once when you send. Replies to texts sent from here come here.'**
  String get connectionGuestVisits;

  /// No description provided for @connectionBluetoothOff.
  ///
  /// In en, this message translates to:
  /// **'Bluetooth is off. Texts wait on the phone until it is on and a node is in reach.'**
  String get connectionBluetoothOff;

  /// No description provided for @connectionLookPublic.
  ///
  /// In en, this message translates to:
  /// **'No node of yours to use, so the phone looks every 15 seconds for a public node to use as a guest.'**
  String get connectionLookPublic;

  /// No description provided for @connectionLookPaired.
  ///
  /// In en, this message translates to:
  /// **'The phone looks every 15 seconds for your paired nodes, your home first. After 30 seconds without one, it uses a public node as a guest meanwhile.'**
  String get connectionLookPaired;

  /// No description provided for @connectionNotLooking.
  ///
  /// In en, this message translates to:
  /// **'The phone is not looking: you stopped it, or it may not use Bluetooth yet. Tap a node in Settings to connect.'**
  String get connectionNotLooking;

  /// The body of the notification that keeps the app running while it is on a node.
  ///
  /// In en, this message translates to:
  /// **'Texts arrive while the screen is off'**
  String get notifyTextsArrive;

  /// No description provided for @notifyChecksMinute.
  ///
  /// In en, this message translates to:
  /// **'Checks for texts about once a minute'**
  String get notifyChecksMinute;

  /// Under the title that says which node the phone looks for: it is that node.
  ///
  /// In en, this message translates to:
  /// **'Texts waiting go out once it is in reach'**
  String get notifyOutInReach;

  /// No description provided for @notifyOutBluetooth.
  ///
  /// In en, this message translates to:
  /// **'Texts waiting go out once Bluetooth is on'**
  String get notifyOutBluetooth;

  /// A notification's title for a text from a contact with no name.
  ///
  /// In en, this message translates to:
  /// **'Unnamed'**
  String get notifyUnnamed;

  /// The name of the kind of notification, in the phone's settings.
  ///
  /// In en, this message translates to:
  /// **'Texts'**
  String get notifyChannel;

  /// No description provided for @notifyChannelAbout.
  ///
  /// In en, this message translates to:
  /// **'A text from a contact'**
  String get notifyChannelAbout;

  /// The name, in the phone's settings, of the kind of notification shown while the app keeps its link to the node; notifyLinkChannelAbout describes it there.
  ///
  /// In en, this message translates to:
  /// **'Connection to the node'**
  String get notifyLinkChannel;

  /// No description provided for @notifyLinkChannelAbout.
  ///
  /// In en, this message translates to:
  /// **'Shown while the app keeps the link to a node or looks for it'**
  String get notifyLinkChannelAbout;

  /// The person: in a notification of a conversation, among those whose home a node on the map is, and the dot for the phone on the map.
  ///
  /// In en, this message translates to:
  /// **'You'**
  String get you;

  /// No description provided for @threadRemovedContact.
  ///
  /// In en, this message translates to:
  /// **'Removed contact'**
  String get threadRemovedContact;

  /// Stands in a received text where one of its parts has not come.
  ///
  /// In en, this message translates to:
  /// **'part {part} of {total} not yet here'**
  String threadPartMissing(int part, int total);

  /// The words of the mark on a received text that came as a first message, read as its tooltip.
  ///
  /// In en, this message translates to:
  /// **'Came as a first message, which anyone could have made'**
  String get threadUnverifiedMark;

  /// A sent text's details: the node it went to.
  ///
  /// In en, this message translates to:
  /// **'To node'**
  String get threadToNode;

  /// A sent text's details: the position of the node it went to.
  ///
  /// In en, this message translates to:
  /// **'At'**
  String get threadAt;

  /// A text's details: how it was encrypted, such as threadSealedForThem.
  ///
  /// In en, this message translates to:
  /// **'Sealed'**
  String get threadSealed;

  /// How a sent text was encrypted, after threadSealed. Lower case, as it follows other words; so are the other threadSealed… values.
  ///
  /// In en, this message translates to:
  /// **'for them alone'**
  String get threadSealedForThem;

  /// No description provided for @threadSealedIntroOut.
  ///
  /// In en, this message translates to:
  /// **'as a first message, with your card'**
  String get threadSealedIntroOut;

  /// How a received text was encrypted.
  ///
  /// In en, this message translates to:
  /// **'by them, for you alone'**
  String get threadSealedForYou;

  /// No description provided for @threadSealedIntroIn.
  ///
  /// In en, this message translates to:
  /// **'to you, as a first message'**
  String get threadSealedIntroIn;

  /// A text's details: the label of its parts' message IDs; count is how many.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{Message ID} other{Message IDs}}'**
  String threadMessageIds(int count);

  /// A received text's details: when the sender wrote it.
  ///
  /// In en, this message translates to:
  /// **'Written'**
  String get threadWritten;

  /// A received text's details: how many of its parts have come, as threadPartsOf.
  ///
  /// In en, this message translates to:
  /// **'Parts here'**
  String get threadPartsHere;

  /// No description provided for @threadPartsOf.
  ///
  /// In en, this message translates to:
  /// **'{here} of {total}'**
  String threadPartsOf(int here, int total);

  /// The title of the choice where a text goes.
  ///
  /// In en, this message translates to:
  /// **'Send to'**
  String get threadSendTo;

  /// {ago} is how long ago, such as timeMinutesAgo.
  ///
  /// In en, this message translates to:
  /// **'Last seen there {ago}'**
  String threadLastSeenThere(String ago);

  /// Send the text to where the contact last was. {ago} is how long ago, such as timeMinutesAgo.
  ///
  /// In en, this message translates to:
  /// **'Where {name} was {ago}'**
  String threadWhereWas(String name, String ago);

  /// Under the choice threadWhereWas: the text goes to the node where the contact last was.
  ///
  /// In en, this message translates to:
  /// **'Fast if they are still near there'**
  String get threadThereHow;

  /// One of the two places a text can be sent to; the other is threadWhereWas.
  ///
  /// In en, this message translates to:
  /// **'Their home node'**
  String get threadTheirHome;

  /// Under the choice threadTheirHome: the text goes to the contact's home node.
  ///
  /// In en, this message translates to:
  /// **'Waits at their home until they fetch it'**
  String get threadHomeHow;

  /// No description provided for @threadNotSent.
  ///
  /// In en, this message translates to:
  /// **'Not sent: {error}'**
  String threadNotSent(String error);

  /// Under the contact's name at the top of a conversation.
  ///
  /// In en, this message translates to:
  /// **'Has not written yet'**
  String get threadNotWritten;

  /// Under the contact's name. {ago} is how long ago, such as timeMinutesAgo.
  ///
  /// In en, this message translates to:
  /// **'Last wrote {ago}'**
  String threadLastWrote(String ago);

  /// No description provided for @threadLastWroteHome.
  ///
  /// In en, this message translates to:
  /// **'Last wrote {ago}, from their home'**
  String threadLastWroteHome(String ago);

  /// No description provided for @threadLastWroteAway.
  ///
  /// In en, this message translates to:
  /// **'Last wrote {ago}, away from home'**
  String threadLastWroteAway(String ago);

  /// No description provided for @threadRename.
  ///
  /// In en, this message translates to:
  /// **'Rename'**
  String get threadRename;

  /// No description provided for @threadShareHome.
  ///
  /// In en, this message translates to:
  /// **'Share my home node'**
  String get threadShareHome;

  /// No description provided for @threadArchiveConversation.
  ///
  /// In en, this message translates to:
  /// **'Archive conversation'**
  String get threadArchiveConversation;

  /// No description provided for @contactArchived.
  ///
  /// In en, this message translates to:
  /// **'{name}: contact archived'**
  String contactArchived(String name);

  /// No description provided for @threadArchiveContact.
  ///
  /// In en, this message translates to:
  /// **'Archive contact'**
  String get threadArchiveContact;

  /// No description provided for @threadRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove {name}?'**
  String threadRemoveTitle(String name);

  /// No description provided for @threadRemoveHow.
  ///
  /// In en, this message translates to:
  /// **'Their texts stay; to write again, add their card.'**
  String get threadRemoveHow;

  /// A button that removes a contact, a phone's record or the person's home, in the dialog that asks or on the phone's row.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get remove;

  /// No description provided for @threadRemoveContact.
  ///
  /// In en, this message translates to:
  /// **'Remove contact'**
  String get threadRemoveContact;

  /// No description provided for @threadNotVerified.
  ///
  /// In en, this message translates to:
  /// **'Not verified yet: their first reply proves it is them.'**
  String get threadNotVerified;

  /// No description provided for @threadIsArchived.
  ///
  /// In en, this message translates to:
  /// **'This conversation is archived.'**
  String get threadIsArchived;

  /// No description provided for @threadWasRemoved.
  ///
  /// In en, this message translates to:
  /// **'This contact was removed.'**
  String get threadWasRemoved;

  /// No description provided for @threadNoNode.
  ///
  /// In en, this message translates to:
  /// **'{name}\'s card names no node yet: ask them for a new one.'**
  String threadNoNode(String name);

  /// A contact's name: the title of the dialog that renames one, and the label of the field in Add contact.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get name;

  /// Under the field being written: how many messages the text takes, two or more.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} part} other{{count} parts}}'**
  String threadParts(int count);

  /// As threadParts, when the text takes more messages than {max}, the most it may.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} part, {max} at most} other{{count} parts, {max} at most}}'**
  String threadTooManyParts(int count, int max);

  /// The hint in the empty field where the person writes a text.
  ///
  /// In en, this message translates to:
  /// **'Text'**
  String get threadHint;

  /// The send button's tooltip.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get threadSend;

  /// {emoji} is the emoji's name, in English.
  ///
  /// In en, this message translates to:
  /// **'They reacted: {emoji}'**
  String threadTheyReacted(String emoji);

  /// {emoji} is the emoji's name, in English.
  ///
  /// In en, this message translates to:
  /// **'You reacted: {emoji}'**
  String threadYouReacted(String emoji);

  /// No description provided for @threadCopy.
  ///
  /// In en, this message translates to:
  /// **'Copy text'**
  String get threadCopy;

  /// The mark on a sent text that went to where the contact last was.
  ///
  /// In en, this message translates to:
  /// **'To where they were'**
  String get threadToWhere;

  /// A mark on a sent text, as threadToWhere is: where it was sent.
  ///
  /// In en, this message translates to:
  /// **'To their home'**
  String get threadToHome;

  /// The button under a sent text whose search found no way: it sends the text once more.
  ///
  /// In en, this message translates to:
  /// **'Send again'**
  String get threadSendAgain;

  /// The mark on a sent text the node has taken.
  ///
  /// In en, this message translates to:
  /// **'Sent into the network, which does not say when it arrives'**
  String get threadTakenMark;

  /// No description provided for @threadReactionSeen.
  ///
  /// In en, this message translates to:
  /// **'{name} sees your reaction on their text.'**
  String threadReactionSeen(String name);

  /// The emoji's names are in English only, so a search finds English words.
  ///
  /// In en, this message translates to:
  /// **'Search emoji by name'**
  String get emojiSearch;

  /// No description provided for @emojiNone.
  ///
  /// In en, this message translates to:
  /// **'No emoji by that name.'**
  String get emojiNone;

  /// Unicode's groups of emoji, as the picker shows them.
  ///
  /// In en, this message translates to:
  /// **'Smileys & emotion'**
  String get emojiSmileys;

  /// No description provided for @emojiPeople.
  ///
  /// In en, this message translates to:
  /// **'People & body'**
  String get emojiPeople;

  /// No description provided for @emojiAnimals.
  ///
  /// In en, this message translates to:
  /// **'Animals & nature'**
  String get emojiAnimals;

  /// No description provided for @emojiFood.
  ///
  /// In en, this message translates to:
  /// **'Food & drink'**
  String get emojiFood;

  /// No description provided for @emojiTravel.
  ///
  /// In en, this message translates to:
  /// **'Travel & places'**
  String get emojiTravel;

  /// No description provided for @emojiActivities.
  ///
  /// In en, this message translates to:
  /// **'Activities'**
  String get emojiActivities;

  /// No description provided for @emojiObjects.
  ///
  /// In en, this message translates to:
  /// **'Objects'**
  String get emojiObjects;

  /// No description provided for @emojiSymbols.
  ///
  /// In en, this message translates to:
  /// **'Symbols'**
  String get emojiSymbols;

  /// No description provided for @emojiFlags.
  ///
  /// In en, this message translates to:
  /// **'Flags'**
  String get emojiFlags;

  /// Why a text was not sent; lower case, as it follows other words, such as threadNotSent's.
  ///
  /// In en, this message translates to:
  /// **'no address to answer to yet'**
  String get sendNoReturnAddress;

  /// Why a text was not sent; follows other words, such as threadNotSent's.
  ///
  /// In en, this message translates to:
  /// **'{name} has no address yet'**
  String sendNoAddress(String name);

  /// Why a text was not sent; lower case, as it follows other words.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{too long: {count} part, {max} at most} other{too long: {count} parts, {max} at most}}'**
  String sendTooLong(int count, int max);

  /// Why a text cannot be sent, under the field and after threadNotSent's words; lower case.
  ///
  /// In en, this message translates to:
  /// **'a character too long to send'**
  String get sendCharTooLong;

  /// A notice: the network found no way to a contact's address for a text, which waits in its thread for Send again or Cancel.
  ///
  /// In en, this message translates to:
  /// **'Your text to {name} found no way there.'**
  String sendTextNoWay(String name);

  /// A notice: the network found no way to a contact's address for the person's reaction; it is not sent again by itself.
  ///
  /// In en, this message translates to:
  /// **'Your reaction {emoji} found no way to {name}.'**
  String sendReactionNoWay(String emoji, String name);

  /// A notice: the network found no way to a contact's address for the person's card; it is not sent again by itself.
  ///
  /// In en, this message translates to:
  /// **'Your card found no way to {name}.'**
  String sendCardNoWay(String name);

  /// A notice: the network found no way to a contact's address for the home the person gave them; it is not sent again by itself.
  ///
  /// In en, this message translates to:
  /// **'The home you gave {name} found no way to them.'**
  String sendHomeNoWay(String name);

  /// No description provided for @sessionBadValue.
  ///
  /// In en, this message translates to:
  /// **'The node sent a value this app cannot read: {error}'**
  String sessionBadValue(String error);

  /// No description provided for @sessionReadFailed.
  ///
  /// In en, this message translates to:
  /// **'Reading the node failed: {error}'**
  String sessionReadFailed(String error);

  /// Why something failed; lower case, as it follows words such as "Setting Public failed: ".
  ///
  /// In en, this message translates to:
  /// **'not connected'**
  String get notConnected;

  /// Why something failed for a guest. The value is the English name of what was asked of the node, such as Public or Phones.
  ///
  /// In en, this message translates to:
  /// **'{what} is the paired phone\'s alone'**
  String sessionPairedOnly(String what);

  /// No description provided for @sessionHomeTaken.
  ///
  /// In en, this message translates to:
  /// **'Your home no longer keeps your mail. Set it again in Settings.'**
  String get sessionHomeTaken;

  /// {id} is a message ID in hex.
  ///
  /// In en, this message translates to:
  /// **'The node gave back message {id}, which this app did not send.'**
  String sessionGivenBackUnknown(String id);

  /// A text, or a command to the home node, could not be sent.
  ///
  /// In en, this message translates to:
  /// **'Sending failed: {error}'**
  String sendFailed(String error);

  /// No description provided for @sessionConfirmRefused.
  ///
  /// In en, this message translates to:
  /// **'The node refused the inbox confirmation: {error}'**
  String sessionConfirmRefused(String error);

  /// No description provided for @sessionInboxFailed.
  ///
  /// In en, this message translates to:
  /// **'Reading the inbox failed: {error}'**
  String sessionInboxFailed(String error);

  /// No description provided for @sessionPlaceFailed.
  ///
  /// In en, this message translates to:
  /// **'Setting the node\'s place failed: {error}'**
  String sessionPlaceFailed(String error);

  /// What the home node answered a fetch of the mail.
  ///
  /// In en, this message translates to:
  /// **'No new mail'**
  String get sessionNoNewMail;

  /// After a fetch of the mail: how many texts the home node still keeps beyond those it sent.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} more at home: fetch again for them} other{{count} more at home: fetch again for them}}'**
  String sessionMoreAtHome(int count);

  /// The node's clock, on the debug screen: it has no time. {offset} is its offset from UTC, such as UTC+01:00.
  ///
  /// In en, this message translates to:
  /// **'no time, offset {offset}'**
  String sessionClockNone(String offset);

  /// The node's clock, set from its GPS. {time} is the UTC time in ISO 8601.
  ///
  /// In en, this message translates to:
  /// **'{time} from the gps, offset {offset}'**
  String sessionClockGps(String time, String offset);

  /// The node's clock, set from a phone.
  ///
  /// In en, this message translates to:
  /// **'{time} from the phone, offset {offset}'**
  String sessionClockPhone(String time, String offset);

  /// The label of a card as text: the one pasted when adding a contact, or the person's own in My card's details.
  ///
  /// In en, this message translates to:
  /// **'Card'**
  String get card;

  /// The button that opens the camera to scan another person's card.
  ///
  /// In en, this message translates to:
  /// **'Scan their card'**
  String get scanTheirCard;

  /// Add contact is the button contactsAdd.
  ///
  /// In en, this message translates to:
  /// **'No contacts yet. Tap Add contact and scan their card.'**
  String get contactsNone;

  /// The line under the contacts that leads to the archived ones; count is how many contacts.
  ///
  /// In en, this message translates to:
  /// **'{count} archived'**
  String contactsArchived(int count);

  /// The button that adds a contact, and the title of the screen it opens.
  ///
  /// In en, this message translates to:
  /// **'Add contact'**
  String get contactsAdd;

  /// Under a contact's name in the list: the contact came by a first message and has not yet replied sealed.
  ///
  /// In en, this message translates to:
  /// **'Not verified yet'**
  String get contactsNotVerified;

  /// Under a contact's name in the list.
  ///
  /// In en, this message translates to:
  /// **'Has a home node'**
  String get contactsHasHome;

  /// Under a contact's name: their card is temporary (it names no home). Lower case, after an hourglass icon.
  ///
  /// In en, this message translates to:
  /// **'temporary: no node known yet'**
  String get contactsTemporaryNoNode;

  /// As contactsTemporaryNoNode, with how long ago they were last known at their node; {ago} is such as timeMinutesAgo or timeJustNow.
  ///
  /// In en, this message translates to:
  /// **'temporary: at the node they used {ago}'**
  String contactsTemporaryAt(String ago);

  /// Under the field where a card's text is pasted, below the button scanTheirCard.
  ///
  /// In en, this message translates to:
  /// **'Or paste the card they sent you'**
  String get contactsPasteHelp;

  /// A heading: the heading of Settings' section on the person's home, and a pasted card's home node, with its address under it. Also a row's label on the debug screen.
  ///
  /// In en, this message translates to:
  /// **'Home node'**
  String get homeNode;

  /// Under homeNode: the card names no home node and no node at all. Lower case, as it follows that title.
  ///
  /// In en, this message translates to:
  /// **'none, and no node yet: ask them for a new card'**
  String get contactsNoHomeNoNode;

  /// Under homeNode: the card names no home node. Lower case, as it follows that title.
  ///
  /// In en, this message translates to:
  /// **'none: a temporary card'**
  String get contactsNoHomeTemporary;

  /// Under the field, when the pasted text is not a card. {error} is why, a sentence without its full stop, such as cardNot; {prefix} is BARK:.
  ///
  /// In en, this message translates to:
  /// **'{error}. Paste a card: it starts with {prefix}'**
  String contactsBadPaste(String error, String prefix);

  /// A contact whose card has no name and whom the person has not named. {start} is the first 8 hex digits of their identity.
  ///
  /// In en, this message translates to:
  /// **'Unnamed ({start}…)'**
  String contactsUnnamed(String start);

  /// The button that sends the person's card through the phone's share sheet.
  ///
  /// In en, this message translates to:
  /// **'Share'**
  String get cardShare;

  /// No description provided for @cardShareFailed.
  ///
  /// In en, this message translates to:
  /// **'Sharing failed: {error}'**
  String cardShareFailed(String error);

  /// In place of the person's name, when they have none yet.
  ///
  /// In en, this message translates to:
  /// **'not set'**
  String get nameNotSet;

  /// The tooltip of the button that changes the person's name.
  ///
  /// In en, this message translates to:
  /// **'Change'**
  String get changeName;

  /// A section on My card that opens to the card's parts as text.
  ///
  /// In en, this message translates to:
  /// **'Details'**
  String get cardDetails;

  /// The person's identity, the public key their texts are sealed to: the label of its hex on My card, and the heading of Settings' section that saves it to a file.
  ///
  /// In en, this message translates to:
  /// **'My identity'**
  String get myIdentity;

  /// A button's tooltip; cardIdentityCopied says it is done. The other cardCopy… and …Copied messages are the same for the other rows.
  ///
  /// In en, this message translates to:
  /// **'Copy the identity'**
  String get cardCopyIdentity;

  /// No description provided for @cardIdentityCopied.
  ///
  /// In en, this message translates to:
  /// **'The identity copied'**
  String get cardIdentityCopied;

  /// No description provided for @cardHomeAddress.
  ///
  /// In en, this message translates to:
  /// **'Home address'**
  String get cardHomeAddress;

  /// No description provided for @cardCopyAddress.
  ///
  /// In en, this message translates to:
  /// **'Copy the address'**
  String get cardCopyAddress;

  /// No description provided for @cardAddressCopied.
  ///
  /// In en, this message translates to:
  /// **'The address copied'**
  String get cardAddressCopied;

  /// The address of the node the person's phone uses now, which a temporary card names.
  ///
  /// In en, this message translates to:
  /// **'The node I use now'**
  String get cardNodeNow;

  /// No description provided for @cardCopyNode.
  ///
  /// In en, this message translates to:
  /// **'Copy the node'**
  String get cardCopyNode;

  /// No description provided for @cardNodeCopied.
  ///
  /// In en, this message translates to:
  /// **'The node copied'**
  String get cardNodeCopied;

  /// No description provided for @cardCopyCard.
  ///
  /// In en, this message translates to:
  /// **'Copy the card'**
  String get cardCopyCard;

  /// No description provided for @cardCardCopied.
  ///
  /// In en, this message translates to:
  /// **'The card copied'**
  String get cardCardCopied;

  /// No description provided for @cardNameFirst.
  ///
  /// In en, this message translates to:
  /// **'Set your name first: whoever scans your card needs to know it.'**
  String get cardNameFirst;

  /// No description provided for @cardSetName.
  ///
  /// In en, this message translates to:
  /// **'Set my name'**
  String get cardSetName;

  /// No description provided for @cardTemporaryHow.
  ///
  /// In en, this message translates to:
  /// **'Temporary card: with no home node, it names the node you use now. Set a home in Settings.'**
  String get cardTemporaryHow;

  /// Under the QR code of a card with a home node: the card holds that node's address.
  ///
  /// In en, this message translates to:
  /// **'It carries your home node\'s address.'**
  String get cardHomeHow;

  /// What a screen reader says for the hourglass that marks a card with no home.
  ///
  /// In en, this message translates to:
  /// **'Temporary card'**
  String get cardTemporary;

  /// The word under a temporary card's QR code, in capitals; it describes the card.
  ///
  /// In en, this message translates to:
  /// **'TEMPORARY'**
  String get cardTemporaryMark;

  /// Why a scanned or pasted text was not read as a card; it may be followed by other words, as in contactsBadPaste. So are the other card… reasons.
  ///
  /// In en, this message translates to:
  /// **'Not a card'**
  String get cardNot;

  /// No description provided for @cardNewer.
  ///
  /// In en, this message translates to:
  /// **'A card from a newer app (card version {version})'**
  String cardNewer(int version);

  /// No description provided for @cardOlder.
  ///
  /// In en, this message translates to:
  /// **'A card from an older app (card version {version})'**
  String cardOlder(int version);

  /// No description provided for @cardNameNotUtf8.
  ///
  /// In en, this message translates to:
  /// **'Not a card: the name is not UTF-8'**
  String get cardNameNotUtf8;

  /// Why the person's card cannot be made: their name is longer than a card holds.
  ///
  /// In en, this message translates to:
  /// **'The name is too long'**
  String get cardNameTooLong;

  /// The title of the camera screen that reads a card's QR code.
  ///
  /// In en, this message translates to:
  /// **'Scan a card'**
  String get scanTitle;

  /// No description provided for @scanNoCamera.
  ///
  /// In en, this message translates to:
  /// **'The camera is not allowed. Allow it in the phone\'s settings, or paste the card.'**
  String get scanNoCamera;

  /// No description provided for @scanPoint.
  ///
  /// In en, this message translates to:
  /// **'Point the camera at the card on the other phone'**
  String get scanPoint;

  /// The help under the row Role of a node's sheet on the map; the other fact messages are the help of the rows they name.
  ///
  /// In en, this message translates to:
  /// **'A relay has outside power, such as USB, and listens all the time, passing on texts for the nodes around it: the more relays, the farther texts reach. A sleepy node is on battery: to last, it sleeps and wakes only in short windows, and sends and receives through one relay near it, its anchor. A fixed relay that loses its power keeps relaying on battery until the battery runs low, and then turns sleepy. Holding the node\'s button switches its role until its power next changes.'**
  String get factRole;

  /// No description provided for @factMoving.
  ///
  /// In en, this message translates to:
  /// **'The node tells this itself from its GPS: after a little over a minute of positions away from where it stood, it counts as carried, and after four hours in one place it stays put again. A carried node\'s place follows its GPS, and it keeps no new mail for others, as it may soon be gone. Setting its place in the app\'s Settings makes it stay put at once.'**
  String get factMoving;

  /// No description provided for @factAnchor.
  ///
  /// In en, this message translates to:
  /// **'A sleepy node sends and receives through one relay near it, its anchor. The anchor keeps the sleepy node\'s mail and hands it over in the short window after each time it wakes, and the sleepy node\'s address is its anchor\'s place. A sleepy node takes the first relay it hears as its anchor; with none yet it holds and passes on texts itself, at a higher cost to the channel.'**
  String get factAnchor;

  /// No description provided for @factPlace.
  ///
  /// In en, this message translates to:
  /// **'Texts find a node by its place: an address is a position and the node\'s ID, and each relay passes a text on toward that position. The place comes from the node\'s GPS, unless a person set it in the app\'s Settings, which wins, as for a relay indoors with no view of the sky. A node with no place yet sends no heartbeat, so no text can find it until it has one.'**
  String get factPlace;

  /// No description provided for @factPower.
  ///
  /// In en, this message translates to:
  /// **'USB means outside power, on which a node can relay all the time. On battery the figure is read from the cell\'s voltage, so it is rough; on this node\'s own sheet it is the raw reading in millivolts. At about a fifth left the battery counts as low, and a relay on it turns sleepy to save the rest.'**
  String get factPower;

  /// No description provided for @factLink.
  ///
  /// In en, this message translates to:
  /// **'How well this node hears the other one: how far its signal is above the radio noise here. Below about 4 dB a full text is often lost when the signal fades, so the link is fragile and used only when no other is left; from about 7 dB it is good; between, fair. It is measured one way: the other node may hear this one better or worse.'**
  String get factLink;

  /// No description provided for @factLastHeard.
  ///
  /// In en, this message translates to:
  /// **'Every node sends a short heartbeat now and then, saying who and where it is. A relay sends one every 5 to 15 seconds, each on the next of 13 channels, so this node hears it about once every one to three minutes; a sleepy node wakes to send one every two and a half to five minutes. A relay not heard for six and a half minutes, or a sleepy node for ten, leaves the list.'**
  String get factLastHeard;

  /// No description provided for @factDistance.
  ///
  /// In en, this message translates to:
  /// **'The straight line from this node\'s place to the other\'s, worked out by the app from the two positions, so it is shown only when both have a place. How far radio reaches depends on height and on what stands between: relays on rooftops hear each other at about 3 km.'**
  String get factDistance;

  /// No description provided for @factKnown.
  ///
  /// In en, this message translates to:
  /// **'What the node says of its own neighbours, in three counts. Heard: the nodes whose heartbeats it hears. Strong: of those, the ones heard well enough that a full text gets through a fade, so they likely hear it too. Exchanged: of those, the ones that have taken a text from it, which proves they hear it.'**
  String get factKnown;

  /// No description provided for @factGpsSignal.
  ///
  /// In en, this message translates to:
  /// **'How strongly the node\'s GPS hears the satellites, in dB-Hz, as its last reading had it. Under {floor} the node takes no position from it; an open sky gives about 40. Move a node to where it reads higher, or set its place by hand.'**
  String factGpsSignal(int floor);

  /// No description provided for @factVersion.
  ///
  /// In en, this message translates to:
  /// **'The version of the firmware the node runs. A new version spreads by itself: a node given it, from a phone or over USB, passes it to the relays around it a piece at a time, leaving the channels to texts. A node takes only a newer version signed by the project, and while it gathers one this row says which and how many of its blocks it holds.'**
  String get factVersion;

  /// No description provided for @factExchanged.
  ///
  /// In en, this message translates to:
  /// **'This node has handed it a text and it took it. That proves the link works both ways: it hears this node, as well as being heard.'**
  String get factExchanged;

  /// No description provided for @factDemoted.
  ///
  /// In en, this message translates to:
  /// **'It did not answer this node\'s last three tries to pass it a text, so it may no longer hear this node. Until it answers again, or for six and a half minutes, it is tried only after every other neighbour with a good enough link.'**
  String get factDemoted;

  /// No description provided for @factAvoided.
  ///
  /// In en, this message translates to:
  /// **'It turned a text down, asking for a wait, or did not answer one. Nothing is sent to it for a while; texts go round it meanwhile, and wait for it only where there is no other way.'**
  String get factAvoided;

  /// No description provided for @factPaired.
  ///
  /// In en, this message translates to:
  /// **'This phone is paired with this node: the node sends your texts into the network, and keeps the ones for you until the app reads them. Only the paired phone sees the node\'s neighbours on this map.'**
  String get factPaired;

  /// The label of a row of a node's sheet on the map; the other fact…Label messages are the labels of the other rows, each with its help in the fact message of the same name without Label.
  ///
  /// In en, this message translates to:
  /// **'Role'**
  String get factRoleLabel;

  /// The row's value is factCarried or factFixed.
  ///
  /// In en, this message translates to:
  /// **'Moving'**
  String get factMovingLabel;

  /// No description provided for @factAnchorLabel.
  ///
  /// In en, this message translates to:
  /// **'Anchor'**
  String get factAnchorLabel;

  /// No description provided for @factPlaceLabel.
  ///
  /// In en, this message translates to:
  /// **'Place'**
  String get factPlaceLabel;

  /// The row's value is USB, or the battery's level.
  ///
  /// In en, this message translates to:
  /// **'Power'**
  String get factPowerLabel;

  /// How well this node hears the other one; the value is factLinkGood, factLinkFair or factLinkFragile.
  ///
  /// In en, this message translates to:
  /// **'Link'**
  String get factLinkLabel;

  /// No description provided for @factLastHeardLabel.
  ///
  /// In en, this message translates to:
  /// **'Last heard'**
  String get factLastHeardLabel;

  /// No description provided for @factDistanceLabel.
  ///
  /// In en, this message translates to:
  /// **'Distance'**
  String get factDistanceLabel;

  /// The neighbours the node knows; the value is factKnownValue.
  ///
  /// In en, this message translates to:
  /// **'Known'**
  String get factKnownLabel;

  /// A row's label on a node's sheet on the map and on the debug screen: how strongly the node's GPS hears the satellites. Also what a screen reader says for the row's icon.
  ///
  /// In en, this message translates to:
  /// **'GPS signal'**
  String get gpsSignal;

  /// The version of the firmware the node runs.
  ///
  /// In en, this message translates to:
  /// **'Version'**
  String get factVersionLabel;

  /// The other node has taken a text from this one; the value is factExchangedValue.
  ///
  /// In en, this message translates to:
  /// **'Exchanged'**
  String get factExchangedLabel;

  /// This node tries the other one last for now; the value is factDemotedValue.
  ///
  /// In en, this message translates to:
  /// **'Demoted'**
  String get factDemotedLabel;

  /// This node sends nothing to the other one for now; the value is factAvoidedValue.
  ///
  /// In en, this message translates to:
  /// **'Avoided'**
  String get factAvoidedLabel;

  /// On the sheet of the node the phone is on; the value is factPairedWithThis.
  ///
  /// In en, this message translates to:
  /// **'Paired'**
  String get factPairedLabel;

  /// What a screen reader says for the icon of the row factRoleLabel; lower case. The other fact…Name messages are the same for the other rows' icons.
  ///
  /// In en, this message translates to:
  /// **'role, sleepy'**
  String get factRoleSleepyName;

  /// No description provided for @factRoleRelayName.
  ///
  /// In en, this message translates to:
  /// **'role, relay'**
  String get factRoleRelayName;

  /// No description provided for @factMovingName.
  ///
  /// In en, this message translates to:
  /// **'moving'**
  String get factMovingName;

  /// No description provided for @factAnchorName.
  ///
  /// In en, this message translates to:
  /// **'anchor'**
  String get factAnchorName;

  /// No description provided for @factPlaceName.
  ///
  /// In en, this message translates to:
  /// **'place'**
  String get factPlaceName;

  /// No description provided for @factPlaceByPersonName.
  ///
  /// In en, this message translates to:
  /// **'place, set by a person'**
  String get factPlaceByPersonName;

  /// No description provided for @factPlaceFromGpsName.
  ///
  /// In en, this message translates to:
  /// **'place, from GPS'**
  String get factPlaceFromGpsName;

  /// The node's place is its anchor's.
  ///
  /// In en, this message translates to:
  /// **'place, its anchor\'s'**
  String get factPlaceAnchorsName;

  /// For a node on USB.
  ///
  /// In en, this message translates to:
  /// **'power'**
  String get factPowerName;

  /// For a node on battery.
  ///
  /// In en, this message translates to:
  /// **'battery'**
  String get factBatteryName;

  /// No description provided for @factLinkGoodName.
  ///
  /// In en, this message translates to:
  /// **'link strength, good'**
  String get factLinkGoodName;

  /// No description provided for @factLinkFairName.
  ///
  /// In en, this message translates to:
  /// **'link strength, fair'**
  String get factLinkFairName;

  /// No description provided for @factLinkFragileName.
  ///
  /// In en, this message translates to:
  /// **'link strength, fragile'**
  String get factLinkFragileName;

  /// No description provided for @factLastHeardName.
  ///
  /// In en, this message translates to:
  /// **'last heard'**
  String get factLastHeardName;

  /// No description provided for @factDistanceName.
  ///
  /// In en, this message translates to:
  /// **'distance'**
  String get factDistanceName;

  /// No description provided for @factKnownName.
  ///
  /// In en, this message translates to:
  /// **'neighbours known'**
  String get factKnownName;

  /// No description provided for @factVersionName.
  ///
  /// In en, this message translates to:
  /// **'firmware version'**
  String get factVersionName;

  /// No description provided for @factPairedName.
  ///
  /// In en, this message translates to:
  /// **'paired'**
  String get factPairedName;

  /// No description provided for @factExchangedName.
  ///
  /// In en, this message translates to:
  /// **'exchanged'**
  String get factExchangedName;

  /// No description provided for @factDemotedName.
  ///
  /// In en, this message translates to:
  /// **'demoted'**
  String get factDemotedName;

  /// No description provided for @factAvoidedName.
  ///
  /// In en, this message translates to:
  /// **'avoided'**
  String get factAvoidedName;

  /// The value of the row factRoleLabel, lower case after the label; 'its' is the node's. The other row values are lower case too.
  ///
  /// In en, this message translates to:
  /// **'sleepy: wakes in windows, speaks through its anchor'**
  String get factRoleSleepy;

  /// No description provided for @factRoleRelay.
  ///
  /// In en, this message translates to:
  /// **'relay: passes on others\' texts'**
  String get factRoleRelay;

  /// The value of the row factMovingLabel: the node moves with a person. Also a node's kind after its ID in the list of nodes at one place, as mapKindSleepy.
  ///
  /// In en, this message translates to:
  /// **'carried'**
  String get factCarried;

  /// The value of the row factMovingLabel: the node stays in one place.
  ///
  /// In en, this message translates to:
  /// **'stays put'**
  String get factFixed;

  /// The value of the row factAnchorLabel, which stands before it: a sleepy node has an anchor, the relay it sends and receives through.
  ///
  /// In en, this message translates to:
  /// **'anchored'**
  String get factAnchored;

  /// As factAnchored, with the anchor's ID; after the label factAnchorLabel.
  ///
  /// In en, this message translates to:
  /// **'anchored to {node}'**
  String factAnchoredTo(String node);

  /// The value of the row factAnchorLabel, which stands before it.
  ///
  /// In en, this message translates to:
  /// **'no anchor yet'**
  String get factNoAnchor;

  /// This neighbour is the anchor of the node the phone is on.
  ///
  /// In en, this message translates to:
  /// **'this node\'s anchor'**
  String get factThisNodesAnchor;

  /// The value of the row factPlaceLabel, which stands before it.
  ///
  /// In en, this message translates to:
  /// **'no place yet'**
  String get factNoPlace;

  /// The place was set by hand in the app; the position follows on a line of its own.
  ///
  /// In en, this message translates to:
  /// **'set by a person\n{place}'**
  String factPlaceByPerson(String place);

  /// No description provided for @factPlaceFromGps.
  ///
  /// In en, this message translates to:
  /// **'from GPS\n{place}'**
  String factPlaceFromGps(String place);

  /// A sleepy node's place is its anchor's; 'its' is the node's.
  ///
  /// In en, this message translates to:
  /// **'its anchor\'s\n{place}'**
  String factPlaceAnchors(String place);

  /// The battery's level.
  ///
  /// In en, this message translates to:
  /// **'low'**
  String get factBatteryLow;

  /// A value the app has not read yet, such as a battery's level on the map or a value on the debug screen; lower case.
  ///
  /// In en, this message translates to:
  /// **'not read'**
  String get notRead;

  /// The battery left.
  ///
  /// In en, this message translates to:
  /// **'{percent}%'**
  String factPercent(int percent);

  /// A battery's voltage, on the map and on the debug screen.
  ///
  /// In en, this message translates to:
  /// **'{voltage} mV'**
  String millivolts(int voltage);

  /// The GPS gave no signal reading; the value of the row gpsSignal, on the map and on the debug screen.
  ///
  /// In en, this message translates to:
  /// **'no reading'**
  String get noReading;

  /// The GPS's signal, too weak for the node to take a position from it.
  ///
  /// In en, this message translates to:
  /// **'{signal} dB-Hz · too weak for a fix'**
  String factGpsTooWeak(int signal);

  /// No description provided for @factGpsEnough.
  ///
  /// In en, this message translates to:
  /// **'{signal} dB-Hz · enough for a fix'**
  String factGpsEnough(int signal);

  /// A link's quality; the link (factLinkLabel) is the other node's signal at this one. Also in the map's legend.
  ///
  /// In en, this message translates to:
  /// **'good'**
  String get factLinkGood;

  /// No description provided for @factLinkFair.
  ///
  /// In en, this message translates to:
  /// **'fair'**
  String get factLinkFair;

  /// No description provided for @factLinkFragile.
  ///
  /// In en, this message translates to:
  /// **'fragile'**
  String get factLinkFragile;

  /// Under the help of the row factLinkLabel.
  ///
  /// In en, this message translates to:
  /// **'This link: {rssi} dBm, {margin} dB over the noise floor.'**
  String factLinkDetail(String rssi, String margin);

  /// How long ago, in lower case, as timeMinutesAgo.
  ///
  /// In en, this message translates to:
  /// **'{seconds} s ago'**
  String timeSecondsAgo(int seconds);

  /// A distance under a kilometre.
  ///
  /// In en, this message translates to:
  /// **'{metres} m'**
  String factMetres(int metres);

  /// No description provided for @factKilometres.
  ///
  /// In en, this message translates to:
  /// **'{kilometres} km'**
  String factKilometres(String kilometres);

  /// The three counts of the node's neighbours that factKnown explains: Heard, Strong, Exchanged.
  ///
  /// In en, this message translates to:
  /// **'{heard} heard · {strong} strong · {exchanged} exchanged'**
  String factKnownValue(int heard, int strong, int exchanged);

  /// The firmware version the node runs, and a newer one it gathers over the radio with how many of its blocks it holds.
  ///
  /// In en, this message translates to:
  /// **'{version} · getting {gathers} {complete}/{blocks}'**
  String factVersionGetting(
    String version,
    String gathers,
    int complete,
    int blocks,
  );

  /// 'It' is the other node.
  ///
  /// In en, this message translates to:
  /// **'it took a message from this node'**
  String get factExchangedValue;

  /// 'It' is the other node; 'its last texts' are the last texts this node passed it.
  ///
  /// In en, this message translates to:
  /// **'it did not answer its last texts'**
  String get factDemotedValue;

  /// 'It' is the other node.
  ///
  /// In en, this message translates to:
  /// **'for now: it turned a text down or did not answer'**
  String get factAvoidedValue;

  /// No description provided for @factPairedWithThis.
  ///
  /// In en, this message translates to:
  /// **'with this phone'**
  String get factPairedWithThis;

  /// The tooltip of a button on the map.
  ///
  /// In en, this message translates to:
  /// **'Show where I am'**
  String get mapShowMe;

  /// The tooltip of the map's refresh button: read the neighbours again.
  ///
  /// In en, this message translates to:
  /// **'Read again'**
  String get mapReadAgain;

  /// No description provided for @mapNotConnected.
  ///
  /// In en, this message translates to:
  /// **'Not connected to a node: connect to see its neighbours.'**
  String get mapNotConnected;

  /// No description provided for @mapGuest.
  ///
  /// In en, this message translates to:
  /// **'Connected as a guest: only the phone paired with this node sees its neighbours.'**
  String get mapGuest;

  /// No description provided for @mapReadFailed.
  ///
  /// In en, this message translates to:
  /// **'Reading the neighbours failed: {error}'**
  String mapReadFailed(String error);

  /// No description provided for @mapNothing.
  ///
  /// In en, this message translates to:
  /// **'The node has no place yet and has heard no neighbour: nothing to draw.'**
  String get mapNothing;

  /// The credit the street map's makers require, in the map's corner.
  ///
  /// In en, this message translates to:
  /// **'© OpenStreetMap contributors'**
  String get mapAttribution;

  /// No description provided for @mapNoPlaceNoLines.
  ///
  /// In en, this message translates to:
  /// **'The node has no place of its own: its neighbours are drawn without lines to it.'**
  String get mapNoPlaceNoLines;

  /// No description provided for @mapNoPlaceAtYou.
  ///
  /// In en, this message translates to:
  /// **'The node has no place of its own: it is drawn where you are.'**
  String get mapNoPlaceAtYou;

  /// No description provided for @mapNoTiles.
  ///
  /// In en, this message translates to:
  /// **'No street map here: the phone has no internet and has not kept this area from before. The nodes and links are drawn on a blank background.'**
  String get mapNoTiles;

  /// No description provided for @mapLastReadFailed.
  ///
  /// In en, this message translates to:
  /// **'The last read failed: {error}'**
  String mapLastReadFailed(String error);

  /// A node's kind after its ID in the list of nodes at one place, as factCarried and mapKindRelay.
  ///
  /// In en, this message translates to:
  /// **'sleepy'**
  String get mapKindSleepy;

  /// No description provided for @mapKindRelay.
  ///
  /// In en, this message translates to:
  /// **'relay'**
  String get mapKindRelay;

  /// The node the phone is on: its kind after its ID in the list of nodes at one place, and what a screen reader says for its marker.
  ///
  /// In en, this message translates to:
  /// **'this node'**
  String get mapThisNode;

  /// On a neighbour's sheet; 'it' is the neighbour, asked over the radio.
  ///
  /// In en, this message translates to:
  /// **'Asking it for power, neighbours and version…'**
  String get mapAsking;

  /// 'It' is the neighbour, a sleepy node.
  ///
  /// In en, this message translates to:
  /// **'It did not answer: it sleeps between its windows.'**
  String get mapNoAnswerSleepy;

  /// 'It' is the neighbour.
  ///
  /// In en, this message translates to:
  /// **'It did not answer.'**
  String get mapNoAnswer;

  /// The title of the list of nodes that share one place on the map; count is two or more.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} node here} other{{count} nodes here}}'**
  String mapNodesHere(int count);

  /// What a screen reader says for a marker that stands for several nodes.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} node here: {names}} other{{count} nodes here: {names}}}'**
  String mapNodesHereNamed(int count, String names);

  /// The legend's first line: how many neighbours the node knows, and when the app read them.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, one{{count} neighbour, read {time}} other{{count} neighbours, read {time}}}'**
  String mapNeighboursRead(int count, String time);

  /// In the legend: a good link's margin over the noise.
  ///
  /// In en, this message translates to:
  /// **'good ≥ {good} dB'**
  String mapBandGood(String good);

  /// No description provided for @mapBandFair.
  ///
  /// In en, this message translates to:
  /// **'fair {fair}–{good} dB'**
  String mapBandFair(String fair, String good);

  /// No description provided for @mapBandFragile.
  ///
  /// In en, this message translates to:
  /// **'fragile < {fair} dB over the noise floor'**
  String mapBandFragile(String fair);

  /// In the legend, beside the icon of a relay; lower case, as the legend's other words.
  ///
  /// In en, this message translates to:
  /// **'a relay'**
  String get mapLegendRelay;

  /// No description provided for @mapLegendSleepy.
  ///
  /// In en, this message translates to:
  /// **'a sleepy node'**
  String get mapLegendSleepy;

  /// No description provided for @mapLegendCarried.
  ///
  /// In en, this message translates to:
  /// **'a carried node'**
  String get mapLegendCarried;

  /// In the legend, beside a count on a marker.
  ///
  /// In en, this message translates to:
  /// **'nodes at one place'**
  String get mapLegendShared;

  /// In the legend, beside the dot for the phone.
  ///
  /// In en, this message translates to:
  /// **'you: your position stays on this phone, never sent or kept'**
  String get mapLegendYou;

  /// Under a node's ID: the people whose home it is, such as you and contacts.
  ///
  /// In en, this message translates to:
  /// **'Home: {names}'**
  String mapHomeOf(String names);

  /// The person, in the list of those whose home a node on the map is, after mapHomeOf's "Home:": in lower case where the language writes it so in a list.
  ///
  /// In en, this message translates to:
  /// **'You'**
  String get mapYou;

  /// Under a node's ID: the contacts last seen at it, as names alone or as mapSeenAgo.
  ///
  /// In en, this message translates to:
  /// **'Last seen: {names}'**
  String mapLastSeen(String names);

  /// A contact and when they were last seen at the node, in mapLastSeen. {ago} is such as timeMinutesAgo.
  ///
  /// In en, this message translates to:
  /// **'{name} {ago}'**
  String mapSeenAgo(String name, String ago);

  /// The button that closes a dialog that only tells something.
  ///
  /// In en, this message translates to:
  /// **'OK'**
  String get ok;

  /// The screen of the app's log, and the row in Settings that opens it.
  ///
  /// In en, this message translates to:
  /// **'Debug'**
  String get debug;

  /// A heading on the debug screen: this phone's own values.
  ///
  /// In en, this message translates to:
  /// **'Me'**
  String get debugMe;

  /// A heading on the debug screen: tools for testing nodes on the bench.
  ///
  /// In en, this message translates to:
  /// **'Bench'**
  String get debugBench;

  /// A heading on the debug screen: the node's status; the spec calls this value Status.
  ///
  /// In en, this message translates to:
  /// **'Status'**
  String get debugStatus;

  /// A heading on the debug screen: the message the node gave back for now; the spec calls this value Given back.
  ///
  /// In en, this message translates to:
  /// **'Given back'**
  String get debugGivenBackHeading;

  /// No description provided for @debugConnectPaired.
  ///
  /// In en, this message translates to:
  /// **'Connect a paired node to pick a firmware file.'**
  String get debugConnectPaired;

  /// A button that opens the update screen with a file picker.
  ///
  /// In en, this message translates to:
  /// **'Pick a firmware file'**
  String get debugPickFirmware;

  /// The label of the row under the heading debugGivenBackHeading: the value the node gave.
  ///
  /// In en, this message translates to:
  /// **'Value'**
  String get debugValue;

  /// The value of debugValue when the node gives nothing back.
  ///
  /// In en, this message translates to:
  /// **'empty'**
  String get debugEmpty;

  /// A message the node gave back, why, and the wait it asks before sending it again.
  ///
  /// In en, this message translates to:
  /// **'{id}, {why}, wait {wait} s'**
  String debugGivenBack(String id, String why, int wait);

  /// A heading on the debug screen: the messages sent, as raw records that stay English.
  ///
  /// In en, this message translates to:
  /// **'Sent'**
  String get debugSent;

  /// A heading on the debug screen, as debugSent.
  ///
  /// In en, this message translates to:
  /// **'Received'**
  String get debugReceived;

  /// A heading on the debug screen: the app's log, which stays English.
  ///
  /// In en, this message translates to:
  /// **'Log'**
  String get debugLog;

  /// Under debugSent or debugReceived: no messages.
  ///
  /// In en, this message translates to:
  /// **'None'**
  String get debugNone;

  /// Under debugLog: no lines.
  ///
  /// In en, this message translates to:
  /// **'Nothing yet'**
  String get debugNothingYet;

  /// A row's label: the start of the person's identity in hex.
  ///
  /// In en, this message translates to:
  /// **'Identity'**
  String get debugIdentity;

  /// A row's label: the home address the person's card carries.
  ///
  /// In en, this message translates to:
  /// **'Card address'**
  String get debugCardAddress;

  /// The value of debugCardAddress; lower case.
  ///
  /// In en, this message translates to:
  /// **'none: no home'**
  String get debugNoneNoHome;

  /// A row's value: there is none; lower case.
  ///
  /// In en, this message translates to:
  /// **'none'**
  String get debugNoValue;

  /// A row's label: how many texts the home node still keeps for the person.
  ///
  /// In en, this message translates to:
  /// **'Mailbox: unsent at home'**
  String get debugUnsentAtHome;

  /// A row's label: how many fetches of the mail the app has made.
  ///
  /// In en, this message translates to:
  /// **'Fetch rounds'**
  String get debugFetchRounds;

  /// A row's label: the number of the last message this phone sent.
  ///
  /// In en, this message translates to:
  /// **'Last sent'**
  String get debugLastSent;

  /// No description provided for @debugAppVersion.
  ///
  /// In en, this message translates to:
  /// **'App version'**
  String get debugAppVersion;

  /// The value of debugAppVersion: the app was built by hand, not by a release; lower case.
  ///
  /// In en, this message translates to:
  /// **'development build'**
  String get debugDevBuild;

  /// A row's label: the newest release the app knows of.
  ///
  /// In en, this message translates to:
  /// **'Latest release'**
  String get debugLatestRelease;

  /// The value of debugLatestRelease before any is known; lower case.
  ///
  /// In en, this message translates to:
  /// **'not known'**
  String get debugNotKnown;

  /// The value of debugLatestRelease: its number, its app's version, its node firmware's version, and when the app last looked.
  ///
  /// In en, this message translates to:
  /// **'{release}, app {app}, firmware {firmware}, checked {checked}'**
  String debugRelease(int release, int app, int firmware, String checked);

  /// A row's label: the node's name over Bluetooth, such as bark-fa4e00000001.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get debugName;

  /// No description provided for @debugNodeId.
  ///
  /// In en, this message translates to:
  /// **'Node ID'**
  String get debugNodeId;

  /// A row's label: whose Bluetooth link this is, debugLinkGuest or debugLinkPaired.
  ///
  /// In en, this message translates to:
  /// **'Link'**
  String get debugLink;

  /// Lower case: the link is a guest's.
  ///
  /// In en, this message translates to:
  /// **'a guest\'s'**
  String get debugLinkGuest;

  /// No description provided for @debugLinkPaired.
  ///
  /// In en, this message translates to:
  /// **'the paired phone\'s'**
  String get debugLinkPaired;

  /// A row's label: the version of the link's rules the node speaks.
  ///
  /// In en, this message translates to:
  /// **'Link version'**
  String get debugLinkVersion;

  /// A row's label: the version of the firmware the node runs.
  ///
  /// In en, this message translates to:
  /// **'Firmware image'**
  String get debugFirmwareImage;

  /// A row's label: the node's clock, as sessionClockNone.
  ///
  /// In en, this message translates to:
  /// **'Clock'**
  String get debugClock;

  /// A row's label: whether the node serves phones not paired with it, debugYes or debugNo.
  ///
  /// In en, this message translates to:
  /// **'Public'**
  String get debugPublic;

  /// A row's value; lower case.
  ///
  /// In en, this message translates to:
  /// **'yes'**
  String get debugYes;

  /// A row's value; lower case.
  ///
  /// In en, this message translates to:
  /// **'no'**
  String get debugNo;

  /// The value of debugPublic on a guest's link: a guest finds only public nodes.
  ///
  /// In en, this message translates to:
  /// **'yes, as a guest finds it'**
  String get debugPublicGuest;

  /// A row's label: what the node keeps of homes, in English words.
  ///
  /// In en, this message translates to:
  /// **'Home'**
  String get debugHome;

  /// A row's label: the node's address, in hex.
  ///
  /// In en, this message translates to:
  /// **'Address'**
  String get debugAddress;

  /// The value of debugAddress; lower case.
  ///
  /// In en, this message translates to:
  /// **'none yet: no place'**
  String get debugNoPlace;

  /// No description provided for @debugAddressCopied.
  ///
  /// In en, this message translates to:
  /// **'Address copied'**
  String get debugAddressCopied;

  /// Under the heading debugStatus: the node's status has not been read.
  ///
  /// In en, this message translates to:
  /// **'Not read'**
  String get debugStatusNotRead;

  /// A row's label under debugStatus; its value is debugYes or debugNo, as are those of the rows after it up to debugPlaced.
  ///
  /// In en, this message translates to:
  /// **'Ready to take a message'**
  String get debugReady;

  /// No description provided for @debugExternalPower.
  ///
  /// In en, this message translates to:
  /// **'External power'**
  String get debugExternalPower;

  /// No description provided for @debugSleepy.
  ///
  /// In en, this message translates to:
  /// **'Sleepy'**
  String get debugSleepy;

  /// No description provided for @debugCarried.
  ///
  /// In en, this message translates to:
  /// **'Carried'**
  String get debugCarried;

  /// The node's place was set by hand in the app.
  ///
  /// In en, this message translates to:
  /// **'Position from a map point'**
  String get debugMapPoint;

  /// The node has a place.
  ///
  /// In en, this message translates to:
  /// **'Placed'**
  String get debugPlaced;

  /// A row's label under debugStatus: a sleepy node that hears no relay to take as its anchor; the spec calls it anchorless.
  ///
  /// In en, this message translates to:
  /// **'Anchorless'**
  String get debugAnchorless;

  /// A row's label under debugStatus: the node's first round of its channels after start-up; the spec calls it the first rotation. Its value is debugRotationRunning or debugRotationOver.
  ///
  /// In en, this message translates to:
  /// **'First rotation'**
  String get debugFirstRotation;

  /// The value of the row debugFirstRotation: the node's first round of its channels after start-up runs.
  ///
  /// In en, this message translates to:
  /// **'running, {left} to go'**
  String debugRotationRunning(String left);

  /// The value of the row debugFirstRotation: it is over.
  ///
  /// In en, this message translates to:
  /// **'over'**
  String get debugRotationOver;

  /// No description provided for @debugRelaysKnown.
  ///
  /// In en, this message translates to:
  /// **'Relays known'**
  String get debugRelaysKnown;

  /// No description provided for @debugNeighboursHeard.
  ///
  /// In en, this message translates to:
  /// **'Neighbours heard'**
  String get debugNeighboursHeard;

  /// A row's label: the position the node's address carries.
  ///
  /// In en, this message translates to:
  /// **'Address position'**
  String get debugAddressPosition;

  /// A row's label: the node's battery voltage.
  ///
  /// In en, this message translates to:
  /// **'Battery'**
  String get debugBattery;

  /// The value of debugBattery; lower case.
  ///
  /// In en, this message translates to:
  /// **'not read or on USB'**
  String get debugBatteryNone;

  /// A row's label: the messages the node holds for this phone.
  ///
  /// In en, this message translates to:
  /// **'Messages in the inbox'**
  String get debugInbox;

  /// The GPS's signal, the value of the row gpsSignal on the debug screen.
  ///
  /// In en, this message translates to:
  /// **'{signal} dB-Hz'**
  String dbHz(int signal);

  /// A button: pair anew, by the code on the node's screen, with a node that no longer knows this phone.
  ///
  /// In en, this message translates to:
  /// **'Pair again'**
  String get pairAgain;

  /// A sentence after one that says a node was unpaired or forgotten: that node is still the person's home node.
  ///
  /// In en, this message translates to:
  /// **'It stays your home.'**
  String get staysYourHome;

  /// After the app unpaired or forgot a node: Android's own pairing with it is still there. Shown above a button to Bluetooth settings.
  ///
  /// In en, this message translates to:
  /// **'Forget the node in Bluetooth settings too: the phone kept its pairing.'**
  String get forgetInBluetoothToo;

  /// What a screen reader says when Settings opens scrolled to the backup.
  ///
  /// In en, this message translates to:
  /// **'Backup: write down your 12 words'**
  String get settingsBackupAnnounce;

  /// A button that sets the node's place to where the phone is.
  ///
  /// In en, this message translates to:
  /// **'Use this phone\'s location'**
  String get settingsPlaceHere;

  /// Under settingsPlaceHere, once it has set the node's place.
  ///
  /// In en, this message translates to:
  /// **'Place set to {place}.'**
  String settingsPlaceSet(String place);

  /// The place typed is not two numbers in degrees.
  ///
  /// In en, this message translates to:
  /// **'Latitude and longitude in degrees, please'**
  String get settingsDegreesPlease;

  /// Under the person's name in Settings.
  ///
  /// In en, this message translates to:
  /// **'On your card, for others to see'**
  String get settingsNameHow;

  /// Under the row Debug in Settings; the link is the phone's Bluetooth link with the node.
  ///
  /// In en, this message translates to:
  /// **'What the link says, for a bug report'**
  String get settingsDebugHow;

  /// A heading in Settings: the paired node the phone is connected to, and its own settings.
  ///
  /// In en, this message translates to:
  /// **'This node'**
  String get settingsThisNode;

  /// The title of a switch: whether the node serves phones that have not paired with it.
  ///
  /// In en, this message translates to:
  /// **'Public'**
  String get settingsPublic;

  /// Under the switch settingsPublic; it is the node.
  ///
  /// In en, this message translates to:
  /// **'Any phone near it can text through it'**
  String get settingsPublicHow;

  /// Public is the switch settingsPublic.
  ///
  /// In en, this message translates to:
  /// **'Setting Public failed: {error}'**
  String settingsPublicFailed(String error);

  /// A button: this phone and the node forget their pairing.
  ///
  /// In en, this message translates to:
  /// **'Unpair'**
  String get settingsUnpair;

  /// No description provided for @settingsUnpairTitle.
  ///
  /// In en, this message translates to:
  /// **'Unpair {node}?'**
  String settingsUnpairTitle(String node);

  /// The dialog that asks before unpairing; settingsUnpairLast and settingsUnpairOthers may follow it.
  ///
  /// In en, this message translates to:
  /// **'The node and this phone forget each other; texts and contacts stay.'**
  String get settingsUnpairHow;

  /// It is the node: this phone is the last paired with it.
  ///
  /// In en, this message translates to:
  /// **'With no phone left, it turns public.'**
  String get settingsUnpairLast;

  /// It is this phone.
  ///
  /// In en, this message translates to:
  /// **'It stays paired with your other nodes.'**
  String get settingsUnpairOthers;

  /// No description provided for @settingsUnpaired.
  ///
  /// In en, this message translates to:
  /// **'Unpaired.'**
  String get settingsUnpaired;

  /// No description provided for @settingsUnpairedPublic.
  ///
  /// In en, this message translates to:
  /// **'Unpaired. The node is public.'**
  String get settingsUnpairedPublic;

  /// No description provided for @settingsUnpairFailed.
  ///
  /// In en, this message translates to:
  /// **'Unpairing failed: {error}'**
  String settingsUnpairFailed(String error);

  /// A heading, and the title of its help (settingsPhonesHelp).
  ///
  /// In en, this message translates to:
  /// **'Phones paired with this node'**
  String get settingsPhonesPaired;

  /// The help on the node's phones. 'One of them' is one of the phones with a home here.
  ///
  /// In en, this message translates to:
  /// **'A node keeps up to eight phones. Anyone at it can pair, by the code on its screen, and every paired phone can change its settings, so remove a phone you do not know or one that is gone. A removal also takes the code on the node\'s screen, to show you are there.\n\nA phone with a home here has its mail kept at this node.\n\nThe owner, one of them, chooses who has a home here, also from far away: they can give a contact a home here, remove one, or make someone else the owner. The first to make the node their home is its owner. At the node, any paired phone can make another the owner by the code on the screen.'**
  String get settingsPhonesHelp;

  /// Under settingsPhonesPaired, when the node has as many phones as it can.
  ///
  /// In en, this message translates to:
  /// **'Full: remove one before another can pair.'**
  String get settingsPhonesFull;

  /// A heading: the phones whose home node this node is.
  ///
  /// In en, this message translates to:
  /// **'Phones with a home here'**
  String get settingsPhonesHome;

  /// No description provided for @settingsNoOwner.
  ///
  /// In en, this message translates to:
  /// **'This node has no owner'**
  String get settingsNoOwner;

  /// A button beside settingsNoOwner.
  ///
  /// In en, this message translates to:
  /// **'Become owner'**
  String get settingsBecomeOwner;

  /// A heading in Settings: whether Android lets the app run with the screen off.
  ///
  /// In en, this message translates to:
  /// **'Battery'**
  String get settingsBattery;

  /// bark is the app's name, always in lower case.
  ///
  /// In en, this message translates to:
  /// **'bark may run with the screen off.'**
  String get settingsBatteryExempt;

  /// The link is the app's Bluetooth link with the node.
  ///
  /// In en, this message translates to:
  /// **'With the screen off, Android may close bark\'s link after some hours.'**
  String get settingsBatteryNotExempt;

  /// A button that asks Android to let the app run with the screen off.
  ///
  /// In en, this message translates to:
  /// **'Let bark run'**
  String get settingsLetRun;

  /// A heading in Settings: the 12 backup words.
  ///
  /// In en, this message translates to:
  /// **'Backup'**
  String get settingsBackup;

  /// No description provided for @settingsBackedUp.
  ///
  /// In en, this message translates to:
  /// **'Your 12 words bring your identity back on another phone.'**
  String get settingsBackedUp;

  /// No description provided for @settingsNotBackedUp.
  ///
  /// In en, this message translates to:
  /// **'Write down your 12 words: a lost phone without them loses your identity.'**
  String get settingsNotBackedUp;

  /// No description provided for @settingsShowWords.
  ///
  /// In en, this message translates to:
  /// **'Show my words'**
  String get settingsShowWords;

  /// A button that opens settingsRestoreTitle.
  ///
  /// In en, this message translates to:
  /// **'Restore from words'**
  String get settingsRestore;

  /// The title of the dialog that shows the backup words.
  ///
  /// In en, this message translates to:
  /// **'My 12 words'**
  String get settingsMyWords;

  /// The button that closes the dialog of the backup words.
  ///
  /// In en, this message translates to:
  /// **'I wrote them down'**
  String get settingsWroteThem;

  /// No description provided for @settingsRestoreTitle.
  ///
  /// In en, this message translates to:
  /// **'Restore from words?'**
  String get settingsRestoreTitle;

  /// No description provided for @settingsRestoreHow.
  ///
  /// In en, this message translates to:
  /// **'This phone takes the identity of the words. Contacts reach it only once they have its card.'**
  String get settingsRestoreHow;

  /// The button that restores the identity, in the dialog settingsRestoreTitle and in the one that takes an identity file's password.
  ///
  /// In en, this message translates to:
  /// **'Restore'**
  String get restore;

  /// The title of the dialog where the backup words are typed.
  ///
  /// In en, this message translates to:
  /// **'Your 12 words'**
  String get settingsYourWords;

  /// No description provided for @settingsNotWords.
  ///
  /// In en, this message translates to:
  /// **'Those are not 12 words of a backup'**
  String get settingsNotWords;

  /// Shown once the identity was restored, from the 12 words or from a file.
  ///
  /// In en, this message translates to:
  /// **'Identity restored'**
  String get restoredIdentity;

  /// A heading: the node's position, from its GPS or set by hand.
  ///
  /// In en, this message translates to:
  /// **'Place'**
  String get settingsPlace;

  /// No description provided for @settingsNoPlace.
  ///
  /// In en, this message translates to:
  /// **'The node has no place yet.'**
  String get settingsNoPlace;

  /// The node's place, set by hand in the app.
  ///
  /// In en, this message translates to:
  /// **'Set by a person: {address}.'**
  String settingsPlaceByPerson(String address);

  /// The node's place, from its own GPS.
  ///
  /// In en, this message translates to:
  /// **'From its GPS: {address}.'**
  String settingsPlaceByGps(String address);

  /// Under settingsPlace: what a place set by hand is for. It is that place; cleared by the button settingsPlaceClear.
  ///
  /// In en, this message translates to:
  /// **'For a node indoors, out of GPS reach. It wins over GPS until cleared.'**
  String get settingsPlaceHow;

  /// The label of a field, in degrees.
  ///
  /// In en, this message translates to:
  /// **'Latitude'**
  String get settingsLatitude;

  /// No description provided for @settingsLongitude.
  ///
  /// In en, this message translates to:
  /// **'Longitude'**
  String get settingsLongitude;

  /// A button: set the node's place to the latitude and longitude typed.
  ///
  /// In en, this message translates to:
  /// **'Set'**
  String get settingsPlaceSetButton;

  /// A button: the node's place set by hand goes, and its GPS gives the place again.
  ///
  /// In en, this message translates to:
  /// **'Clear'**
  String get settingsPlaceClear;

  /// The title of the dialog shown before Android asks to allow notifications.
  ///
  /// In en, this message translates to:
  /// **'Notifications'**
  String get settingsNotifications;

  /// bark is the app's name, always in lower case.
  ///
  /// In en, this message translates to:
  /// **'bark tells you when a text comes. Android asks next.'**
  String get settingsNotificationsWhy;

  /// No description provided for @settingsPairingFailed.
  ///
  /// In en, this message translates to:
  /// **'Pairing failed: {error}'**
  String settingsPairingFailed(String error);

  /// No description provided for @settingsConnectingFailed.
  ///
  /// In en, this message translates to:
  /// **'Connecting failed: {error}'**
  String settingsConnectingFailed(String error);

  /// The title of the dialog that says why a pairing ended without one.
  ///
  /// In en, this message translates to:
  /// **'Not paired'**
  String get settingsNotPaired;

  /// The title of a dialog: the node removed this phone.
  ///
  /// In en, this message translates to:
  /// **'This node is no longer yours'**
  String get settingsNotOurs;

  /// No description provided for @settingsNotOursHow.
  ///
  /// In en, this message translates to:
  /// **'{node} no longer knows this phone. Pair again with the code on its screen.'**
  String settingsNotOursHow(String node);

  /// No description provided for @nodeConnecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting…'**
  String get nodeConnecting;

  /// While the phone pairs with a node.
  ///
  /// In en, this message translates to:
  /// **'Type the code on the node\'s screen'**
  String get nodeTypeCode;

  /// No description provided for @nodePairedConnecting.
  ///
  /// In en, this message translates to:
  /// **'Paired, connecting…'**
  String get nodePairedConnecting;

  /// The heading of the section on the node the phone is on, in Settings and on the debug screen.
  ///
  /// In en, this message translates to:
  /// **'Node'**
  String get node;

  /// A button that ends the link with the node; the pairing stays.
  ///
  /// In en, this message translates to:
  /// **'Disconnect'**
  String get nodeDisconnect;

  /// A button: the phone stops looking for nodes by itself.
  ///
  /// In en, this message translates to:
  /// **'Stop looking'**
  String get nodeStopLooking;

  /// A button that looks for the nodes nearby.
  ///
  /// In en, this message translates to:
  /// **'Scan'**
  String get nodeScan;

  /// The button nodeScan while it looks.
  ///
  /// In en, this message translates to:
  /// **'Stop'**
  String get nodeStopScan;

  /// No description provided for @nodeScanFailed.
  ///
  /// In en, this message translates to:
  /// **'Scan failed: {error}'**
  String nodeScanFailed(String error);

  /// No description provided for @nodeNoneFound.
  ///
  /// In en, this message translates to:
  /// **'No node found nearby.'**
  String get nodeNoneFound;

  /// No description provided for @nodeNoneFoundHow.
  ///
  /// In en, this message translates to:
  /// **'Switch one on, or come closer.'**
  String get nodeNoneFoundHow;

  /// A heading above the nodes this phone is paired with.
  ///
  /// In en, this message translates to:
  /// **'Paired with'**
  String get nodePairedWith;

  /// A button: this phone stops listing a node it was paired with.
  ///
  /// In en, this message translates to:
  /// **'Forget'**
  String get nodeForget;

  /// The title of the help on a paired node that removed this phone.
  ///
  /// In en, this message translates to:
  /// **'{node} does not know this phone'**
  String nodeLostTitle(String node);

  /// The help of nodeLostTitle. Pair again and Forget are the buttons pairAgain and nodeForget.
  ///
  /// In en, this message translates to:
  /// **'Someone paired with it removed this phone, the node lost its memory, or you moved here from another phone: a pairing stays on the phone it was made on.\n\nPair again: pair anew with the code on its screen. Shown while the node is in reach.\n\nForget: stop listing the node here.'**
  String get nodeLostHelp;

  /// How a node in Settings stands, as a line of its own: the phone is connected to it. Also the first word of a found node's row, before words such as nodeFoundPublic.
  ///
  /// In en, this message translates to:
  /// **'Connected'**
  String get nodeConnected;

  /// How a paired node stands, under its ID: the person's home, and the phone is connected to it. The other nodeHome… messages are the same for the home.
  ///
  /// In en, this message translates to:
  /// **'Your home, connected'**
  String get nodeHomeConnected;

  /// How a paired node stands: it removed this phone.
  ///
  /// In en, this message translates to:
  /// **'Does not know this phone'**
  String get nodeLost;

  /// No description provided for @nodeHomeLost.
  ///
  /// In en, this message translates to:
  /// **'Your home, does not know this phone'**
  String get nodeHomeLost;

  /// How a paired node stands: it or this app needs an update.
  ///
  /// In en, this message translates to:
  /// **'Cannot be used yet. Tap to try again'**
  String get nodeUnserved;

  /// No description provided for @nodeHomeUnserved.
  ///
  /// In en, this message translates to:
  /// **'Your home, cannot be used yet. Tap to try again'**
  String get nodeHomeUnserved;

  /// How a paired node stands: the phone looks for it and does not hear it.
  ///
  /// In en, this message translates to:
  /// **'Not in reach'**
  String get nodeAway;

  /// No description provided for @nodeHomeAway.
  ///
  /// In en, this message translates to:
  /// **'Your home, not in reach'**
  String get nodeHomeAway;

  /// How a paired node stands: the phone is not looking for nodes.
  ///
  /// In en, this message translates to:
  /// **'Not looking. Tap to connect'**
  String get nodeNotLooking;

  /// No description provided for @nodeHomeNotLooking.
  ///
  /// In en, this message translates to:
  /// **'Your home, not looking. Tap to connect'**
  String get nodeHomeNotLooking;

  /// Under a paired node's ID: it is the person's home.
  ///
  /// In en, this message translates to:
  /// **'Your home'**
  String get nodeYourHome;

  /// A node found by a scan serves phones that have not paired. Lower case: it is one of a list of words joined by commas, such as nodeConnected, public, signalStrong.
  ///
  /// In en, this message translates to:
  /// **'public'**
  String get nodeFoundPublic;

  /// As nodeFoundPublic: lower case, in the list of words under a found node.
  ///
  /// In en, this message translates to:
  /// **'cannot be used yet'**
  String get nodeFoundUnserved;

  /// A button: pair with the node.
  ///
  /// In en, this message translates to:
  /// **'Pair'**
  String get nodePair;

  /// No description provided for @nodePairTitle.
  ///
  /// In en, this message translates to:
  /// **'Pair with {node}?'**
  String nodePairTitle(String node);

  /// It is the node.
  ///
  /// In en, this message translates to:
  /// **'You can then change its settings and update it. Type the code on its screen.'**
  String get nodePairHow;

  /// No description provided for @nodeForgetTitle.
  ///
  /// In en, this message translates to:
  /// **'Forget {node}?'**
  String nodeForgetTitle(String node);

  /// It is the node; staysYourHome may follow.
  ///
  /// In en, this message translates to:
  /// **'This phone stops listing it.'**
  String get nodeForgetHow;

  /// The node was forgotten; staysYourHome, then forgetInBluetoothToo or nodeForgetLater, may follow.
  ///
  /// In en, this message translates to:
  /// **'Forgotten.'**
  String get nodeForgotten;

  /// The phone is Android, which keeps its own pairing with the node.
  ///
  /// In en, this message translates to:
  /// **'The phone drops its pairing when it next hears the node, or forget it in Bluetooth settings now.'**
  String get nodeForgetLater;

  /// No description provided for @nodeForgetFailed.
  ///
  /// In en, this message translates to:
  /// **'Forgetting failed: {error}'**
  String nodeForgetFailed(String error);

  /// A button or menu item on a phone's row: that phone's person becomes the home node's owner.
  ///
  /// In en, this message translates to:
  /// **'Make owner'**
  String get peopleMakeOwner;

  /// Under a phone in the node's list: it is this phone. Lower case, as it is one of a list of words joined by commas; so are the other words of that list, peopleOwner to peopleNeverSeen.
  ///
  /// In en, this message translates to:
  /// **'this phone'**
  String get peopleThisPhone;

  /// In the list of words under a phone: it is the person's own.
  ///
  /// In en, this message translates to:
  /// **'you'**
  String get peopleYou;

  /// No description provided for @peopleOwner.
  ///
  /// In en, this message translates to:
  /// **'owner'**
  String get peopleOwner;

  /// Under a phone in the list of homes: it is paired with this node as well.
  ///
  /// In en, this message translates to:
  /// **'paired too'**
  String get peoplePairedToo;

  /// Under a phone in the list of paired phones: it has its home here as well.
  ///
  /// In en, this message translates to:
  /// **'has a home here too'**
  String get peopleHomeToo;

  /// The phone is connected to the node now.
  ///
  /// In en, this message translates to:
  /// **'linked now'**
  String get peopleLinked;

  /// When the node last saw the phone.
  ///
  /// In en, this message translates to:
  /// **'seen {when}'**
  String peopleSeen(String when);

  /// The node has not yet seen the phone.
  ///
  /// In en, this message translates to:
  /// **'seen never'**
  String get peopleNeverSeen;

  /// The tooltip of a phone row's menu.
  ///
  /// In en, this message translates to:
  /// **'Choices'**
  String get peopleChoices;

  /// A phone's row, when its person has given no name.
  ///
  /// In en, this message translates to:
  /// **'A phone with no name'**
  String get peopleNoName;

  /// The title of a dialog. {name} is the name the phone's person gave.
  ///
  /// In en, this message translates to:
  /// **'Remove {name}\'s phone and its home?'**
  String peopleRemovePhoneAndHome(String name);

  /// No description provided for @peopleRemoveUnnamedAndHome.
  ///
  /// In en, this message translates to:
  /// **'Remove the phone with no name and its home?'**
  String get peopleRemoveUnnamedAndHome;

  /// No description provided for @peopleRemovePhone.
  ///
  /// In en, this message translates to:
  /// **'Remove {name}\'s phone?'**
  String peopleRemovePhone(String name);

  /// No description provided for @peopleRemoveUnnamed.
  ///
  /// In en, this message translates to:
  /// **'Remove the phone with no name?'**
  String get peopleRemoveUnnamed;

  /// No description provided for @peopleRemoveThisHome.
  ///
  /// In en, this message translates to:
  /// **'Remove this home?'**
  String get peopleRemoveThisHome;

  /// No description provided for @peoplePhoneGoes.
  ///
  /// In en, this message translates to:
  /// **'{name}\'s phone can no longer use the node unless paired again.'**
  String peoplePhoneGoes(String name);

  /// No description provided for @peopleUnnamedGoes.
  ///
  /// In en, this message translates to:
  /// **'The phone with no name can no longer use the node unless paired again.'**
  String get peopleUnnamedGoes;

  /// Their: the person of the phone being removed.
  ///
  /// In en, this message translates to:
  /// **'The node stops keeping their mail.'**
  String get peopleNodeStopsMail;

  /// The owner removes a person's home at their home node.
  ///
  /// In en, this message translates to:
  /// **'Remove {name}\'s home?'**
  String peopleRemoveHome(String name);

  /// As peopleRemoveHome, for a person with no name.
  ///
  /// In en, this message translates to:
  /// **'Remove this person\'s home?'**
  String get peopleRemoveTheirHome;

  /// No description provided for @peopleHomeStopsMail.
  ///
  /// In en, this message translates to:
  /// **'Your home stops keeping their mail.'**
  String get peopleHomeStopsMail;

  /// No description provided for @peopleMakeOwnerTitle.
  ///
  /// In en, this message translates to:
  /// **'Make {name} the owner?'**
  String peopleMakeOwnerTitle(String name);

  /// No description provided for @peopleMakeThemOwner.
  ///
  /// In en, this message translates to:
  /// **'Make this person the owner?'**
  String get peopleMakeThemOwner;

  /// No description provided for @peopleOwnerChooses.
  ///
  /// In en, this message translates to:
  /// **'The owner chooses who has a home here.'**
  String get peopleOwnerChooses;

  /// The person, now the owner, hands the role on.
  ///
  /// In en, this message translates to:
  /// **'The owner chooses who has a home here. You no longer can.'**
  String get peopleOwnerChoosesNotYou;

  /// Ends the dialog of a change made at the node, before the button peopleShowCode.
  ///
  /// In en, this message translates to:
  /// **'You type the code the node then shows.'**
  String get peopleTypeCodeNext;

  /// A button: the node shows a code on its screen.
  ///
  /// In en, this message translates to:
  /// **'Show the code'**
  String get peopleShowCode;

  /// Another phone removed the record first.
  ///
  /// In en, this message translates to:
  /// **'Already gone.'**
  String get peopleGone;

  /// No description provided for @peopleCodeFailed.
  ///
  /// In en, this message translates to:
  /// **'Asking the node for a code failed: {error}'**
  String peopleCodeFailed(String error);

  /// A phone's record was removed.
  ///
  /// In en, this message translates to:
  /// **'Removed'**
  String get peopleRemoved;

  /// No description provided for @peopleIsOwner.
  ///
  /// In en, this message translates to:
  /// **'{name} is the owner'**
  String peopleIsOwner(String name);

  /// As peopleIsOwner, for a person with no name.
  ///
  /// In en, this message translates to:
  /// **'This person is the owner'**
  String get peopleThemOwner;

  /// The title of a dialog: a wrong or late code changed nothing.
  ///
  /// In en, this message translates to:
  /// **'Not changed'**
  String get peopleNotChanged;

  /// No description provided for @peopleNotRemoved.
  ///
  /// In en, this message translates to:
  /// **'Not removed'**
  String get peopleNotRemoved;

  /// No description provided for @peopleWrongCode.
  ///
  /// In en, this message translates to:
  /// **'Wrong code, or too late. Each code works once.'**
  String get peopleWrongCode;

  /// A button: ask the node for another code.
  ///
  /// In en, this message translates to:
  /// **'New code'**
  String get peopleNewCode;

  /// The change typed with the node's code failed.
  ///
  /// In en, this message translates to:
  /// **'It failed: {error}'**
  String peopleItFailed(String error);

  /// The title of the dialog where the code is typed.
  ///
  /// In en, this message translates to:
  /// **'The code on the node'**
  String get peopleCodeTitle;

  /// No description provided for @peopleCodeHow.
  ///
  /// In en, this message translates to:
  /// **'Type the six digits under {word} on the node\'s screen, within 30 s.'**
  String peopleCodeHow(String word);

  /// The label of the field where the code is typed.
  ///
  /// In en, this message translates to:
  /// **'Code'**
  String get peopleCode;

  /// No description provided for @linkNeedsFirmware.
  ///
  /// In en, this message translates to:
  /// **'{node} needs newer firmware. Update the node.'**
  String linkNeedsFirmware(String node);

  /// No description provided for @linkNeedsApp.
  ///
  /// In en, this message translates to:
  /// **'{node} needs a newer app. Update the app.'**
  String linkNeedsApp(String node);

  /// Why a value the node sent could not be read, after a colon, as in sessionBadValue; lower case. {what} stays English.
  ///
  /// In en, this message translates to:
  /// **'malformed {what}'**
  String linkMalformed(String what);

  /// Why the node refused an update. Lower case, as it may follow other words, such as 'The node refused: '; so are the other linkRefused… reasons.
  ///
  /// In en, this message translates to:
  /// **'the file is not firmware this node reads'**
  String get linkRefusedHeader;

  /// No description provided for @linkRefusedVersion.
  ///
  /// In en, this message translates to:
  /// **'the firmware is not newer than the version the node runs'**
  String get linkRefusedVersion;

  /// No description provided for @linkRefusedBusy.
  ///
  /// In en, this message translates to:
  /// **'the node holds checked new firmware already: reboot it first'**
  String get linkRefusedBusy;

  /// No description provided for @linkRefusedIncomplete.
  ///
  /// In en, this message translates to:
  /// **'the node does not hold every byte of the firmware'**
  String get linkRefusedIncomplete;

  /// No description provided for @linkRefusedHash.
  ///
  /// In en, this message translates to:
  /// **'the bytes the node holds are not the firmware\'s; send it again'**
  String get linkRefusedHash;

  /// No description provided for @linkRefusedBattery.
  ///
  /// In en, this message translates to:
  /// **'the node is on a low battery; it keeps the firmware, to go on once it is on USB or charged'**
  String get linkRefusedBattery;

  /// No description provided for @linkRefusedFlash.
  ///
  /// In en, this message translates to:
  /// **'the node\'s flash memory failed'**
  String get linkRefusedFlash;

  /// No description provided for @linkRefusedSignature.
  ///
  /// In en, this message translates to:
  /// **'the firmware is not signed by a key the node holds: a file built here is signed with `{command}`, a release\'s is signed already'**
  String linkRefusedSignature(String command);

  /// The node answered with a Bluetooth ATT error; it may follow other words, such as sendFailed's.
  ///
  /// In en, this message translates to:
  /// **'ATT error 0x{code}'**
  String linkAttError(String code);

  /// Why something failed: the node removed this phone. It may follow other words.
  ///
  /// In en, this message translates to:
  /// **'{node} no longer knows this phone'**
  String linkNotOurs(String node);

  /// The phone is Android, which kept its own pairing with the node.
  ///
  /// In en, this message translates to:
  /// **'The phone kept its old pairing with {node}, so it cannot use the node. Forget it in Bluetooth settings.'**
  String linkBondKept(String node);

  /// Two sentences, the last without its full stop, which the app adds.
  ///
  /// In en, this message translates to:
  /// **'The link to {node} ended. Try again nearer to it'**
  String linkEnded(String node);

  /// A sentence without its full stop, which the app adds.
  ///
  /// In en, this message translates to:
  /// **'Not paired: cancelled, a wrong code, or the node is full (its screen says {full})'**
  String linkNotPaired(String full);

  /// A sentence without its full stop, which the app adds.
  ///
  /// In en, this message translates to:
  /// **'{node} not found: come nearer and try again'**
  String linkNotSeen(String node);

  /// Why a file, picked or downloaded, is not node firmware the app can send; it follows updateNoFirmware's words, so lower case. So are the other linkImage… reasons.
  ///
  /// In en, this message translates to:
  /// **'malformed image: too short'**
  String get linkImageTooShort;

  /// The file does not begin as a firmware file does.
  ///
  /// In en, this message translates to:
  /// **'malformed image: not one'**
  String get linkImageNotOne;

  /// The firmware file is of a layout this app does not know.
  ///
  /// In en, this message translates to:
  /// **'malformed image: format'**
  String get linkImageFormat;

  /// The firmware file is longer or shorter than its header says.
  ///
  /// In en, this message translates to:
  /// **'malformed image: length'**
  String get linkImageLength;

  /// The firmware file's bytes do not match the checksum in its header: it is damaged.
  ///
  /// In en, this message translates to:
  /// **'malformed image: hash'**
  String get linkImageHash;

  /// A node takes no more pairings; shown in Settings' sections on the home and on the node.
  ///
  /// In en, this message translates to:
  /// **'This node is full: eight phones are paired with it.'**
  String get fullNode;

  /// Under the heading homeNode: the person has no home node.
  ///
  /// In en, this message translates to:
  /// **'None: your card is temporary.'**
  String get homeNone;

  /// Under the heading homeNode: the home's address.
  ///
  /// In en, this message translates to:
  /// **'At {address}.'**
  String homeAt(String address);

  /// The place is the node's position, from its GPS or set by hand under This node; not room for more.
  ///
  /// In en, this message translates to:
  /// **'This node has no place yet, so it cannot be a home.'**
  String get homeNoPlace;

  /// As homeNoPlace, for the paired phone; This node is the section settingsThisNode below, where the place is set.
  ///
  /// In en, this message translates to:
  /// **'This node has no place yet, so it cannot be a home. Set it under This node.'**
  String get homeNoPlaceSetIt;

  /// Why the node the phone is on cannot be a home: it is on battery, or carried.
  ///
  /// In en, this message translates to:
  /// **'A home must stay on power in one place.'**
  String get homeNeedsPower;

  /// Shown while the phone is on its home node. 'It' is that node; 'another' is another node.
  ///
  /// In en, this message translates to:
  /// **'Your home no longer keeps your mail. Make it your home again, or choose another.'**
  String get homeKeyGone;

  /// No description provided for @homeSilent.
  ///
  /// In en, this message translates to:
  /// **'Your home did not answer your last fetch.'**
  String get homeSilent;

  /// No description provided for @homeHoldsOtherKey.
  ///
  /// In en, this message translates to:
  /// **'This node keeps mail for this phone, and is not your home.'**
  String get homeHoldsOtherKey;

  /// No description provided for @homeOthers.
  ///
  /// In en, this message translates to:
  /// **'This node is someone else\'s home.'**
  String get homeOthers;

  /// A button: the person has no home node any more. Also named in homeHelp.
  ///
  /// In en, this message translates to:
  /// **'Remove home'**
  String get homeRemove;

  /// A button: the node the phone is on, which is not the home, stops keeping the person's mail.
  ///
  /// In en, this message translates to:
  /// **'Forget me here'**
  String get homeForgetMe;

  /// A button: after a restore from the 12 words, the person gives their home's address to use it again. Also named in homeHelp.
  ///
  /// In en, this message translates to:
  /// **'Use my existing home'**
  String get homeUseExisting;

  /// The home the person named with homeUseExisting did not answer.
  ///
  /// In en, this message translates to:
  /// **'{node} did not answer. Nothing changed.'**
  String homeNoAnswer(String node);

  /// The network found no way to the home the person named with homeUseExisting.
  ///
  /// In en, this message translates to:
  /// **'No way found to {node}. Nothing changed.'**
  String homeNoWay(String node);

  /// What became of a fetch of the mail whose search for the home node found no way.
  ///
  /// In en, this message translates to:
  /// **'No way found to your home'**
  String get mailboxFetchNoWay;

  /// While the home named with homeUseExisting has not answered.
  ///
  /// In en, this message translates to:
  /// **'Asking {node} for your mail…'**
  String homeAsking(String node);

  /// The person's old home, being left; 'it' is that node.
  ///
  /// In en, this message translates to:
  /// **'Leaving {node}: it sends you its mail, then forgets you.'**
  String homeLeaving(String node);

  /// A heading, and the title of its help (homePeopleHelp): the people whose home is the person's home node.
  ///
  /// In en, this message translates to:
  /// **'People with a home here'**
  String get homePeople;

  /// Under homePeople: the person owns their home node.
  ///
  /// In en, this message translates to:
  /// **'You are its owner.'**
  String get homeYouOwner;

  /// A button: ask the home for the list of homePeople.
  ///
  /// In en, this message translates to:
  /// **'Show them'**
  String get homeShowThem;

  /// A button: the button homeShowThem once a list has come, and on the map, ask a node that did not answer again.
  ///
  /// In en, this message translates to:
  /// **'Ask again'**
  String get askAgain;

  /// No description provided for @homeAskFailed.
  ///
  /// In en, this message translates to:
  /// **'Asking failed: {error}'**
  String homeAskFailed(String error);

  /// Making a node the home, or having none, failed.
  ///
  /// In en, this message translates to:
  /// **'Setting the home failed: {error}'**
  String homeSetFailed(String error);

  /// No description provided for @homeRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove home?'**
  String get homeRemoveTitle;

  /// No description provided for @homeSetTitle.
  ///
  /// In en, this message translates to:
  /// **'Set this node as home?'**
  String get homeSetTitle;

  /// The first sentence of the dialog homeSetTitle, for a node the phone uses as a guest; one of homeCardWaits to homeCardGoes follows. 'Its' is the node's.
  ///
  /// In en, this message translates to:
  /// **'First you pair: type the code on its screen.'**
  String get homePairFirst;

  /// In the dialogs homeSetTitle and homeRemoveTitle. Send my card is the button commonSendCard.
  ///
  /// In en, this message translates to:
  /// **'Your card stays; its last change still waits for Send my card.'**
  String get homeCardWaits;

  /// No description provided for @homeCardSame.
  ///
  /// In en, this message translates to:
  /// **'Your card stays the same.'**
  String get homeCardSame;

  /// The person has no contacts to send the new card to.
  ///
  /// In en, this message translates to:
  /// **'Your card changes.'**
  String get homeCardChanges;

  /// Who gets the new card; count is how many contacts, one or more.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{Your new card goes to your contact.} other{Your new card goes to your {count} contacts.}}'**
  String homeCardGoes(int count);

  /// The button that confirms the dialog homeSetTitle.
  ///
  /// In en, this message translates to:
  /// **'Set'**
  String get homeSet;

  /// The help on the home node. Fetch my mail, Remove home and Use my existing home are the buttons commonFetch, homeRemove and homeUseExisting.
  ///
  /// In en, this message translates to:
  /// **'Your home is a node that stays on power in one place. It keeps texts to you while your phone is away, and your card carries its address, so the card stays good wherever you go. The inbox button on the first screen, Fetch my mail, brings them to the node you are on.\n\nWithout a home your card is temporary: it names the node you use now, which keeps your mail only while you come back to it.\n\nTo make a node your home, the phone pairs with it. A friend who owns a home node can also give you a home there from afar.\n\nWhen you choose another home, or none, your old home sends you the mail it keeps, then forgets you.\n\nA home that stops answering may be off or out of reach, or may have lost your place there. Fetch again later; if it still does not answer, make another node your home, or choose Remove home.\n\nUse my existing home is for a phone that got your identity back from your 12 words: give your home\'s address, and once your home answers it is your home again.'**
  String get homeHelp;

  /// The help of homePeople. 'Each' is each of those changes.
  ///
  /// In en, this message translates to:
  /// **'As the owner you choose who has a home here, from anywhere: give a contact a home here from their conversation\'s menu, remove a person\'s home, or make someone else the owner.\n\nEach goes to your home with a fetch of your mail, and its answer comes back with it.'**
  String get homePeopleHelp;

  /// The title of the dialog opened by homeUseExisting.
  ///
  /// In en, this message translates to:
  /// **'Your home\'s address'**
  String get homeAddressTitle;

  /// In the dialog homeAddressTitle: 'it' is the home's address.
  ///
  /// In en, this message translates to:
  /// **'Its 26 hex digits, or a card of yours that names it.'**
  String get homeAddressHow;

  /// No description provided for @homeAddressLabel.
  ///
  /// In en, this message translates to:
  /// **'Address or card'**
  String get homeAddressLabel;

  /// No description provided for @homeCardNoHome.
  ///
  /// In en, this message translates to:
  /// **'That card names no home'**
  String get homeCardNoHome;

  /// No description provided for @homeNotAddress.
  ///
  /// In en, this message translates to:
  /// **'Not an address or a card'**
  String get homeNotAddress;

  /// A button in the dialog homeAddressTitle that opens the camera.
  ///
  /// In en, this message translates to:
  /// **'Scan a card'**
  String get homeScanCard;

  /// The button that ends the dialog homeAddressTitle: ask the home node for the person's mail.
  ///
  /// In en, this message translates to:
  /// **'Ask it'**
  String get homeAskIt;

  /// No description provided for @mailboxNoHomeToFetch.
  ///
  /// In en, this message translates to:
  /// **'No home node to fetch from'**
  String get mailboxNoHomeToFetch;

  /// After Fetch my mail, with no node connected: it goes once there is one.
  ///
  /// In en, this message translates to:
  /// **'The fetch waits for a node'**
  String get mailboxFetchQueued;

  /// After Fetch my mail, on the home node itself.
  ///
  /// In en, this message translates to:
  /// **'At your home, your mail comes by itself'**
  String get mailboxAtHome;

  /// No description provided for @mailboxStillWaiting.
  ///
  /// In en, this message translates to:
  /// **'Still waiting for the last fetch'**
  String get mailboxStillWaiting;

  /// No description provided for @mailboxNoPlace.
  ///
  /// In en, this message translates to:
  /// **'This node has no place yet, so mail cannot come back to it'**
  String get mailboxNoPlace;

  /// No description provided for @mailboxFetching.
  ///
  /// In en, this message translates to:
  /// **'Fetching your mail'**
  String get mailboxFetching;

  /// No description provided for @mailboxNotConnected.
  ///
  /// In en, this message translates to:
  /// **'Not connected to a node'**
  String get mailboxNotConnected;

  /// No description provided for @mailboxRefused.
  ///
  /// In en, this message translates to:
  /// **'The node refused. Nothing changed.'**
  String get mailboxRefused;

  /// No description provided for @mailboxIsHome.
  ///
  /// In en, this message translates to:
  /// **'This node is your home'**
  String get mailboxIsHome;

  /// No description provided for @mailboxIsHomeLeft.
  ///
  /// In en, this message translates to:
  /// **'This node is your home. Your old home sends you its mail, then forgets you.'**
  String get mailboxIsHomeLeft;

  /// After Remove home: there was no home to remove.
  ///
  /// In en, this message translates to:
  /// **'No home'**
  String get mailboxNoHome;

  /// No description provided for @mailboxRefusedStillHome.
  ///
  /// In en, this message translates to:
  /// **'The node refused. It is still your home.'**
  String get mailboxRefusedStillHome;

  /// No description provided for @mailboxHomeRemoved.
  ///
  /// In en, this message translates to:
  /// **'Home removed'**
  String get mailboxHomeRemoved;

  /// No description provided for @mailboxHomeRemovedLeft.
  ///
  /// In en, this message translates to:
  /// **'Home removed. Your old home sends you its mail, then forgets you.'**
  String get mailboxHomeRemovedLeft;

  /// After the button commonUseGift: the home given is no longer there.
  ///
  /// In en, this message translates to:
  /// **'No home to use'**
  String get mailboxNoGift;

  /// No description provided for @mailboxNodeIsHome.
  ///
  /// In en, this message translates to:
  /// **'{node} is your home'**
  String mailboxNodeIsHome(String node);

  /// No description provided for @mailboxNodeIsHomeLeft.
  ///
  /// In en, this message translates to:
  /// **'{node} is your home. Your old home sends you its mail, then forgets you.'**
  String mailboxNodeIsHomeLeft(String node);

  /// No description provided for @mailboxHasHome.
  ///
  /// In en, this message translates to:
  /// **'You have a home already'**
  String get mailboxHasHome;

  /// No description provided for @mailboxConnectFirst.
  ///
  /// In en, this message translates to:
  /// **'Connect to a node first'**
  String get mailboxConnectFirst;

  /// No description provided for @mailboxAsking.
  ///
  /// In en, this message translates to:
  /// **'Asking {node} for your mail'**
  String mailboxAsking(String node);

  /// A node the person named as their home answered that it keeps no mail of theirs.
  ///
  /// In en, this message translates to:
  /// **'{node} does not keep your mail. Nothing changed.'**
  String mailboxKeepsNot(String node);

  /// No description provided for @mailboxHomeGone.
  ///
  /// In en, this message translates to:
  /// **'Your home no longer keeps your mail. Choose a new home in Settings.'**
  String get mailboxHomeGone;

  /// No description provided for @mailboxRefusedStillKeeps.
  ///
  /// In en, this message translates to:
  /// **'The node refused. It still keeps your mail.'**
  String get mailboxRefusedStillKeeps;

  /// After Forget me here: the node, which is not the home, no longer keeps the person's mail.
  ///
  /// In en, this message translates to:
  /// **'It forgot you'**
  String get mailboxForgotYou;

  /// No description provided for @mailboxNotPaired.
  ///
  /// In en, this message translates to:
  /// **'Not paired, so not your home'**
  String get mailboxNotPaired;

  /// No description provided for @mailboxNeedsPower.
  ///
  /// In en, this message translates to:
  /// **'Paired, but a home must stay on power in one place'**
  String get mailboxNeedsPower;

  /// No description provided for @mailboxNoHomeNode.
  ///
  /// In en, this message translates to:
  /// **'No home node'**
  String get mailboxNoHomeNode;

  /// The owner's request went to the home node with a fetch of the mail.
  ///
  /// In en, this message translates to:
  /// **'Sent to your home; its answer comes with your mail'**
  String get mailboxSentHome;

  /// Why the person cannot become owner: the phone is not connected to their home node.
  ///
  /// In en, this message translates to:
  /// **'Not at your home'**
  String get mailboxNotAtHome;

  /// After Become owner: the person now owns their home node.
  ///
  /// In en, this message translates to:
  /// **'You are its owner'**
  String get mailboxYouOwner;

  /// After Become owner: the node did not make the person its owner.
  ///
  /// In en, this message translates to:
  /// **'The node did not take it'**
  String get mailboxNotTaken;

  /// No description provided for @mailboxOnlyOwner.
  ///
  /// In en, this message translates to:
  /// **'Only your home\'s owner can share it'**
  String get mailboxOnlyOwner;

  /// No description provided for @mailboxNotOwner.
  ///
  /// In en, this message translates to:
  /// **'Your home says you are not its owner'**
  String get mailboxNotOwner;

  /// The owner gave a contact a home at their home node, and the app sends the contact the home. {name} is the contact's name.
  ///
  /// In en, this message translates to:
  /// **'{name} has a home at yours now. They are told'**
  String mailboxShared(String name);

  /// No description provided for @mailboxHomeFull.
  ///
  /// In en, this message translates to:
  /// **'Your home is full: it keeps eight people'**
  String get mailboxHomeFull;

  /// The owner removed a person's home at their home node.
  ///
  /// In en, this message translates to:
  /// **'Removed from your home'**
  String get mailboxRemoved;

  /// No description provided for @mailboxNewOwner.
  ///
  /// In en, this message translates to:
  /// **'Your home has a new owner'**
  String get mailboxNewOwner;

  /// {name} is the name the new owner's person gave.
  ///
  /// In en, this message translates to:
  /// **'{name} is your home\'s owner now'**
  String mailboxIsOwner(String name);

  /// The person's home that the owner removed or handed on was gone already.
  ///
  /// In en, this message translates to:
  /// **'Already gone'**
  String get mailboxGone;

  /// The phone's location was asked for, to set a node's place or to show the phone on the map, and the person did not allow it.
  ///
  /// In en, this message translates to:
  /// **'Location permission denied'**
  String get placeDenied;

  /// The phone could not tell where it is. {error} is the phone's own reason, in English.
  ///
  /// In en, this message translates to:
  /// **'No location: {error}'**
  String placeNone(String error);

  /// A version of the app or of a node's firmware, standing alone: Settings' line on the app or the node, or the title of a firmware file chosen to send.
  ///
  /// In en, this message translates to:
  /// **'Version {version}'**
  String version(int version);

  /// A button that sends new firmware to the connected node.
  ///
  /// In en, this message translates to:
  /// **'Send to the node'**
  String get sendToNode;

  /// The line on the first screen and in Settings while a paired node is away, restarting into a new version of its firmware.
  ///
  /// In en, this message translates to:
  /// **'{node} is installing version {version}, back in about a minute'**
  String installInstalling(String node, int version);

  /// As installInstalling, once the node is away longer than its restart takes.
  ///
  /// In en, this message translates to:
  /// **'{node} is installing version {version}, not back yet'**
  String installInstallingLate(String node, int version);

  /// The help on that line, for firmware this phone sent the node.
  ///
  /// In en, this message translates to:
  /// **'{node} restarts into version {version}, which this phone sent it, for about a minute. The phone waits for it and reconnects by itself.'**
  String installSentHelp(String node, int version);

  /// The help on that line, for firmware the node got over the radio from other nodes. 'It' is the node.
  ///
  /// In en, this message translates to:
  /// **'{node} got version {version} from the nodes around it. It passes on the texts it holds, at most ten minutes, then restarts for about a minute. The phone reconnects by itself; nothing is lost.'**
  String installMeshHelp(String node, int version);

  /// The title of the screen that sends new firmware to the connected node.
  ///
  /// In en, this message translates to:
  /// **'Update firmware'**
  String get updateTitle;

  /// No description provided for @updateNotConnected.
  ///
  /// In en, this message translates to:
  /// **'Not connected to a node.'**
  String get updateNotConnected;

  /// The version of the firmware the connected node runs.
  ///
  /// In en, this message translates to:
  /// **'{node} runs version {version}.'**
  String updateRuns(String node, int version);

  /// As updateRuns, after this phone updated the node. {ago} is how long ago, such as timeMinutesAgo or timeJustNow.
  ///
  /// In en, this message translates to:
  /// **'{node} runs version {version}, updated from version {from} {ago}.'**
  String updateRunsSince(String node, int version, int from, String ago);

  /// A button: download the firmware of the project's latest release, to send it to the node.
  ///
  /// In en, this message translates to:
  /// **'Latest release'**
  String get updateLatest;

  /// A button: choose a firmware file on the phone, to send it to the node.
  ///
  /// In en, this message translates to:
  /// **'Pick a file'**
  String get updatePickFile;

  /// Stands for the name of a firmware file chosen on the phone when it has none; it begins the line updateImage, in lower case.
  ///
  /// In en, this message translates to:
  /// **'a picked file'**
  String get updatePickedFile;

  /// Why there is no firmware to send: the person closed the file picker. It follows updateNoFirmware's words, so lower case.
  ///
  /// In en, this message translates to:
  /// **'no file chosen'**
  String get updateNoFile;

  /// Why there is no firmware to send: downloading the release's file failed. It follows updateNoFirmware's words, so lower case.
  ///
  /// In en, this message translates to:
  /// **'could not download the file: check the internet connection'**
  String get updateDownloadFailed;

  /// Downloading or reading a firmware file failed. {error} is why, such as linkImageHash.
  ///
  /// In en, this message translates to:
  /// **'No firmware: {error}'**
  String updateNoFirmware(String error);

  /// The title of a warning: since the update, the node hears no other node.
  ///
  /// In en, this message translates to:
  /// **'The node hears no one'**
  String get updateHearsNoOne;

  /// Under updateHearsNoOne. 'It' is the node; 'that release' is the release of the version the node ran before, whose file {file} is.
  ///
  /// In en, this message translates to:
  /// **'Since version {version} it hears no other node: the new firmware may have broken its radio. To go back, double-tap its reset button and copy {file} from that release to it over USB; the phone cannot send an older version.'**
  String updateHearsNoOneWarning(int version, String file);

  /// Why the button updateLatest is off: the app has not yet learnt of any release.
  ///
  /// In en, this message translates to:
  /// **'No release known yet.'**
  String get updateNoRelease;

  /// Why the button updateLatest is off.
  ///
  /// In en, this message translates to:
  /// **'Release {release} carries no node firmware.'**
  String updateReleaseNoFirmware(int release);

  /// Why the button updateLatest is off.
  ///
  /// In en, this message translates to:
  /// **'The node already runs the latest release\'s firmware, version {version}.'**
  String updateRunsLatest(int version);

  /// Why the button updateLatest is off; {version} is the version of the latest release's firmware.
  ///
  /// In en, this message translates to:
  /// **'The node runs a newer version than the latest release\'s firmware, {version}.'**
  String updateRunsNewer(int version);

  /// Under the firmware's version: where it came from and its size.
  ///
  /// In en, this message translates to:
  /// **'{source}, {kib} KiB'**
  String updateImage(String source, int kib);

  /// As updateImage, for firmware no newer than the version the node runs, which the node would not take.
  ///
  /// In en, this message translates to:
  /// **'{source}, {kib} KiB: not newer than the node\'s'**
  String updateImageNotNewer(String source, int kib);

  /// How much of the firmware the node has received, in kibibytes.
  ///
  /// In en, this message translates to:
  /// **'{held} of {total} KiB on the node'**
  String updateOnNode(int held, int total);

  /// While the firmware goes to the node.
  ///
  /// In en, this message translates to:
  /// **'Sending…'**
  String get updateSending;

  /// No description provided for @updateChecking.
  ///
  /// In en, this message translates to:
  /// **'The node checks the firmware…'**
  String get updateChecking;

  /// A button: go on sending the firmware after the link dropped.
  ///
  /// In en, this message translates to:
  /// **'Go on'**
  String get updateGoOn;

  /// A button: ask the node again to check the firmware it has, as after it refused for a low battery.
  ///
  /// In en, this message translates to:
  /// **'Try again'**
  String get updateTryAgain;

  /// A button: the node restarts and starts the new firmware.
  ///
  /// In en, this message translates to:
  /// **'Reboot into version {version}'**
  String updateReboot(int version);

  /// The update stopped: the node took none of the firmware's pieces for a while.
  ///
  /// In en, this message translates to:
  /// **'The node takes no pieces'**
  String get updateStalled;

  /// 'It' is the update. The link is the phone's Bluetooth link with the node.
  ///
  /// In en, this message translates to:
  /// **'The link dropped; it goes on when the node is back'**
  String get updatePaused;

  /// The node has every byte of the firmware and has checked it; what is left is to restart it.
  ///
  /// In en, this message translates to:
  /// **'The node holds version {version}'**
  String updateHolds(int version);

  /// No description provided for @updateLost.
  ///
  /// In en, this message translates to:
  /// **'The node lost the firmware'**
  String get updateLost;

  /// No description provided for @updateChecked.
  ///
  /// In en, this message translates to:
  /// **'The node checked version {version}'**
  String updateChecked(int version);

  /// Asking the node to check the firmware it received and take it failed.
  ///
  /// In en, this message translates to:
  /// **'Committing failed: {error}'**
  String updateCommitFailed(String error);

  /// The node answered with an error, such as linkAttError.
  ///
  /// In en, this message translates to:
  /// **'The node answered {error}'**
  String updateAnswered(String error);

  /// {reason} is one of the linkRefused… reasons.
  ///
  /// In en, this message translates to:
  /// **'The node refused: {reason}'**
  String updateRefused(String reason);

  /// No description provided for @updateRefusedNoReason.
  ///
  /// In en, this message translates to:
  /// **'The node refused: it did not say why'**
  String get updateRefusedNoReason;

  /// The node restarts into the new firmware, which takes about a minute.
  ///
  /// In en, this message translates to:
  /// **'The node restarts, about a minute'**
  String get updateRestarts;

  /// The node has not come back after its restart for longer than it takes.
  ///
  /// In en, this message translates to:
  /// **'The node is not back; it may be out of reach'**
  String get updateNotBack;

  /// No description provided for @updateRebootFailed.
  ///
  /// In en, this message translates to:
  /// **'Reboot failed: {error}'**
  String updateRebootFailed(String error);

  /// Why a step of the update failed: the phone is on another node than the one it updates. Lower case, as it follows words such as updateRebootFailed's.
  ///
  /// In en, this message translates to:
  /// **'connected to another node than {node}'**
  String updateOtherNode(String node);

  /// Why a step of the update failed: the phone uses the node as a guest. Lower case, as updateOtherNode.
  ///
  /// In en, this message translates to:
  /// **'only the phone paired with the node updates it'**
  String get updatePairedOnly;

  /// The new firmware did not start and the node went back to the version it ran before, {running}.
  ///
  /// In en, this message translates to:
  /// **'Version {version} did not start; the node is back on {running}'**
  String updateDidNotStart(int version, int running);

  /// The heading of Settings' section on new versions of the app and of the node's firmware.
  ///
  /// In en, this message translates to:
  /// **'Updates'**
  String get updatesHeading;

  /// No description provided for @updatesCheck.
  ///
  /// In en, this message translates to:
  /// **'Check for updates'**
  String get updatesCheck;

  /// No description provided for @updatesUnreached.
  ///
  /// In en, this message translates to:
  /// **'Could not reach the release list'**
  String get updatesUnreached;

  /// The row on this app's version; releaseAppOut names it.
  ///
  /// In en, this message translates to:
  /// **'App'**
  String get updatesApp;

  /// Under updatesApp: this app was built by hand, not by a release, and so has no version.
  ///
  /// In en, this message translates to:
  /// **'A development build'**
  String get updatesDevBuild;

  /// Under updatesApp: this app's version, and the newer one of the latest release.
  ///
  /// In en, this message translates to:
  /// **'Version {version}; {latest} is out. Tap to install'**
  String updatesAppOut(int version, int latest);

  /// No description provided for @updatesAppDownloading.
  ///
  /// In en, this message translates to:
  /// **'Version {version}; downloading {latest}…'**
  String updatesAppDownloading(int version, int latest);

  /// No description provided for @updatesAppDownloadingPercent.
  ///
  /// In en, this message translates to:
  /// **'Version {version}; downloading {latest}… {percent}%'**
  String updatesAppDownloadingPercent(int version, int latest, int percent);

  /// No description provided for @updatesAppReady.
  ///
  /// In en, this message translates to:
  /// **'Version {version}; {latest} is downloaded. Tap to install'**
  String updatesAppReady(int version, int latest);

  /// No description provided for @updatesAppFailed.
  ///
  /// In en, this message translates to:
  /// **'Version {version}; {latest} did not download. Tap to try again'**
  String updatesAppFailed(int version, int latest);

  /// A sentence after the line on the app's version, when the node runs newer firmware than this app knows; 'it' is the app. The app adds the full stop before it.
  ///
  /// In en, this message translates to:
  /// **'Update it for the node\'s new features'**
  String get updatesNodeNewer;

  /// The row on the connected node's firmware; releaseFirmwareOut names it too.
  ///
  /// In en, this message translates to:
  /// **'Node firmware'**
  String get updatesNodeFirmware;

  /// No node connected: under updatesNodeFirmware in Settings, and under the heading node on the debug screen.
  ///
  /// In en, this message translates to:
  /// **'Not connected'**
  String get nodeNotConnected;

  /// Under updatesNodeFirmware, for a node this phone uses as a guest: 'its' and 'it' are the node.
  ///
  /// In en, this message translates to:
  /// **'Only its paired phones update it'**
  String get updatesGuest;

  /// Under updatesNodeFirmware: the version the node runs, and the newer one of the latest release.
  ///
  /// In en, this message translates to:
  /// **'Version {version}; {latest} is out'**
  String updatesFirmwareOut(int version, int latest);

  /// What Check for updates found, in one line: {app} is one of the releaseApp… clauses, {node} one of the others, in lower case.
  ///
  /// In en, this message translates to:
  /// **'{app}; {node}'**
  String releaseLine(String app, String node);

  /// This app was built by hand, not by a release. It begins releaseLine.
  ///
  /// In en, this message translates to:
  /// **'App is a development build'**
  String get releaseAppDev;

  /// App below is the row updatesApp. It begins releaseLine, so it has a capital; so does releaseAppUpToDate.
  ///
  /// In en, this message translates to:
  /// **'App {version} is out: tap App below'**
  String releaseAppOut(int version);

  /// This app is the latest release's. It begins releaseLine, so it has a capital.
  ///
  /// In en, this message translates to:
  /// **'App up to date ({version})'**
  String releaseAppUpToDate(int version);

  /// Follows a releaseApp… clause in releaseLine, so lower case; so do the other release… clauses on the node.
  ///
  /// In en, this message translates to:
  /// **'no node linked'**
  String get releaseNoNode;

  /// The phone uses the node as a guest; only its paired phones update it.
  ///
  /// In en, this message translates to:
  /// **'the node is not yours to update'**
  String get releaseNotYours;

  /// The node runs firmware built by hand and loaded over USB, which has no version.
  ///
  /// In en, this message translates to:
  /// **'node firmware is a bench build'**
  String get releaseBench;

  /// The node runs version {version}; the latest release has no node firmware.
  ///
  /// In en, this message translates to:
  /// **'node firmware {version}; the release carries none'**
  String releaseNoneCarried(int version);

  /// Below is the row updatesNodeFirmware, with its button sendToNode.
  ///
  /// In en, this message translates to:
  /// **'node firmware {version} is out: send it below'**
  String releaseFirmwareOut(int version);

  /// No description provided for @releaseFirmwareUpToDate.
  ///
  /// In en, this message translates to:
  /// **'node firmware {version}, up to date'**
  String releaseFirmwareUpToDate(int version);

  /// The help of Settings' section myIdentity, on the identity file. Restore from a file is the button identityRestore; the box is identityAddMessages.
  ///
  /// In en, this message translates to:
  /// **'The file holds your identity, name, home, contacts and the nodes you paired with, and your messages if you tick the box. Your password locks it: without it nobody can open the file, not even you.\n\nOn the new phone, tap Restore from a file, then pair with your nodes again: a pairing stays on the phone it was made on. Your home keeps your mail meanwhile, and you can fetch it from any other node.\n\nUse one phone at a time: two phones with one identity would each take mail the other never sees.\n\nYour 12 words bring back your identity alone.'**
  String get identityHelp;

  /// Under the heading myIdentity: 'it' is the identity.
  ///
  /// In en, this message translates to:
  /// **'Save it to a file to move to a new phone.'**
  String get identitySaveHow;

  /// A button that saves the identity file.
  ///
  /// In en, this message translates to:
  /// **'Save to a file'**
  String get identitySave;

  /// A button that restores the identity from an identity file.
  ///
  /// In en, this message translates to:
  /// **'Restore from a file'**
  String get identityRestore;

  /// The title of the dialog that takes the identity file's new password.
  ///
  /// In en, this message translates to:
  /// **'Save my identity'**
  String get identitySaveTitle;

  /// The identity file was saved.
  ///
  /// In en, this message translates to:
  /// **'Saved'**
  String get identitySaved;

  /// The person closed the phone's file dialog without saving.
  ///
  /// In en, this message translates to:
  /// **'Not saved'**
  String get identityNotSaved;

  /// No description provided for @identitySaveFailed.
  ///
  /// In en, this message translates to:
  /// **'Saving failed: {error}'**
  String identitySaveFailed(String error);

  /// Why saving the identity file failed: the place picked could not be written; lower case, after identitySaveFailed's words.
  ///
  /// In en, this message translates to:
  /// **'no stream for {uri}'**
  String identityNoStream(String uri);

  /// Why saving the identity file failed: Android's save dialog is open already; it follows identitySaveFailed's words, so lower case.
  ///
  /// In en, this message translates to:
  /// **'a save is under way'**
  String get identitySaveBusy;

  /// No description provided for @identityRestoreTitle.
  ///
  /// In en, this message translates to:
  /// **'Restore from a file?'**
  String get identityRestoreTitle;

  /// No description provided for @identityRestoreHow.
  ///
  /// In en, this message translates to:
  /// **'This replaces the identity, contacts and messages on this phone.'**
  String get identityRestoreHow;

  /// The button of the dialog identityRestoreTitle: it opens the phone's file picker.
  ///
  /// In en, this message translates to:
  /// **'Choose file'**
  String get identityChooseFile;

  /// Opening the picked file failed.
  ///
  /// In en, this message translates to:
  /// **'Opening failed: {error}'**
  String identityOpenFailed(String error);

  /// The title of the dialog that takes the password of the identity file being restored.
  ///
  /// In en, this message translates to:
  /// **'The file\'s password'**
  String get identityPasswordTitle;

  /// No description provided for @identityRestoredPairAgain.
  ///
  /// In en, this message translates to:
  /// **'Identity restored. Pair again with your nodes.'**
  String get identityRestoredPairAgain;

  /// The identity file was not read. {reason} is a sentence with its full stop, such as identityWrongPassword.
  ///
  /// In en, this message translates to:
  /// **'{reason} Nothing changed.'**
  String identityRefused(String reason);

  /// bark is the app's name, always in lower case.
  ///
  /// In en, this message translates to:
  /// **'This is not a bark identity file.'**
  String get identityNotFile;

  /// No description provided for @identityOtherLayout.
  ///
  /// In en, this message translates to:
  /// **'This file is from another version of the app.'**
  String get identityOtherLayout;

  /// No description provided for @identityWrongPassword.
  ///
  /// In en, this message translates to:
  /// **'Wrong password, or the file is damaged.'**
  String get identityWrongPassword;

  /// The label of the field where the identity file's password is typed.
  ///
  /// In en, this message translates to:
  /// **'Password'**
  String get identityPassword;

  /// The label of the field where a new password is typed a second time.
  ///
  /// In en, this message translates to:
  /// **'Password again'**
  String get identityPasswordAgain;

  /// Under identityPasswordAgain: the two passwords typed differ.
  ///
  /// In en, this message translates to:
  /// **'Not the same'**
  String get identityNotSame;

  /// A box to tick: the identity file holds the person's messages too.
  ///
  /// In en, this message translates to:
  /// **'Add my messages'**
  String get identityAddMessages;

  /// In the dialog identitySaveTitle: 'it' is the identity.
  ///
  /// In en, this message translates to:
  /// **'Use it on one phone at a time.'**
  String get identityOnePhone;
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
