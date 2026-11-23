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

  @override
  String get cancel => 'Cancel';

  @override
  String get save => 'Save';

  @override
  String get notNow => 'Not now';

  @override
  String get timeJustNow => 'just now';

  @override
  String timeMinutesAgo(int minutes) {
    return '$minutes min ago';
  }

  @override
  String timeHoursAgo(int hours) {
    return '$hours h ago';
  }

  @override
  String timeDaysAgo(int days) {
    String _temp0 = intl.Intl.pluralLogic(
      days,
      locale: localeName,
      other: '$days days ago',
      one: '$days day ago',
    );
    return '$_temp0';
  }

  @override
  String get timeNever => 'never';

  @override
  String get timeToday => 'today';

  @override
  String get timeYesterday => 'yesterday';

  @override
  String get sentQueued => 'waiting for a node';

  @override
  String get sentWriting => 'being handed to the node';

  @override
  String get sentTaken => 'sent into the network';

  @override
  String get sentBusyNow => 'the network is busy, sending again now';

  @override
  String sentBusyIn(String wait) {
    return 'the network is busy, sending again in $wait';
  }

  @override
  String get sentRefused => 'refused by the node';

  @override
  String sentPartsSent(int taken, int total) {
    return '$taken of $total sent';
  }

  @override
  String sentPartProblem(int part, String state) {
    return 'part $part: $state';
  }

  @override
  String get sentQueuedHelp =>
      'On this phone, until a node is linked and ready for it.';

  @override
  String get sentWritingHelp => 'The phone is giving it to the node.';

  @override
  String get sentTakenHelp =>
      'The node has it and passes it on. The network gives no further news: it does not say when it arrives.';

  @override
  String get sentBusy => 'The network is busy';

  @override
  String get sentBusyHelp =>
      'The network gave it back for now. The app sends it again by itself once the wait is over.';

  @override
  String get sentRefusedHelp =>
      'The node will not take it as it is, a fault of the app or the node. Cancel removes it; Debug shows the reason.';

  @override
  String get sentReaction => 'Their reaction';

  @override
  String get sentReactionHelp =>
      'The emoji on the text\'s corner, if they picked one.';

  @override
  String get receivedYourReaction => 'Your reaction';

  @override
  String get receivedYourReactionHelp =>
      'Press and hold a text to react with an emoji, or to copy it. They see it once it arrives. Pick another to change it.';

  @override
  String get receivedFirst => 'First message';

  @override
  String get receivedFirstHelp =>
      'Anyone with your card could have sent a first message, so this one is not proven to be theirs.';

  @override
  String get receivedMailbox => 'From your mailbox';

  @override
  String get receivedMailboxHelp =>
      'It waited at your home node and came when you fetched your mail.';

  @override
  String get receivedParts => 'Parts';

  @override
  String get receivedPartsHelp =>
      'A long text comes in parts, in any order. Italics stand for a part not yet here.';

  @override
  String get receivedTime => 'The time';

  @override
  String get receivedTimeHelp =>
      'When it came. When it was written is under This message.';

  @override
  String get commonThisMessage => 'This message';

  @override
  String commonNotPaired(String reason) {
    return '$reason. Still a guest here.';
  }

  @override
  String commonPairingFailed(String error) {
    return 'Pairing failed: $error. Still a guest here.';
  }

  @override
  String commonGift(String name, String node) {
    return '$name gave you a home at $node';
  }

  @override
  String get commonGiftTitle => 'A home given';

  @override
  String get commonGiftHelp =>
      'The owner of a home node added you there. Used, it is your home: it keeps texts to you while you are away, and your card carries its address, so your contacts get your new card.\n\nA home you had before sends you its mail, then forgets you.';

  @override
  String get commonUseGift => 'Use it';

  @override
  String commonFailed(String error) {
    return 'Failed: $error';
  }

  @override
  String get commonHomeOffer =>
      'No home node yet. Make this one your home? It keeps texts to you while you are away. You pair first, with the code on its screen.';

  @override
  String get commonSetHome => 'Set home';

  @override
  String get commonBluetoothSettings => 'Bluetooth settings';

  @override
  String commonBluetoothSettingsFailed(String error) {
    return 'Opening Bluetooth settings failed: $error';
  }

  @override
  String get commonFetchQueued => 'Fetch waits for a node';

  @override
  String commonFetchWaiting(String wait) {
    return 'Waiting for your home node to answer, $wait at most';
  }

  @override
  String get commonFetch => 'Fetch my mail';

  @override
  String commonFetchFailed(String error) {
    return 'Fetching failed: $error';
  }

  @override
  String get commonCardNoHome => 'Your card changed: no home';

  @override
  String get commonCardNewHome => 'Your card changed: new home address';

  @override
  String commonCardOld(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Your $count contacts have the old one.',
      one: 'Your contact has the old one.',
    );
    return '$_temp0';
  }

  @override
  String get commonCardQueued => 'Your card is queued';

  @override
  String commonCardFailed(String error) {
    return 'Sending the card failed: $error';
  }

  @override
  String get commonSendCard => 'Send my card';

  @override
  String get commonMyName => 'My name';

  @override
  String commonAbout(String title) {
    return 'About $title';
  }

  @override
  String get commonWhyThisNode => 'Why this node';

  @override
  String get commonWhyBluetooth =>
      'bark texts through a node near you, over Bluetooth. Android asks next.';

  @override
  String get commonContinue => 'Continue';

  @override
  String get statusPublicNode => 'a public node';

  @override
  String statusLooking(String node) {
    return 'Not connected. Looking for $node; texts wait on the phone';
  }

  @override
  String statusBluetoothOff(String node) {
    return 'Not connected. Bluetooth is off: turn it on to reach $node';
  }

  @override
  String get statusNotLooking => 'Not connected. Texts wait on the phone';

  @override
  String statusGuestFor30s(String node, String install) {
    return 'Connected to $node as a guest for 30 s$install';
  }

  @override
  String statusGuest(String node, String install) {
    return 'Connected to $node as a guest$install';
  }

  @override
  String statusAtHome(String node, String install) {
    return 'Connected to your home, $node$install';
  }

  @override
  String statusPaired(String node, String install) {
    return 'Connected to $node, paired$install';
  }

  @override
  String statusHomeAt(String node) {
    return 'Home at $node';
  }

  @override
  String statusStarting(String wait) {
    return 'Starting up, $wait';
  }

  @override
  String get statusHearsNone => 'Hears no other node yet: texts wait here';

  @override
  String statusHears(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Hears $count nodes',
      one: 'Hears 1 node',
    );
    return '$_temp0';
  }

  @override
  String get signalUnknown => 'signal unknown';

  @override
  String get signalStrong => 'strong signal';

  @override
  String get signalFair => 'fair signal';

  @override
  String get signalWeak => 'weak signal';

  @override
  String connectionLostOne(String node) {
    return '$node does not know this phone: pair again with it in Settings.';
  }

  @override
  String connectionLostMany(String nodes) {
    return '$nodes do not know this phone: pair again with them in Settings.';
  }

  @override
  String connectionUnservedOne(String node) {
    return '$node cannot be used yet: it or this app needs an update. Tap it in Settings to try again.';
  }

  @override
  String connectionUnservedMany(String nodes) {
    return '$nodes cannot be used yet: they or this app need an update. Tap one in Settings to try again.';
  }

  @override
  String get connectionAtHome =>
      'Your home. The phone stays linked while it is in reach, so texts come at once, also with the screen off. It always goes to your home first.';

  @override
  String get connectionPaired =>
      'A node you are paired with. The phone stays linked while it is in reach.';

  @override
  String get connectionPairedNoHome =>
      'With no home, your card names this node while you use it.';

  @override
  String get connectionPairedHome =>
      'Texts to you wait at your home; Fetch my mail brings them here.';

  @override
  String get connectionMovesHome =>
      'When your home is in reach, the phone moves to it within a minute.';

  @override
  String get connectionTappedGuest =>
      'Someone else\'s public node, which you chose. It serves one guest at a time and ends a guest\'s link after 30 seconds; then the phone chooses a node itself.';

  @override
  String get connectionGuestWeak =>
      'Your paired nodes are heard too weakly here, so the phone uses this public node until it hears one well.';

  @override
  String get connectionGuestAway =>
      'None of your paired nodes has been in reach for 30 seconds, so the phone uses this public node until it hears one.';

  @override
  String get connectionGuestNoneNear =>
      'No node of yours is near, so the phone uses this public node, someone else\'s.';

  @override
  String get connectionGuestVisits =>
      'As a guest it visits about once a minute, and at once when you send. Replies to texts sent from here come here.';

  @override
  String get connectionBluetoothOff =>
      'Bluetooth is off. Texts wait on the phone until it is on and a node is in reach.';

  @override
  String get connectionLookPublic =>
      'No node of yours to use, so the phone looks every 15 seconds for a public node to use as a guest.';

  @override
  String get connectionLookPaired =>
      'The phone looks every 15 seconds for your paired nodes, your home first. After 30 seconds without one, it uses a public node as a guest meanwhile.';

  @override
  String get connectionNotLooking =>
      'The phone is not looking: you stopped it, or it may not use Bluetooth yet. Tap a node in Settings to connect.';

  @override
  String get notifyTextsArrive => 'Texts arrive while the screen is off';

  @override
  String get notifyChecksMinute => 'Checks for texts about once a minute';

  @override
  String get notifyOutInReach => 'Texts waiting go out once it is in reach';

  @override
  String get notifyOutBluetooth => 'Texts waiting go out once Bluetooth is on';

  @override
  String get notifyUnnamed => 'Unnamed';

  @override
  String get notifyChannel => 'Texts';

  @override
  String get notifyChannelAbout => 'A text from a contact';

  @override
  String get notifyYou => 'You';

  @override
  String get threadRemovedContact => 'Removed contact';

  @override
  String threadPartMissing(int part, int total) {
    return 'part $part of $total not yet here';
  }

  @override
  String get threadUnverifiedMark =>
      'Came as a first message, which anyone could have made';

  @override
  String get threadToNode => 'To node';

  @override
  String get threadAt => 'At';

  @override
  String get threadSealed => 'Sealed';

  @override
  String get threadSealedForThem => 'for them alone';

  @override
  String get threadSealedIntroOut => 'as a first message, with your card';

  @override
  String get threadSealedForYou => 'by them, for you alone';

  @override
  String get threadSealedIntroIn => 'to you, as a first message';

  @override
  String threadMessageIds(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Message IDs',
      one: 'Message ID',
    );
    return '$_temp0';
  }

  @override
  String get threadWritten => 'Written';

  @override
  String get threadPartsHere => 'Parts here';

  @override
  String threadPartsOf(int here, int total) {
    return '$here of $total';
  }

  @override
  String get threadSendTo => 'Send to';

  @override
  String threadLastSeenThere(String ago) {
    return 'Last seen there $ago';
  }

  @override
  String threadWhereWas(String name, String ago) {
    return 'Where $name was $ago';
  }

  @override
  String get threadThereHow => 'Fast if they are still near there';

  @override
  String get threadTheirHome => 'Their home node';

  @override
  String get threadHomeHow => 'Waits at their home until they fetch it';

  @override
  String threadNotSent(String error) {
    return 'Not sent: $error';
  }

  @override
  String get threadNotWritten => 'Has not written yet';

  @override
  String threadLastWrote(String ago) {
    return 'Last wrote $ago';
  }

  @override
  String threadLastWroteHome(String ago) {
    return 'Last wrote $ago, from their home';
  }

  @override
  String threadLastWroteAway(String ago) {
    return 'Last wrote $ago, away from home';
  }

  @override
  String get threadRename => 'Rename';

  @override
  String get threadShareHome => 'Share my home node';

  @override
  String get threadArchiveConversation => 'Archive conversation';

  @override
  String contactArchived(String name) {
    return '$name: contact archived';
  }

  @override
  String get threadArchiveContact => 'Archive contact';

  @override
  String threadRemoveTitle(String name) {
    return 'Remove $name?';
  }

  @override
  String get threadRemoveHow =>
      'Their texts stay; to write again, add their card.';

  @override
  String get threadRemove => 'Remove';

  @override
  String get threadRemoveContact => 'Remove contact';

  @override
  String get threadNotVerified =>
      'Not verified yet: their first reply proves it is them.';

  @override
  String get threadIsArchived => 'This conversation is archived.';

  @override
  String get threadWasRemoved => 'This contact was removed.';

  @override
  String threadNoNode(String name) {
    return '$name\'s card names no node yet: ask them for a new one.';
  }

  @override
  String get name => 'Name';

  @override
  String threadParts(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count parts',
      one: '$count part',
    );
    return '$_temp0';
  }

  @override
  String threadTooManyParts(int count, int max) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count parts, $max at most',
      one: '$count part, $max at most',
    );
    return '$_temp0';
  }

  @override
  String get threadHint => 'Text';

  @override
  String get threadSend => 'Send';

  @override
  String threadTheyReacted(String emoji) {
    return 'They reacted: $emoji';
  }

  @override
  String threadYouReacted(String emoji) {
    return 'You reacted: $emoji';
  }

  @override
  String get threadCopy => 'Copy text';

  @override
  String get threadToWhere => 'To where they were';

  @override
  String get threadToHome => 'To their home';

  @override
  String get threadTakenMark =>
      'Sent into the network, which does not say when it arrives';

  @override
  String threadReactionSeen(String name) {
    return '$name sees your reaction on their text.';
  }

  @override
  String get emojiSearch => 'Search emoji by name';

  @override
  String get emojiNone => 'No emoji by that name.';

  @override
  String get emojiSmileys => 'Smileys & emotion';

  @override
  String get emojiPeople => 'People & body';

  @override
  String get emojiAnimals => 'Animals & nature';

  @override
  String get emojiFood => 'Food & drink';

  @override
  String get emojiTravel => 'Travel & places';

  @override
  String get emojiActivities => 'Activities';

  @override
  String get emojiObjects => 'Objects';

  @override
  String get emojiSymbols => 'Symbols';

  @override
  String get emojiFlags => 'Flags';

  @override
  String get sendNoReturnAddress => 'no address to answer to yet';

  @override
  String sendNoAddress(String name) {
    return '$name has no address yet';
  }

  @override
  String sendTooLong(int count, int max) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'too long: $count parts, $max at most',
      one: 'too long: $count part, $max at most',
    );
    return '$_temp0';
  }

  @override
  String get sendCharTooLong => 'a character too long to send';

  @override
  String sessionBadValue(String error) {
    return 'The node sent a value this app cannot read: $error';
  }

  @override
  String sessionReadFailed(String error) {
    return 'Reading the node failed: $error';
  }

  @override
  String get sessionNotConnected => 'not connected';

  @override
  String sessionPairedOnly(String what) {
    return '$what is the paired phone\'s alone';
  }

  @override
  String get sessionHomeTaken =>
      'Your home no longer keeps your mail. Set it again in Settings.';

  @override
  String sessionGivenBackUnknown(String id) {
    return 'The node gave back message $id, which this app did not send.';
  }

  @override
  String sessionSendFailed(String error) {
    return 'Sending failed: $error';
  }

  @override
  String sessionConfirmRefused(String error) {
    return 'The node refused the inbox confirmation: $error';
  }

  @override
  String sessionInboxFailed(String error) {
    return 'Reading the inbox failed: $error';
  }

  @override
  String sessionPlaceFailed(String error) {
    return 'Setting the node\'s place failed: $error';
  }

  @override
  String get sessionNoNewMail => 'No new mail';

  @override
  String sessionMoreAtHome(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count more at home: fetch again for them',
      one: '$count more at home: fetch again for them',
    );
    return '$_temp0';
  }

  @override
  String sessionClockNone(String offset) {
    return 'no time, offset $offset';
  }

  @override
  String sessionClockGps(String time, String offset) {
    return '$time from the gps, offset $offset';
  }

  @override
  String sessionClockPhone(String time, String offset) {
    return '$time from the phone, offset $offset';
  }

  @override
  String get card => 'Card';

  @override
  String get scanTheirCard => 'Scan their card';

  @override
  String get contactsNone =>
      'No contacts yet. Tap Add contact and scan their card.';

  @override
  String contactsArchived(int count) {
    return '$count archived';
  }

  @override
  String get contactsAdd => 'Add contact';

  @override
  String get contactsNotVerified => 'Not verified yet';

  @override
  String get contactsHasHome => 'Has a home node';

  @override
  String get contactsTemporaryNoNode => 'temporary: no node known yet';

  @override
  String contactsTemporaryAt(String ago) {
    return 'temporary: at the node they used $ago';
  }

  @override
  String get contactsPasteHelp => 'Or paste the card they sent you';

  @override
  String get contactsHomeNode => 'Home node';

  @override
  String get contactsNoHomeNoNode =>
      'none, and no node yet: ask them for a new card';

  @override
  String get contactsNoHomeTemporary => 'none: a temporary card';

  @override
  String contactsBadPaste(String error, String prefix) {
    return '$error. Paste a card: it starts with $prefix';
  }

  @override
  String contactsUnnamed(String start) {
    return 'Unnamed ($start…)';
  }

  @override
  String get cardShare => 'Share';

  @override
  String cardShareFailed(String error) {
    return 'Sharing failed: $error';
  }

  @override
  String get cardNameNotSet => 'not set';

  @override
  String get cardChangeName => 'Change';

  @override
  String get cardDetails => 'Details';

  @override
  String get cardMyIdentity => 'My identity';

  @override
  String get cardCopyIdentity => 'Copy the identity';

  @override
  String get cardIdentityCopied => 'The identity copied';

  @override
  String get cardHomeAddress => 'Home address';

  @override
  String get cardCopyAddress => 'Copy the address';

  @override
  String get cardAddressCopied => 'The address copied';

  @override
  String get cardNodeNow => 'The node I use now';

  @override
  String get cardCopyNode => 'Copy the node';

  @override
  String get cardNodeCopied => 'The node copied';

  @override
  String get cardCopyCard => 'Copy the card';

  @override
  String get cardCardCopied => 'The card copied';

  @override
  String get cardNameFirst =>
      'Set your name first: whoever scans your card needs to know it.';

  @override
  String get cardSetName => 'Set my name';

  @override
  String get cardTemporaryHow =>
      'Temporary card: with no home node, it names the node you use now. Set a home in Settings.';

  @override
  String get cardHomeHow => 'It carries your home node\'s address.';

  @override
  String get cardTemporary => 'Temporary card';

  @override
  String get cardTemporaryMark => 'TEMPORARY';

  @override
  String get cardNot => 'Not a card';

  @override
  String cardNewer(int version) {
    return 'A card from a newer app (card version $version)';
  }

  @override
  String cardOlder(int version) {
    return 'A card from an older app (card version $version)';
  }

  @override
  String get cardNameNotUtf8 => 'Not a card: the name is not UTF-8';

  @override
  String get cardNameTooLong => 'The name is too long';

  @override
  String get scanTitle => 'Scan a card';

  @override
  String get scanNoCamera =>
      'The camera is not allowed. Allow it in the phone\'s settings, or paste the card.';

  @override
  String get scanPoint => 'Point the camera at the card on the other phone';

  @override
  String get factRole =>
      'A relay has outside power, such as USB, and listens all the time, passing on texts for the nodes around it: the more relays, the farther texts reach. A sleepy node is on battery: to last, it sleeps and wakes only in short windows, and sends and receives through one relay near it, its anchor. A fixed relay that loses its power keeps relaying on battery until the battery runs low, and then turns sleepy. Holding the node\'s button switches its role until its power next changes.';

  @override
  String get factMoving =>
      'The node tells this itself from its GPS: after a little over a minute of positions away from where it stood, it counts as carried, and after four hours in one place it stays put again. A carried node\'s place follows its GPS, and it keeps no new mail for others, as it may soon be gone. Setting its place in the app\'s Settings makes it stay put at once.';

  @override
  String get factAnchor =>
      'A sleepy node sends and receives through one relay near it, its anchor. The anchor keeps the sleepy node\'s mail and hands it over in the short window after each time it wakes, and the sleepy node\'s address is its anchor\'s place. A sleepy node takes the first relay it hears as its anchor; with none yet it holds and passes on texts itself, at a higher cost to the channel.';

  @override
  String get factPlace =>
      'Texts find a node by its place: an address is a position and the node\'s ID, and each relay passes a text on toward that position. The place comes from the node\'s GPS, unless a person set it in the app\'s Settings, which wins, as for a relay indoors with no view of the sky. A node with no place yet sends no heartbeat, so no text can find it until it has one.';

  @override
  String get factPower =>
      'USB means outside power, on which a node can relay all the time. On battery the figure is read from the cell\'s voltage, so it is rough; on this node\'s own sheet it is the raw reading in millivolts. At about a fifth left the battery counts as low, and a relay on it turns sleepy to save the rest.';

  @override
  String get factLink =>
      'How well this node hears the other one: how far its signal is above the radio noise here. Below about 4 dB a full text is often lost when the signal fades, so the link is fragile and used only when no other is left; from about 7 dB it is good; between, fair. It is measured one way: the other node may hear this one better or worse.';

  @override
  String get factLastHeard =>
      'Every node sends a short heartbeat now and then, saying who and where it is. A relay sends one every 5 to 15 seconds, each on the next of 13 channels, so this node hears it about once every one to three minutes; a sleepy node wakes to send one every two and a half to five minutes. A relay not heard for six and a half minutes, or a sleepy node for ten, leaves the list.';

  @override
  String get factDistance =>
      'The straight line from this node\'s place to the other\'s, worked out by the app from the two positions, so it is shown only when both have a place. How far radio reaches depends on height and on what stands between: relays on rooftops hear each other at about 3 km.';

  @override
  String get factKnown =>
      'What the node says of its own neighbours, in three counts. Heard: the nodes whose heartbeats it hears. Strong: of those, the ones heard well enough that a full text gets through a fade, so they likely hear it too. Exchanged: of those, the ones that have taken a text from it, which proves they hear it.';

  @override
  String factGpsSignal(int floor) {
    return 'How strongly the node\'s GPS hears the satellites, in dB-Hz, as its last reading had it. Under $floor the node takes no position from it; an open sky gives about 40. Move a node to where it reads higher, or set its place by hand.';
  }

  @override
  String get factVersion =>
      'The version of the firmware the node runs. A new version spreads by itself: a node given it, from a phone or over USB, passes it to the relays around it a piece at a time, leaving the channels to texts. A node takes only a newer version signed by the project, and while it gathers one this row says which and how many of its blocks it holds.';

  @override
  String get factExchanged =>
      'This node has handed it a text and it took it. That proves the link works both ways: it hears this node, as well as being heard.';

  @override
  String get factDemoted =>
      'It did not answer this node\'s last three tries to pass it a text, so it may no longer hear this node. Until it answers again, or for six and a half minutes, it is tried only after every other neighbour with a good enough link.';

  @override
  String get factAvoided =>
      'It turned a text down, asking for a wait, or did not answer one. Nothing is sent to it for a while; texts go round it meanwhile, and wait for it only where there is no other way.';

  @override
  String get factPaired =>
      'This phone is paired with this node: the node sends your texts into the network, and keeps the ones for you until the app reads them. Only the paired phone sees the node\'s neighbours on this map.';
}
