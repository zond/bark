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
  String get sentNoNeighbour => 'the node hears no other, sending when it does';

  @override
  String get sentNoWay => 'found no way there';

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
      'The node has it and passes it on. The network does not say when it arrives; it gives it back if it finds no way there.';

  @override
  String get sentBusy => 'The network is busy';

  @override
  String get sentBusyHelp =>
      'The network gave it back for now. The app sends it again by itself once the wait is over.';

  @override
  String get sentNoNeighbourHelp =>
      'The node hears no other node now. The app sends it again by itself once it does.';

  @override
  String get sentNoWayHelp =>
      'The network found no way to their address: their node may be off or gone, or their address old. Send again tries once more, as a new message; Cancel removes it.';

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
  String failed(String error) {
    return 'Failed: $error';
  }

  @override
  String get commonHomeOffer =>
      'No home node yet. Make this one your home? It keeps texts to you while you are away. You pair first, with the code on its screen.';

  @override
  String get setHome => 'Set home';

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
  String statusGuestFor30s(String node) {
    return 'Connected to $node as a guest for 30 s';
  }

  @override
  String statusGuestFor30sInstalling(String node, int version) {
    return 'Connected to $node as a guest for 30 s, installing version $version';
  }

  @override
  String statusGuestFor30sUpdated(String node, int version) {
    return 'Connected to $node as a guest for 30 s, updated to version $version';
  }

  @override
  String statusGuest(String node) {
    return 'Connected to $node as a guest';
  }

  @override
  String statusGuestInstalling(String node, int version) {
    return 'Connected to $node as a guest, installing version $version';
  }

  @override
  String statusGuestUpdated(String node, int version) {
    return 'Connected to $node as a guest, updated to version $version';
  }

  @override
  String statusAtHome(String node) {
    return 'Connected to your home, $node';
  }

  @override
  String statusAtHomeInstalling(String node, int version) {
    return 'Connected to your home, $node, installing version $version';
  }

  @override
  String statusAtHomeUpdated(String node, int version) {
    return 'Connected to your home, $node, updated to version $version';
  }

  @override
  String statusPaired(String node) {
    return 'Connected to $node, paired';
  }

  @override
  String statusPairedInstalling(String node, int version) {
    return 'Connected to $node, paired, installing version $version';
  }

  @override
  String statusPairedUpdated(String node, int version) {
    return 'Connected to $node, paired, updated to version $version';
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
  String get notifyLinkChannel => 'Connection to the node';

  @override
  String get notifyLinkChannelAbout =>
      'Shown while the app keeps the link to a node or looks for it';

  @override
  String get you => 'You';

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
  String get remove => 'Remove';

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
  String get threadCancelSending => 'Cancel sending';

  @override
  String get threadReact => 'React';

  @override
  String get threadSendAgain => 'Send again';

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
  String sendTextNoWay(String name) {
    return 'Your text to $name found no way there.';
  }

  @override
  String sendReactionNoWay(String emoji, String name) {
    return 'Your reaction $emoji found no way to $name.';
  }

  @override
  String sendCardNoWay(String name) {
    return 'Your card found no way to $name.';
  }

  @override
  String sendHomeNoWay(String name) {
    return 'The home you gave $name found no way to them.';
  }

  @override
  String sessionBadValue(String error) {
    return 'The node sent a value this app cannot read: $error';
  }

  @override
  String sessionReadFailed(String error) {
    return 'Reading the node failed: $error';
  }

  @override
  String get notConnected => 'not connected';

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
  String sendFailed(String error) {
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
  String get homeNode => 'Home node';

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
  String get nameNotSet => 'not set';

  @override
  String get changeName => 'Change';

  @override
  String get cardDetails => 'Details';

  @override
  String get myIdentity => 'My identity';

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
    return 'How strongly the node\'s GPS hears the satellites, in dB-Hz, at its last reading, and how long ago that was: a node that stands still rests its GPS, so the reading can be old. Under $floor the node takes no position from it; an open sky gives about 40. Move a node to where it reads higher, or set its place by hand.';
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

  @override
  String get factRoleLabel => 'Role';

  @override
  String get factMovingLabel => 'Moving';

  @override
  String get factAnchorLabel => 'Anchor';

  @override
  String get factPlaceLabel => 'Place';

  @override
  String get factPowerLabel => 'Power';

  @override
  String get factLinkLabel => 'Link';

  @override
  String get factLastHeardLabel => 'Last heard';

  @override
  String get factDistanceLabel => 'Distance';

  @override
  String get factKnownLabel => 'Known';

  @override
  String get gpsSignal => 'GPS signal';

  @override
  String get factVersionLabel => 'Version';

  @override
  String get factExchangedLabel => 'Exchanged';

  @override
  String get factDemotedLabel => 'Demoted';

  @override
  String get factAvoidedLabel => 'Avoided';

  @override
  String get factPairedLabel => 'Paired';

  @override
  String get factRoleSleepyName => 'role, sleepy';

  @override
  String get factRoleRelayName => 'role, relay';

  @override
  String get factMovingName => 'moving';

  @override
  String get factAnchorName => 'anchor';

  @override
  String get factPlaceName => 'place';

  @override
  String get factPlaceByPersonName => 'place, set by a person';

  @override
  String get factPlaceFromGpsName => 'place, from GPS';

  @override
  String get factPlaceAnchorsName => 'place, its anchor\'s';

  @override
  String get factPowerName => 'power';

  @override
  String get factBatteryName => 'battery';

  @override
  String get factLinkGoodName => 'link strength, good';

  @override
  String get factLinkFairName => 'link strength, fair';

  @override
  String get factLinkFragileName => 'link strength, fragile';

  @override
  String get factLastHeardName => 'last heard';

  @override
  String get factDistanceName => 'distance';

  @override
  String get factKnownName => 'neighbours known';

  @override
  String get factVersionName => 'firmware version';

  @override
  String get factPairedName => 'paired';

  @override
  String get factExchangedName => 'exchanged';

  @override
  String get factDemotedName => 'demoted';

  @override
  String get factAvoidedName => 'avoided';

  @override
  String get factRoleSleepy =>
      'sleepy: wakes in windows, speaks through its anchor';

  @override
  String get factRoleRelay => 'relay: passes on others\' texts';

  @override
  String get factCarried => 'carried';

  @override
  String get factFixed => 'stays put';

  @override
  String get factAnchored => 'anchored';

  @override
  String factAnchoredTo(String node) {
    return 'anchored to $node';
  }

  @override
  String get factNoAnchor => 'no anchor yet';

  @override
  String get factThisNodesAnchor => 'this node\'s anchor';

  @override
  String get factNoPlace => 'no place yet';

  @override
  String factPlaceByPerson(String place) {
    return 'set by a person\n$place';
  }

  @override
  String factPlaceFromGps(String place) {
    return 'from GPS\n$place';
  }

  @override
  String factPlaceAnchors(String place) {
    return 'its anchor\'s\n$place';
  }

  @override
  String get factBatteryLow => 'low';

  @override
  String get notRead => 'not read';

  @override
  String factPercent(int percent) {
    return '$percent%';
  }

  @override
  String millivolts(int voltage) {
    return '$voltage mV';
  }

  @override
  String get noReading => 'no reading';

  @override
  String factGpsTooWeak(int signal) {
    return '$signal dB-Hz · too weak for a fix';
  }

  @override
  String factGpsEnough(int signal) {
    return '$signal dB-Hz · enough for a fix';
  }

  @override
  String get factGpsNoSatellites => 'no satellites';

  @override
  String factGpsAgo(String reading, String ago) {
    return '$reading, $ago';
  }

  @override
  String get factLinkGood => 'good';

  @override
  String get factLinkFair => 'fair';

  @override
  String get factLinkFragile => 'fragile';

  @override
  String factLinkDetail(String rssi, String margin) {
    return 'This link: $rssi dBm, $margin dB over the noise floor.';
  }

  @override
  String timeSecondsAgo(int seconds) {
    return '$seconds s ago';
  }

  @override
  String factMetres(int metres) {
    return '$metres m';
  }

  @override
  String factKilometres(String kilometres) {
    return '$kilometres km';
  }

  @override
  String factKnownValue(int heard, int strong, int exchanged) {
    return '$heard heard · $strong strong · $exchanged exchanged';
  }

  @override
  String factVersionGetting(
    String version,
    String gathers,
    int complete,
    int blocks,
  ) {
    return '$version · getting $gathers $complete/$blocks';
  }

  @override
  String get factExchangedValue => 'it took a message from this node';

  @override
  String get factDemotedValue => 'it did not answer its last texts';

  @override
  String get factAvoidedValue =>
      'for now: it turned a text down or did not answer';

  @override
  String get factPairedWithThis => 'with this phone';

  @override
  String get mapShowMe => 'Show where I am';

  @override
  String get mapReadAgain => 'Read again';

  @override
  String get mapNotConnected =>
      'Not connected to a node: connect to see its neighbours.';

  @override
  String get mapGuest =>
      'Connected as a guest: only the phone paired with this node sees its neighbours.';

  @override
  String mapReadFailed(String error) {
    return 'Reading the neighbours failed: $error';
  }

  @override
  String get mapNothing =>
      'The node has no place yet and has heard no neighbour: nothing to draw.';

  @override
  String get mapAttribution => '© OpenStreetMap contributors';

  @override
  String get mapNoPlaceNoLines =>
      'The node has no place of its own: its neighbours are drawn without lines to it.';

  @override
  String get mapNoPlaceAtYou =>
      'The node has no place of its own: it is drawn where you are.';

  @override
  String mapNeighboursNoPlace(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count neighbours have no place: they are drawn at this node',
      one: '$count neighbour has no place: it is drawn at this node',
    );
    return '$_temp0';
  }

  @override
  String get mapNoTiles =>
      'No street map here: the phone has no internet and has not kept this area from before. The nodes and links are drawn on a blank background.';

  @override
  String mapLastReadFailed(String error) {
    return 'The last read failed: $error';
  }

  @override
  String get mapKindSleepy => 'sleepy';

  @override
  String get mapKindRelay => 'relay';

  @override
  String get mapThisNode => 'this node';

  @override
  String get mapAsking => 'Asking it for power, neighbours and version…';

  @override
  String get mapNoAnswerSleepy =>
      'It did not answer: it sleeps between its windows.';

  @override
  String get mapNoAnswer => 'It did not answer.';

  @override
  String mapNodesHere(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count nodes here',
      one: '$count node here',
    );
    return '$_temp0';
  }

  @override
  String mapNodesHereNamed(int count, String names) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count nodes here: $names',
      one: '$count node here: $names',
    );
    return '$_temp0';
  }

  @override
  String mapNeighboursRead(int count, String time) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count neighbours, read $time',
      one: '$count neighbour, read $time',
    );
    return '$_temp0';
  }

  @override
  String mapBandGood(String good) {
    return 'good ≥ $good dB';
  }

  @override
  String mapBandFair(String fair, String good) {
    return 'fair $fair–$good dB';
  }

  @override
  String mapBandFragile(String fair) {
    return 'fragile < $fair dB over the noise floor';
  }

  @override
  String get mapLegendRelay => 'a relay';

  @override
  String get mapLegendSleepy => 'a sleepy node';

  @override
  String get mapLegendCarried => 'a carried node';

  @override
  String get mapLegendShared => 'nodes at one place';

  @override
  String get mapLegendYou =>
      'you: your position stays on this phone, never sent or kept';

  @override
  String mapHomeOf(String names) {
    return 'Home: $names';
  }

  @override
  String get mapYou => 'You';

  @override
  String mapLastSeen(String names) {
    return 'Last seen: $names';
  }

  @override
  String mapSeenAgo(String name, String ago) {
    return '$name $ago';
  }

  @override
  String get ok => 'OK';

  @override
  String get debug => 'Debug';

  @override
  String get debugMe => 'Me';

  @override
  String get debugBench => 'Bench';

  @override
  String get debugStatus => 'Status';

  @override
  String get debugGivenBackHeading => 'Given back';

  @override
  String get debugConnectPaired =>
      'Connect a paired node to pick a firmware file.';

  @override
  String get debugPickFirmware => 'Pick a firmware file';

  @override
  String get debugValue => 'Value';

  @override
  String get debugEmpty => 'empty';

  @override
  String debugGivenBack(String id, String why, int wait) {
    return '$id, $why, wait $wait s';
  }

  @override
  String get debugSent => 'Sent';

  @override
  String get debugReceived => 'Received';

  @override
  String get debugLog => 'Log';

  @override
  String get debugNone => 'None';

  @override
  String get debugNothingYet => 'Nothing yet';

  @override
  String get debugIdentity => 'Identity';

  @override
  String get debugCardAddress => 'Card address';

  @override
  String get debugNoneNoHome => 'none: no home';

  @override
  String get debugNoValue => 'none';

  @override
  String get debugUnsentAtHome => 'Mailbox: unsent at home';

  @override
  String get debugFetchRounds => 'Fetch rounds';

  @override
  String get debugLastSent => 'Last sent';

  @override
  String get debugAppVersion => 'App version';

  @override
  String get debugDevBuild => 'development build';

  @override
  String get debugLatestRelease => 'Latest release';

  @override
  String get debugNotKnown => 'not known';

  @override
  String debugRelease(int release, int app, int firmware, String checked) {
    return '$release, app $app, firmware $firmware, checked $checked';
  }

  @override
  String get debugName => 'Name';

  @override
  String get debugNodeId => 'Node ID';

  @override
  String get debugLink => 'Link';

  @override
  String get debugLinkGuest => 'a guest\'s';

  @override
  String get debugLinkPaired => 'the paired phone\'s';

  @override
  String get debugLinkVersion => 'Link version';

  @override
  String get debugFirmwareImage => 'Firmware image';

  @override
  String get debugClock => 'Clock';

  @override
  String get debugPublic => 'Public';

  @override
  String get debugYes => 'yes';

  @override
  String get debugNo => 'no';

  @override
  String get debugPublicGuest => 'yes, as a guest finds it';

  @override
  String get debugHome => 'Home';

  @override
  String get debugAddress => 'Address';

  @override
  String get debugNoPlace => 'none yet: no place';

  @override
  String get debugAddressCopied => 'Address copied';

  @override
  String get debugStatusNotRead => 'Not read';

  @override
  String get debugReady => 'Ready to take a message';

  @override
  String get debugExternalPower => 'External power';

  @override
  String get debugSleepy => 'Sleepy';

  @override
  String get debugCarried => 'Carried';

  @override
  String get debugMapPoint => 'Position from a map point';

  @override
  String get debugPlaced => 'Placed';

  @override
  String get debugAnchorless => 'Anchorless';

  @override
  String get debugFirstRotation => 'First rotation';

  @override
  String debugRotationRunning(String left) {
    return 'running, $left to go';
  }

  @override
  String get debugRotationOver => 'over';

  @override
  String get debugRelaysKnown => 'Relays known';

  @override
  String get debugNeighboursHeard => 'Neighbours heard';

  @override
  String get debugAddressPosition => 'Address position';

  @override
  String get debugBattery => 'Battery';

  @override
  String get debugBatteryNone => 'not read or on USB';

  @override
  String get debugInbox => 'Messages in the inbox';

  @override
  String dbHz(int signal) {
    return '$signal dB-Hz';
  }

  @override
  String get pairAgain => 'Pair again';

  @override
  String get staysYourHome => 'It stays your home.';

  @override
  String get forgetInBluetoothToo =>
      'Forget the node in Bluetooth settings too: the phone kept its pairing.';

  @override
  String get settingsBackupAnnounce => 'Backup: write down your 12 words';

  @override
  String get settingsPlaceHere => 'Use this phone\'s location';

  @override
  String settingsPlaceSet(String place) {
    return 'Place set to $place.';
  }

  @override
  String get settingsDegreesPlease =>
      'Latitude and longitude in degrees, please';

  @override
  String get settingsNameHow => 'On your card, for others to see';

  @override
  String get settingsDebugHow => 'What the link says, for a bug report';

  @override
  String get settingsThisNode => 'This node';

  @override
  String get settingsPublic => 'Public';

  @override
  String get settingsPublicHow => 'Any phone near it can text through it';

  @override
  String settingsPublicFailed(String error) {
    return 'Setting Public failed: $error';
  }

  @override
  String get settingsUnpair => 'Unpair';

  @override
  String settingsUnpairTitle(String node) {
    return 'Unpair $node?';
  }

  @override
  String get settingsUnpairHow =>
      'The node and this phone forget each other; texts and contacts stay.';

  @override
  String get settingsUnpairLast => 'With no phone left, it turns public.';

  @override
  String get settingsUnpairOthers => 'It stays paired with your other nodes.';

  @override
  String get settingsUnpaired => 'Unpaired.';

  @override
  String get settingsUnpairedPublic => 'Unpaired. The node is public.';

  @override
  String settingsUnpairFailed(String error) {
    return 'Unpairing failed: $error';
  }

  @override
  String get settingsPhonesPaired => 'Phones paired with this node';

  @override
  String get settingsPhonesHelp =>
      'A node keeps up to eight phones. Anyone at it can pair, by the code on its screen, and every paired phone can change its settings, so remove a phone you do not know or one that is gone. A removal also takes the code on the node\'s screen, to show you are there.\n\nA phone with a home here has its mail kept at this node.\n\nThe owner, one of them, chooses who has a home here, also from far away: they can give a contact a home here, remove one, or make someone else the owner. The first to make the node their home is its owner. At the node, any paired phone can make another the owner by the code on the screen.';

  @override
  String get settingsPhonesFull => 'Full: remove one before another can pair.';

  @override
  String get settingsPhonesHome => 'Phones with a home here';

  @override
  String get settingsNoOwner => 'This node has no owner';

  @override
  String get settingsBecomeOwner => 'Become owner';

  @override
  String get settingsBattery => 'Battery';

  @override
  String get settingsBatteryExempt => 'bark may run with the screen off.';

  @override
  String get settingsBatteryNotExempt =>
      'With the screen off, Android may close bark\'s link after some hours.';

  @override
  String get settingsLetRun => 'Let bark run';

  @override
  String get settingsBackup => 'Backup';

  @override
  String get settingsBackedUp =>
      'Your 12 words bring your identity back on another phone.';

  @override
  String get settingsNotBackedUp =>
      'Write down your 12 words: a lost phone without them loses your identity.';

  @override
  String get settingsShowWords => 'Show my words';

  @override
  String get settingsRestore => 'Restore from words';

  @override
  String get settingsMyWords => 'My 12 words';

  @override
  String get settingsWroteThem => 'I wrote them down';

  @override
  String get settingsRestoreTitle => 'Restore from words?';

  @override
  String get settingsRestoreHow =>
      'This phone takes the identity of the words. Contacts reach it only once they have its card.';

  @override
  String get restore => 'Restore';

  @override
  String get settingsYourWords => 'Your 12 words';

  @override
  String get settingsNotWords => 'Those are not 12 words of a backup';

  @override
  String get restoredIdentity => 'Identity restored';

  @override
  String get settingsPlace => 'Place';

  @override
  String get settingsNoPlace => 'The node has no place yet.';

  @override
  String settingsPlaceByPerson(String address) {
    return 'Set by a person: $address.';
  }

  @override
  String settingsPlaceByGps(String address) {
    return 'From its GPS: $address.';
  }

  @override
  String get settingsPlaceHow =>
      'For a node indoors, out of GPS reach. It wins over GPS until cleared.';

  @override
  String get settingsLatitude => 'Latitude';

  @override
  String get settingsLongitude => 'Longitude';

  @override
  String get settingsPlaceSetButton => 'Set';

  @override
  String get settingsPlaceClear => 'Clear';

  @override
  String get settingsNotifications => 'Notifications';

  @override
  String get settingsNotificationsWhy =>
      'bark tells you when a text comes. Android asks next.';

  @override
  String settingsPairingFailed(String error) {
    return 'Pairing failed: $error';
  }

  @override
  String settingsConnectingFailed(String error) {
    return 'Connecting failed: $error';
  }

  @override
  String get settingsNotPaired => 'Not paired';

  @override
  String get settingsNotOurs => 'This node is no longer yours';

  @override
  String settingsNotOursHow(String node) {
    return '$node no longer knows this phone. Pair again with the code on its screen.';
  }

  @override
  String get nodeConnecting => 'Connecting…';

  @override
  String get nodeTypeCode => 'Type the code on the node\'s screen';

  @override
  String get nodePairedConnecting => 'Paired, connecting…';

  @override
  String get node => 'Node';

  @override
  String get nodeDisconnect => 'Disconnect';

  @override
  String get nodeStopLooking => 'Stop looking';

  @override
  String get nodeScan => 'Scan';

  @override
  String get nodeStopScan => 'Stop';

  @override
  String nodeScanFailed(String error) {
    return 'Scan failed: $error';
  }

  @override
  String get nodeNoneFound => 'No node found nearby.';

  @override
  String get nodeNoneFoundHow => 'Switch one on, or come closer.';

  @override
  String get nodePairedWith => 'Paired with';

  @override
  String get nodeForget => 'Forget';

  @override
  String nodeLostTitle(String node) {
    return '$node does not know this phone';
  }

  @override
  String get nodeLostHelp =>
      'Someone paired with it removed this phone, the node lost its memory, or you moved here from another phone: a pairing stays on the phone it was made on.\n\nPair again: pair anew with the code on its screen. Shown while the node is in reach.\n\nForget: stop listing the node here.';

  @override
  String get nodeConnected => 'Connected';

  @override
  String get nodeHomeConnected => 'Your home, connected';

  @override
  String get nodeLost => 'Does not know this phone';

  @override
  String get nodeHomeLost => 'Your home, does not know this phone';

  @override
  String get nodeUnserved => 'Cannot be used yet. Tap to try again';

  @override
  String get nodeHomeUnserved =>
      'Your home, cannot be used yet. Tap to try again';

  @override
  String get nodeAway => 'Not in reach';

  @override
  String get nodeHomeAway => 'Your home, not in reach';

  @override
  String get nodeNotLooking => 'Not looking. Tap to connect';

  @override
  String get nodeHomeNotLooking => 'Your home, not looking. Tap to connect';

  @override
  String get nodeYourHome => 'Your home';

  @override
  String get nodeFoundPublic => 'public';

  @override
  String get nodeFoundUnserved => 'cannot be used yet';

  @override
  String get nodePair => 'Pair';

  @override
  String nodePairTitle(String node) {
    return 'Pair with $node?';
  }

  @override
  String get nodePairHow =>
      'You can then change its settings and update it. Type the code on its screen.';

  @override
  String nodeForgetTitle(String node) {
    return 'Forget $node?';
  }

  @override
  String get nodeForgetHow => 'This phone stops listing it.';

  @override
  String get nodeForgotten => 'Forgotten.';

  @override
  String get nodeForgetLater =>
      'The phone drops its pairing when it next hears the node, or forget it in Bluetooth settings now.';

  @override
  String nodeForgetFailed(String error) {
    return 'Forgetting failed: $error';
  }

  @override
  String get peopleMakeOwner => 'Make owner';

  @override
  String get peopleThisPhone => 'this phone';

  @override
  String get peopleYou => 'you';

  @override
  String get peopleOwner => 'owner';

  @override
  String get peoplePairedToo => 'paired too';

  @override
  String get peopleHomeToo => 'has a home here too';

  @override
  String get peopleLinked => 'linked now';

  @override
  String peopleSeen(String when) {
    return 'seen $when';
  }

  @override
  String get peopleNeverSeen => 'seen never';

  @override
  String get peopleChoices => 'Choices';

  @override
  String get peopleNoName => 'A phone with no name';

  @override
  String peopleRemovePhoneAndHome(String name) {
    return 'Remove $name\'s phone and its home?';
  }

  @override
  String get peopleRemoveUnnamedAndHome =>
      'Remove the phone with no name and its home?';

  @override
  String peopleRemovePhone(String name) {
    return 'Remove $name\'s phone?';
  }

  @override
  String get peopleRemoveUnnamed => 'Remove the phone with no name?';

  @override
  String get peopleRemoveThisHome => 'Remove this home?';

  @override
  String peoplePhoneGoes(String name) {
    return '$name\'s phone can no longer use the node unless paired again.';
  }

  @override
  String get peopleUnnamedGoes =>
      'The phone with no name can no longer use the node unless paired again.';

  @override
  String get peopleNodeStopsMail => 'The node stops keeping their mail.';

  @override
  String peopleRemoveHome(String name) {
    return 'Remove $name\'s home?';
  }

  @override
  String get peopleRemoveTheirHome => 'Remove this person\'s home?';

  @override
  String get peopleHomeStopsMail => 'Your home stops keeping their mail.';

  @override
  String peopleMakeOwnerTitle(String name) {
    return 'Make $name the owner?';
  }

  @override
  String get peopleMakeThemOwner => 'Make this person the owner?';

  @override
  String get peopleOwnerChooses => 'The owner chooses who has a home here.';

  @override
  String get peopleOwnerChoosesNotYou =>
      'The owner chooses who has a home here. You no longer can.';

  @override
  String get peopleTypeCodeNext => 'You type the code the node then shows.';

  @override
  String get peopleShowCode => 'Show the code';

  @override
  String get peopleGone => 'Already gone.';

  @override
  String peopleCodeFailed(String error) {
    return 'Asking the node for a code failed: $error';
  }

  @override
  String get peopleRemoved => 'Removed';

  @override
  String peopleIsOwner(String name) {
    return '$name is the owner';
  }

  @override
  String get peopleThemOwner => 'This person is the owner';

  @override
  String get peopleNotChanged => 'Not changed';

  @override
  String get peopleNotRemoved => 'Not removed';

  @override
  String get peopleWrongCode =>
      'Wrong code, or too late. Each code works once.';

  @override
  String get peopleNewCode => 'New code';

  @override
  String peopleItFailed(String error) {
    return 'It failed: $error';
  }

  @override
  String get peopleCodeTitle => 'The code on the node';

  @override
  String peopleCodeHow(String word) {
    return 'Type the six digits under $word on the node\'s screen, within 30 s.';
  }

  @override
  String get peopleCode => 'Code';

  @override
  String linkNeedsFirmware(String node) {
    return '$node needs newer firmware. Update the node.';
  }

  @override
  String linkNeedsApp(String node) {
    return '$node needs a newer app. Update the app.';
  }

  @override
  String linkMalformed(String what) {
    return 'malformed $what';
  }

  @override
  String get linkRefusedHeader => 'the file is not firmware this node reads';

  @override
  String get linkRefusedVersion =>
      'the firmware is not newer than the version the node runs';

  @override
  String get linkRefusedBusy =>
      'the node holds checked new firmware already: reboot it first';

  @override
  String get linkRefusedIncomplete =>
      'the node does not hold every byte of the firmware';

  @override
  String get linkRefusedHash =>
      'the bytes the node holds are not the firmware\'s; send it again';

  @override
  String get linkRefusedBattery =>
      'the node is on a low battery; it keeps the firmware, to go on once it is on USB or charged';

  @override
  String get linkRefusedFlash => 'the node\'s flash memory failed';

  @override
  String linkRefusedSignature(String command) {
    return 'the firmware is not signed by a key the node holds: a file built here is signed with `$command`, a release\'s is signed already';
  }

  @override
  String linkAttError(String code) {
    return 'ATT error 0x$code';
  }

  @override
  String linkNotOurs(String node) {
    return '$node no longer knows this phone';
  }

  @override
  String linkBondKept(String node) {
    return 'The phone kept its old pairing with $node, so it cannot use the node. Forget it in Bluetooth settings.';
  }

  @override
  String linkEnded(String node) {
    return 'The link to $node ended. Try again nearer to it';
  }

  @override
  String linkNotPaired(String full) {
    return 'Not paired: cancelled, a wrong code, or the node is full (its screen says $full)';
  }

  @override
  String linkNotSeen(String node) {
    return '$node not found: come nearer and try again';
  }

  @override
  String get linkImageTooShort => 'malformed image: too short';

  @override
  String get linkImageNotOne => 'malformed image: not one';

  @override
  String get linkImageFormat => 'malformed image: format';

  @override
  String get linkImageLength => 'malformed image: length';

  @override
  String get linkImageHash => 'malformed image: hash';

  @override
  String get fullNode => 'This node is full: eight phones are paired with it.';

  @override
  String get homeNone => 'None: your card is temporary.';

  @override
  String homeAt(String address) {
    return 'At $address.';
  }

  @override
  String get homeNoPlace =>
      'This node has no place yet, so it cannot be a home.';

  @override
  String get homeNoPlaceSetIt =>
      'This node has no place yet, so it cannot be a home. Set it under This node.';

  @override
  String get homeNeedsPower => 'A home must stay on power in one place.';

  @override
  String get homeKeyGone =>
      'Your home no longer keeps your mail. Make it your home again, or choose another.';

  @override
  String get homeSilent => 'Your home did not answer your last fetch.';

  @override
  String get homeHoldsOtherKey =>
      'This node keeps mail for this phone, and is not your home.';

  @override
  String get homeOthers => 'This node is someone else\'s home.';

  @override
  String get homeRemove => 'Remove home';

  @override
  String get homeForgetMe => 'Forget me here';

  @override
  String get homeUseExisting => 'Use my existing home';

  @override
  String homeNoAnswer(String node) {
    return '$node did not answer. Nothing changed.';
  }

  @override
  String homeNoWay(String node) {
    return 'No way found to $node. Nothing changed.';
  }

  @override
  String get mailboxFetchNoWay => 'No way found to your home';

  @override
  String homeAsking(String node) {
    return 'Asking $node for your mail…';
  }

  @override
  String homeLeaving(String node) {
    return 'Leaving $node: it sends you its mail, then forgets you.';
  }

  @override
  String get homePeople => 'People with a home here';

  @override
  String get homeYouOwner => 'You are its owner.';

  @override
  String get homeShowThem => 'Show them';

  @override
  String get askAgain => 'Ask again';

  @override
  String homeAskFailed(String error) {
    return 'Asking failed: $error';
  }

  @override
  String homeSetFailed(String error) {
    return 'Setting the home failed: $error';
  }

  @override
  String get homeRemoveTitle => 'Remove home?';

  @override
  String get homeSetTitle => 'Set this node as home?';

  @override
  String get homePairFirst => 'First you pair: type the code on its screen.';

  @override
  String get homeCardWaits =>
      'Your card stays; its last change still waits for Send my card.';

  @override
  String get homeCardSame => 'Your card stays the same.';

  @override
  String get homeCardChanges => 'Your card changes.';

  @override
  String homeCardGoes(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Your new card goes to your $count contacts.',
      one: 'Your new card goes to your contact.',
    );
    return '$_temp0';
  }

  @override
  String get homeSet => 'Set';

  @override
  String get homeHelp =>
      'Your home is a node that stays on power in one place. It keeps texts to you while your phone is away, and your card carries its address, so the card stays good wherever you go. The inbox button on the first screen, Fetch my mail, brings them to the node you are on.\n\nWithout a home your card is temporary: it names the node you use now, which keeps your mail only while you come back to it.\n\nTo make a node your home, the phone pairs with it. A friend who owns a home node can also give you a home there from afar.\n\nWhen you choose another home, or none, your old home sends you the mail it keeps, then forgets you.\n\nA home that stops answering may be off or out of reach, or may have lost your place there. Fetch again later; if it still does not answer, make another node your home, or choose Remove home.\n\nUse my existing home is for a phone that got your identity back from your 12 words: give your home\'s address, and once your home answers it is your home again.';

  @override
  String get homePeopleHelp =>
      'As the owner you choose who has a home here, from anywhere: give a contact a home here from their conversation\'s menu, remove a person\'s home, or make someone else the owner.\n\nEach goes to your home with a fetch of your mail, and its answer comes back with it.';

  @override
  String get homeAddressTitle => 'Your home\'s address';

  @override
  String get homeAddressHow =>
      'Its 26 hex digits, or a card of yours that names it.';

  @override
  String get homeAddressLabel => 'Address or card';

  @override
  String get homeCardNoHome => 'That card names no home';

  @override
  String get homeNotAddress => 'Not an address or a card';

  @override
  String get homeScanCard => 'Scan a card';

  @override
  String get homeAskIt => 'Ask it';

  @override
  String get mailboxNoHomeToFetch => 'No home node to fetch from';

  @override
  String get mailboxFetchQueued => 'The fetch waits for a node';

  @override
  String get mailboxAtHome => 'At your home, your mail comes by itself';

  @override
  String get mailboxStillWaiting => 'Still waiting for the last fetch';

  @override
  String get mailboxNoPlace =>
      'This node has no place yet, so mail cannot come back to it';

  @override
  String get mailboxFetching => 'Fetching your mail';

  @override
  String get mailboxNotConnected => 'Not connected to a node';

  @override
  String get mailboxRefused => 'The node refused. Nothing changed.';

  @override
  String get mailboxIsHome => 'This node is your home';

  @override
  String get mailboxIsHomeLeft =>
      'This node is your home. Your old home sends you its mail, then forgets you.';

  @override
  String get mailboxNoHome => 'No home';

  @override
  String get mailboxRefusedStillHome =>
      'The node refused. It is still your home.';

  @override
  String get mailboxHomeRemoved => 'Home removed';

  @override
  String get mailboxHomeRemovedLeft =>
      'Home removed. Your old home sends you its mail, then forgets you.';

  @override
  String get mailboxNoGift => 'No home to use';

  @override
  String mailboxNodeIsHome(String node) {
    return '$node is your home';
  }

  @override
  String mailboxNodeIsHomeLeft(String node) {
    return '$node is your home. Your old home sends you its mail, then forgets you.';
  }

  @override
  String get mailboxHasHome => 'You have a home already';

  @override
  String get mailboxConnectFirst => 'Connect to a node first';

  @override
  String mailboxAsking(String node) {
    return 'Asking $node for your mail';
  }

  @override
  String mailboxKeepsNot(String node) {
    return '$node does not keep your mail. Nothing changed.';
  }

  @override
  String get mailboxHomeGone =>
      'Your home no longer keeps your mail. Choose a new home in Settings.';

  @override
  String get mailboxRefusedStillKeeps =>
      'The node refused. It still keeps your mail.';

  @override
  String get mailboxForgotYou => 'It forgot you';

  @override
  String get mailboxNotPaired => 'Not paired, so not your home';

  @override
  String get mailboxNeedsPower =>
      'Paired, but a home must stay on power in one place';

  @override
  String get mailboxNoHomeNode => 'No home node';

  @override
  String get mailboxSentHome =>
      'Sent to your home; its answer comes with your mail';

  @override
  String get mailboxNotAtHome => 'Not at your home';

  @override
  String get mailboxYouOwner => 'You are its owner';

  @override
  String get mailboxNotTaken => 'The node did not take it';

  @override
  String get mailboxOnlyOwner => 'Only your home\'s owner can share it';

  @override
  String get mailboxNotOwner => 'Your home says you are not its owner';

  @override
  String mailboxShared(String name) {
    return '$name has a home at yours now. They are told';
  }

  @override
  String get mailboxHomeFull => 'Your home is full: it keeps eight people';

  @override
  String get mailboxRemoved => 'Removed from your home';

  @override
  String get mailboxNewOwner => 'Your home has a new owner';

  @override
  String mailboxIsOwner(String name) {
    return '$name is your home\'s owner now';
  }

  @override
  String get mailboxGone => 'Already gone';

  @override
  String get placeDenied => 'Location permission denied';

  @override
  String placeNone(String error) {
    return 'No location: $error';
  }

  @override
  String version(int version) {
    return 'Version $version';
  }

  @override
  String get sendToNode => 'Send to the node';

  @override
  String installInstalling(String node, int version) {
    return '$node is installing version $version, back in about a minute';
  }

  @override
  String installInstallingLate(String node, int version) {
    return '$node is installing version $version, not back yet';
  }

  @override
  String installSentHelp(String node, int version) {
    return '$node restarts into version $version, which this phone sent it, for about a minute. The phone waits for it and reconnects by itself.';
  }

  @override
  String installMeshHelp(String node, int version) {
    return '$node got version $version from the nodes around it. It passes on the texts it holds, at most ten minutes, then restarts for about a minute. The phone reconnects by itself; nothing is lost.';
  }

  @override
  String get updateTitle => 'Update firmware';

  @override
  String get updateNotConnected => 'Not connected to a node.';

  @override
  String updateRuns(String node, int version) {
    return '$node runs version $version.';
  }

  @override
  String updateRunsSince(String node, int version, int from, String ago) {
    return '$node runs version $version, updated from version $from $ago.';
  }

  @override
  String get updatesInstall => 'Install';

  @override
  String get debugCopyLog => 'Copy the log';

  @override
  String get debugLogCopied => 'Log copied';

  @override
  String get updateLatest => 'Download latest release';

  @override
  String get updatePickFile => 'Pick a file';

  @override
  String get updatePickedFile => 'a picked file';

  @override
  String get updateNoFile => 'no file chosen';

  @override
  String get updateDownloadFailed =>
      'could not download the file: check the internet connection';

  @override
  String updateNoFirmware(String error) {
    return 'No firmware: $error';
  }

  @override
  String get updateHearsNoOne => 'The node hears no one';

  @override
  String updateHearsNoOneWarning(int version, String file) {
    return 'Since version $version it hears no other node: the new firmware may have broken its radio. To go back, double-tap its reset button and copy $file from that release to it over USB; the phone cannot send an older version.';
  }

  @override
  String get updateNoRelease => 'No release known yet.';

  @override
  String updateReleaseNoFirmware(int release) {
    return 'Release $release carries no node firmware.';
  }

  @override
  String updateRunsLatest(int version) {
    return 'The node already runs the latest release\'s firmware, version $version.';
  }

  @override
  String updateRunsNewer(int version) {
    return 'The node runs a newer version than the latest release\'s firmware, $version.';
  }

  @override
  String updateImage(String source, int kib) {
    return '$source, $kib KiB';
  }

  @override
  String updateImageNotNewer(String source, int kib) {
    return '$source, $kib KiB: not newer than the node\'s';
  }

  @override
  String updateOnNode(int held, int total) {
    return '$held of $total KiB on the node';
  }

  @override
  String get updateSending => 'Sending…';

  @override
  String get updateChecking => 'The node checks the firmware…';

  @override
  String get updateGoOn => 'Go on';

  @override
  String get updateTryAgain => 'Try again';

  @override
  String updateReboot(int version) {
    return 'Reboot into version $version';
  }

  @override
  String get updateStalled => 'The node takes no pieces';

  @override
  String get updatePaused =>
      'The link dropped; it goes on when the node is back';

  @override
  String updateHolds(int version) {
    return 'The node holds version $version';
  }

  @override
  String get updateLost => 'The node lost the firmware';

  @override
  String updateChecked(int version) {
    return 'The node checked version $version';
  }

  @override
  String updateCommitFailed(String error) {
    return 'Committing failed: $error';
  }

  @override
  String updateAnswered(String error) {
    return 'The node answered $error';
  }

  @override
  String updateRefused(String reason) {
    return 'The node refused: $reason';
  }

  @override
  String get updateRefusedNoReason => 'The node refused: it did not say why';

  @override
  String get updateRestarts => 'The node restarts, about a minute';

  @override
  String get updateNotBack => 'The node is not back; it may be out of reach';

  @override
  String updateRebootFailed(String error) {
    return 'Reboot failed: $error';
  }

  @override
  String updateOtherNode(String node) {
    return 'connected to another node than $node';
  }

  @override
  String get updatePairedOnly =>
      'only the phone paired with the node updates it';

  @override
  String updateDidNotStart(int version, int running) {
    return 'Version $version did not start; the node is back on $running';
  }

  @override
  String get updatesHeading => 'Updates';

  @override
  String get updatesCheck => 'Check for updates';

  @override
  String get updatesUnreached => 'Could not reach the release list';

  @override
  String get updatesApp => 'App';

  @override
  String get updatesDevBuild => 'A development build';

  @override
  String updatesAppOut(int version, int latest) {
    return 'Version $version; $latest is out';
  }

  @override
  String updatesAppDownloading(int version, int latest) {
    return 'Version $version; downloading $latest…';
  }

  @override
  String updatesAppDownloadingPercent(int version, int latest, int percent) {
    return 'Version $version; downloading $latest… $percent%';
  }

  @override
  String updatesAppReady(int version, int latest) {
    return 'Version $version; $latest is downloaded';
  }

  @override
  String updatesAppFailed(int version, int latest) {
    return 'Version $version; $latest did not download';
  }

  @override
  String get updatesNodeNewer => 'Update it for the node\'s new features';

  @override
  String get updatesNodeFirmware => 'Node firmware';

  @override
  String get nodeNotConnected => 'Not connected';

  @override
  String get updatesGuest => 'Only its paired phones update it';

  @override
  String updatesFirmwareOut(int version, int latest) {
    return 'Version $version; $latest is out';
  }

  @override
  String releaseLine(String app, String node) {
    return '$app; $node';
  }

  @override
  String get releaseAppDev => 'App is a development build';

  @override
  String releaseAppOut(int version) {
    return 'App $version is out: install it below';
  }

  @override
  String releaseAppUpToDate(int version) {
    return 'App up to date ($version)';
  }

  @override
  String get releaseNoNode => 'no node linked';

  @override
  String get releaseNotYours => 'the node is not yours to update';

  @override
  String get releaseBench => 'node firmware is a bench build';

  @override
  String releaseNoneCarried(int version) {
    return 'node firmware $version; the release carries none';
  }

  @override
  String releaseFirmwareOut(int version) {
    return 'node firmware $version is out: send it below';
  }

  @override
  String releaseFirmwareUpToDate(int version) {
    return 'node firmware $version, up to date';
  }

  @override
  String get identityHelp =>
      'The file holds your identity, name, home, contacts and the nodes you paired with, and your messages if you tick the box. Your password locks it: without it nobody can open the file, not even you.\n\nOn the new phone, tap Restore from a file, then pair with your nodes again: a pairing stays on the phone it was made on. Your home keeps your mail meanwhile, and you can fetch it from any other node.\n\nUse one phone at a time: two phones with one identity would each take mail the other never sees.\n\nYour 12 words bring back your identity alone.';

  @override
  String get identitySaveHow => 'Save it to a file to move to a new phone.';

  @override
  String get identitySave => 'Save to a file';

  @override
  String get identityRestore => 'Restore from a file';

  @override
  String get identitySaveTitle => 'Save my identity';

  @override
  String get identitySaved => 'Saved';

  @override
  String get identityNotSaved => 'Not saved';

  @override
  String identitySaveFailed(String error) {
    return 'Saving failed: $error';
  }

  @override
  String identityNoStream(String uri) {
    return 'no stream for $uri';
  }

  @override
  String get identitySaveBusy => 'a save is under way';

  @override
  String get identityRestoreTitle => 'Restore from a file?';

  @override
  String get identityRestoreHow =>
      'This replaces the identity, contacts and messages on this phone.';

  @override
  String get identityChooseFile => 'Choose file';

  @override
  String identityOpenFailed(String error) {
    return 'Opening failed: $error';
  }

  @override
  String get identityPasswordTitle => 'The file\'s password';

  @override
  String get identityRestoredPairAgain =>
      'Identity restored. Pair again with your nodes.';

  @override
  String identityRefused(String reason) {
    return '$reason Nothing changed.';
  }

  @override
  String get identityNotFile => 'This is not a bark identity file.';

  @override
  String get identityOtherLayout =>
      'This file is from another version of the app.';

  @override
  String get identityWrongPassword => 'Wrong password, or the file is damaged.';

  @override
  String get identityPassword => 'Password';

  @override
  String get identityPasswordAgain => 'Password again';

  @override
  String get identityNotSame => 'Not the same';

  @override
  String get identityAddMessages => 'Add my messages';

  @override
  String get identityOnePhone => 'Use it on one phone at a time.';
}
