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
  /// **'The node has it and passes it on. The network gives no further news: it does not say when it arrives.'**
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

  /// No description provided for @commonFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed: {error}'**
  String commonFailed(String error);

  /// No description provided for @commonHomeOffer.
  ///
  /// In en, this message translates to:
  /// **'No home node yet. Make this one your home? It keeps texts to you while you are away. You pair first, with the code on its screen.'**
  String get commonHomeOffer;

  /// No description provided for @commonSetHome.
  ///
  /// In en, this message translates to:
  /// **'Set home'**
  String get commonSetHome;

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

  /// {install} is what the node's install adds: empty, or a clause that begins with a comma, such as ', installing version 5'.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest for 30 s{install}'**
  String statusGuestFor30s(String node, String install);

  /// {install} as in statusGuestFor30s.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node} as a guest{install}'**
  String statusGuest(String node, String install);

  /// {install} as in statusGuestFor30s.
  ///
  /// In en, this message translates to:
  /// **'Connected to your home, {node}{install}'**
  String statusAtHome(String node, String install);

  /// {install} as in statusGuestFor30s.
  ///
  /// In en, this message translates to:
  /// **'Connected to {node}, paired{install}'**
  String statusPaired(String node, String install);

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

  /// The person, in a notification of a conversation.
  ///
  /// In en, this message translates to:
  /// **'You'**
  String get notifyYou;

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

  /// No description provided for @threadContactArchived.
  ///
  /// In en, this message translates to:
  /// **'{name}: contact archived'**
  String threadContactArchived(String name);

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

  /// The button that removes a contact, in the dialog that asks.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get threadRemove;

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

  /// The title of the field that renames a contact.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get threadName;

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
  String get sessionNotConnected;

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

  /// No description provided for @sessionSendFailed.
  ///
  /// In en, this message translates to:
  /// **'Sending failed: {error}'**
  String sessionSendFailed(String error);

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
