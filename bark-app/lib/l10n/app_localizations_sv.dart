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
      one: '$count arkiverat',
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

  @override
  String get cancel => 'Avbryt';

  @override
  String get save => 'Spara';

  @override
  String get notNow => 'Inte nu';

  @override
  String get timeJustNow => 'nyss';

  @override
  String timeMinutesAgo(int minutes) {
    return 'för $minutes min sedan';
  }

  @override
  String timeHoursAgo(int hours) {
    return 'för $hours tim sedan';
  }

  @override
  String timeDaysAgo(int days) {
    String _temp0 = intl.Intl.pluralLogic(
      days,
      locale: localeName,
      other: 'för $days dagar sedan',
      one: 'för $days dag sedan',
    );
    return '$_temp0';
  }

  @override
  String get timeNever => 'aldrig';

  @override
  String get timeToday => 'i dag';

  @override
  String get timeYesterday => 'i går';

  @override
  String get sentQueued => 'väntar på en nod';

  @override
  String get sentWriting => 'lämnas till noden';

  @override
  String get sentTaken => 'skickat ut i nätet';

  @override
  String get sentBusyNow => 'nätet är upptaget, skickar igen nu';

  @override
  String sentBusyIn(String wait) {
    return 'nätet är upptaget, skickar igen om $wait';
  }

  @override
  String get sentRefused => 'avvisat av noden';

  @override
  String sentPartsSent(int taken, int total) {
    return '$taken av $total skickade';
  }

  @override
  String sentPartProblem(int part, String state) {
    return 'del $part: $state';
  }

  @override
  String get sentQueuedHelp =>
      'På telefonen, tills en nod är ansluten och redo för det.';

  @override
  String get sentWritingHelp => 'Telefonen lämnar det till noden.';

  @override
  String get sentTakenHelp =>
      'Noden har det och skickar det vidare. Nätet säger inget mer, inte heller när det kommer fram.';

  @override
  String get sentBusy => 'Nätet är upptaget';

  @override
  String get sentBusyHelp =>
      'Nätet gav tillbaka det tills vidare. Appen skickar det igen av sig själv när väntan är över.';

  @override
  String get sentRefusedHelp =>
      'Noden tar inte emot det som det är, ett fel i appen eller noden. Avbryt tar bort det; Felsökning visar orsaken.';

  @override
  String get sentReaction => 'Deras reaktion';

  @override
  String get sentReactionHelp => 'Emojin i meddelandets hörn, om de valde en.';

  @override
  String get receivedYourReaction => 'Din reaktion';

  @override
  String get receivedYourReactionHelp =>
      'Håll in ett meddelande för att reagera med en emoji eller kopiera det. De ser den när den kommer fram. Välj en annan för att byta.';

  @override
  String get receivedFirst => 'Första meddelandet';

  @override
  String get receivedFirstHelp =>
      'Vem som helst med ditt kort kan ha skickat ett första meddelande, så det är inte bevisat att det här kommer från dem.';

  @override
  String get receivedMailbox => 'Från din brevlåda';

  @override
  String get receivedMailboxHelp =>
      'Det väntade på din hemnod och kom när du hämtade din post.';

  @override
  String get receivedParts => 'Delar';

  @override
  String get receivedPartsHelp =>
      'Ett långt meddelande kommer i delar, i vilken ordning som helst. Kursiv stil står för en del som inte kommit än.';

  @override
  String get receivedTime => 'Tiden';

  @override
  String get receivedTimeHelp =>
      'När det kom. När det skrevs står under Det här meddelandet.';

  @override
  String get commonThisMessage => 'Det här meddelandet';

  @override
  String commonNotPaired(String reason) {
    return '$reason. Fortfarande gäst här.';
  }

  @override
  String commonPairingFailed(String error) {
    return 'Parkopplingen misslyckades: $error. Fortfarande gäst här.';
  }

  @override
  String commonGift(String name, String node) {
    return '$name gav dig ett hem på $node';
  }

  @override
  String get commonGiftTitle => 'Ett hem i gåva';

  @override
  String get commonGiftHelp =>
      'Ägaren till en hemnod har lagt till dig där. Använder du det blir det ditt hem: det sparar meddelanden till dig medan du är borta, och ditt kort får dess adress, så dina kontakter får ditt nya kort.\n\nEtt hem du hade förut skickar dig din post och glömmer sedan bort dig.';

  @override
  String get commonUseGift => 'Använd det';

  @override
  String commonFailed(String error) {
    return 'Misslyckades: $error';
  }

  @override
  String get commonHomeOffer =>
      'Ingen hemnod än. Göra den här till ditt hem? Den sparar meddelanden till dig medan du är borta. Först parkopplar du med koden på dess skärm.';

  @override
  String get commonSetHome => 'Gör till hem';

  @override
  String get commonBluetoothSettings => 'Bluetooth-inställningar';

  @override
  String commonBluetoothSettingsFailed(String error) {
    return 'Det gick inte att öppna Bluetooth-inställningarna: $error';
  }

  @override
  String get commonFetchQueued => 'Hämtningen väntar på en nod';

  @override
  String commonFetchWaiting(String wait) {
    return 'Väntar på svar från din hemnod, högst $wait';
  }

  @override
  String get commonFetch => 'Hämta min post';

  @override
  String commonFetchFailed(String error) {
    return 'Hämtningen misslyckades: $error';
  }

  @override
  String get commonCardNoHome => 'Ditt kort har ändrats: inget hem';

  @override
  String get commonCardNewHome => 'Ditt kort har ändrats: ny hemadress';

  @override
  String commonCardOld(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Dina $count kontakter har det gamla.',
      one: 'Din kontakt har det gamla.',
    );
    return '$_temp0';
  }

  @override
  String get commonCardQueued => 'Ditt kort står i kö';

  @override
  String commonCardFailed(String error) {
    return 'Det gick inte att skicka kortet: $error';
  }

  @override
  String get commonSendCard => 'Skicka mitt kort';

  @override
  String get commonMyName => 'Mitt namn';

  @override
  String commonAbout(String title) {
    return 'Om $title';
  }

  @override
  String get commonWhyThisNode => 'Varför den här noden';

  @override
  String get commonWhyBluetooth =>
      'bark skickar meddelanden via en nod nära dig, över Bluetooth. Android frågar härnäst.';

  @override
  String get commonContinue => 'Fortsätt';

  @override
  String get statusPublicNode => 'en publik nod';

  @override
  String statusLooking(String node) {
    return 'Inte ansluten. Letar efter $node; meddelanden väntar på telefonen';
  }

  @override
  String statusBluetoothOff(String node) {
    return 'Inte ansluten. Bluetooth är av: slå på det för att nå $node';
  }

  @override
  String get statusNotLooking =>
      'Inte ansluten. Meddelanden väntar på telefonen';

  @override
  String statusGuestFor30s(String node, String install) {
    return 'Ansluten till $node som gäst i 30 s$install';
  }

  @override
  String statusGuest(String node, String install) {
    return 'Ansluten till $node som gäst$install';
  }

  @override
  String statusAtHome(String node, String install) {
    return 'Ansluten till ditt hem, $node$install';
  }

  @override
  String statusPaired(String node, String install) {
    return 'Ansluten till $node, parkopplad$install';
  }

  @override
  String statusHomeAt(String node) {
    return 'Hem på $node';
  }

  @override
  String statusStarting(String wait) {
    return 'Startar, $wait';
  }

  @override
  String get statusHearsNone =>
      'Hör ingen annan nod än: meddelanden väntar här';

  @override
  String statusHears(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Hör $count noder',
      one: 'Hör 1 nod',
    );
    return '$_temp0';
  }

  @override
  String get signalUnknown => 'okänd signal';

  @override
  String get signalStrong => 'stark signal';

  @override
  String get signalFair => 'medelstark signal';

  @override
  String get signalWeak => 'svag signal';

  @override
  String connectionLostOne(String node) {
    return '$node känner inte igen den här telefonen: parkoppla med den igen i Inställningar.';
  }

  @override
  String connectionLostMany(String nodes) {
    return '$nodes känner inte igen den här telefonen: parkoppla med dem igen i Inställningar.';
  }

  @override
  String connectionUnservedOne(String node) {
    return '$node kan inte användas än: den eller appen behöver uppdateras. Tryck på den i Inställningar för att försöka igen.';
  }

  @override
  String connectionUnservedMany(String nodes) {
    return '$nodes kan inte användas än: de eller appen behöver uppdateras. Tryck på en av dem i Inställningar för att försöka igen.';
  }

  @override
  String get connectionAtHome =>
      'Ditt hem. Telefonen förblir ansluten medan den är inom räckhåll, så meddelanden kommer direkt, även med skärmen av. Den går alltid till ditt hem först.';

  @override
  String get connectionPaired =>
      'En nod du är parkopplad med. Telefonen förblir ansluten medan den är inom räckhåll.';

  @override
  String get connectionPairedNoHome =>
      'Utan hem anger ditt kort den här noden medan du använder den.';

  @override
  String get connectionPairedHome =>
      'Meddelanden till dig väntar i ditt hem; Hämta min post hämtar dem hit.';

  @override
  String get connectionMovesHome =>
      'När ditt hem är inom räckhåll byter telefonen till det inom en minut.';

  @override
  String get connectionTappedGuest =>
      'Någon annans publika nod, som du valde. Den tar emot en gäst i taget och avslutar en gästs anslutning efter 30 sekunder; sedan väljer telefonen en nod själv.';

  @override
  String get connectionGuestWeak =>
      'Dina parkopplade noder hörs för svagt här, så telefonen använder den här publika noden tills den hör någon av dem bra.';

  @override
  String get connectionGuestAway =>
      'Ingen av dina parkopplade noder har varit inom räckhåll på 30 sekunder, så telefonen använder den här publika noden tills den hör någon av dem.';

  @override
  String get connectionGuestNoneNear =>
      'Ingen av dina noder är nära, så telefonen använder den här publika noden, som är någon annans.';

  @override
  String get connectionGuestVisits =>
      'Som gäst ansluter den ungefär en gång i minuten, och direkt när du skickar. Svar på meddelanden som skickats härifrån kommer hit.';

  @override
  String get connectionBluetoothOff =>
      'Bluetooth är av. Meddelanden väntar på telefonen tills det är på och en nod är inom räckhåll.';

  @override
  String get connectionLookPublic =>
      'Ingen egen nod att använda, så telefonen letar var 15:e sekund efter en publik nod att använda som gäst.';

  @override
  String get connectionLookPaired =>
      'Telefonen letar var 15:e sekund efter dina parkopplade noder, ditt hem först. Efter 30 sekunder utan någon använder den en publik nod som gäst så länge.';

  @override
  String get connectionNotLooking =>
      'Telefonen letar inte: du stoppade sökningen, eller så får den inte använda Bluetooth än. Tryck på en nod i Inställningar för att ansluta.';

  @override
  String get notifyTextsArrive => 'Meddelanden kommer även med skärmen av';

  @override
  String get notifyChecksMinute =>
      'Söker efter meddelanden ungefär en gång i minuten';

  @override
  String get notifyOutInReach =>
      'Väntande meddelanden går iväg när den är inom räckhåll';

  @override
  String get notifyOutBluetooth =>
      'Väntande meddelanden går iväg när Bluetooth är på';

  @override
  String get notifyUnnamed => 'Namnlös';

  @override
  String get notifyChannel => 'Meddelanden';

  @override
  String get notifyChannelAbout => 'Ett meddelande från en kontakt';

  @override
  String get notifyYou => 'Du';

  @override
  String get threadRemovedContact => 'Borttagen kontakt';

  @override
  String threadPartMissing(int part, int total) {
    return 'del $part av $total har inte kommit än';
  }

  @override
  String get threadUnverifiedMark =>
      'Ett första meddelande, som vem som helst kan ha skickat';

  @override
  String get threadToNode => 'Till nod';

  @override
  String get threadAt => 'Plats';

  @override
  String get threadSealed => 'Krypterat';

  @override
  String get threadSealedForThem => 'bara för dem';

  @override
  String get threadSealedIntroOut => 'som ett första meddelande, med ditt kort';

  @override
  String get threadSealedForYou => 'av dem, bara för dig';

  @override
  String get threadSealedIntroIn => 'för dig, som ett första meddelande';

  @override
  String threadMessageIds(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Meddelande-ID',
      one: 'Meddelande-ID',
    );
    return '$_temp0';
  }

  @override
  String get threadWritten => 'Skrivet';

  @override
  String get threadPartsHere => 'Mottagna delar';

  @override
  String threadPartsOf(int here, int total) {
    return '$here av $total';
  }

  @override
  String get threadSendTo => 'Skicka till';

  @override
  String threadLastSeenThere(String ago) {
    return 'Senast sedd där $ago';
  }

  @override
  String threadWhereWas(String name, String ago) {
    return 'Där $name var $ago';
  }

  @override
  String get threadThereHow => 'Snabbt om de är kvar där';

  @override
  String get threadTheirHome => 'Deras hemnod';

  @override
  String get threadHomeHow => 'Väntar i deras hem tills de hämtar det';

  @override
  String threadNotSent(String error) {
    return 'Inte skickat: $error';
  }

  @override
  String get threadNotWritten => 'Har inte skrivit än';

  @override
  String threadLastWrote(String ago) {
    return 'Skrev senast $ago';
  }

  @override
  String threadLastWroteHome(String ago) {
    return 'Skrev senast $ago, hemifrån';
  }

  @override
  String threadLastWroteAway(String ago) {
    return 'Skrev senast $ago, inte hemifrån';
  }

  @override
  String get threadRename => 'Byt namn';

  @override
  String get threadShareHome => 'Dela min hemnod';

  @override
  String get threadArchiveConversation => 'Arkivera samtalet';

  @override
  String contactArchived(String name) {
    return '$name: kontakten arkiverad';
  }

  @override
  String get threadArchiveContact => 'Arkivera kontakten';

  @override
  String threadRemoveTitle(String name) {
    return 'Ta bort $name?';
  }

  @override
  String get threadRemoveHow =>
      'Meddelandena blir kvar. Lägg till deras kort för att skriva igen.';

  @override
  String get threadRemove => 'Ta bort';

  @override
  String get threadRemoveContact => 'Ta bort kontakten';

  @override
  String get threadNotVerified =>
      'Inte verifierad än: deras första svar visar att det är de.';

  @override
  String get threadIsArchived => 'Samtalet är arkiverat.';

  @override
  String get threadWasRemoved => 'Kontakten har tagits bort.';

  @override
  String threadNoNode(String name) {
    return 'Kortet från $name anger ingen nod än: be om ett nytt.';
  }

  @override
  String get name => 'Namn';

  @override
  String threadParts(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count delar',
      one: '$count del',
    );
    return '$_temp0';
  }

  @override
  String threadTooManyParts(int count, int max) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count delar, högst $max',
      one: '$count del, högst $max',
    );
    return '$_temp0';
  }

  @override
  String get threadHint => 'Meddelande';

  @override
  String get threadSend => 'Skicka';

  @override
  String threadTheyReacted(String emoji) {
    return 'Deras reaktion: $emoji';
  }

  @override
  String threadYouReacted(String emoji) {
    return 'Din reaktion: $emoji';
  }

  @override
  String get threadCopy => 'Kopiera text';

  @override
  String get threadToWhere => 'Dit de var';

  @override
  String get threadToHome => 'Till deras hem';

  @override
  String get threadTakenMark =>
      'Skickat ut i nätet, som inte säger när det kommer fram';

  @override
  String threadReactionSeen(String name) {
    return '$name ser din reaktion på sitt meddelande.';
  }

  @override
  String get emojiSearch => 'Sök emoji på engelskt namn';

  @override
  String get emojiNone => 'Ingen emoji med det namnet.';

  @override
  String get emojiSmileys => 'Smileys och känslor';

  @override
  String get emojiPeople => 'Människor och kropp';

  @override
  String get emojiAnimals => 'Djur och natur';

  @override
  String get emojiFood => 'Mat och dryck';

  @override
  String get emojiTravel => 'Resor och platser';

  @override
  String get emojiActivities => 'Aktiviteter';

  @override
  String get emojiObjects => 'Föremål';

  @override
  String get emojiSymbols => 'Symboler';

  @override
  String get emojiFlags => 'Flaggor';

  @override
  String get sendNoReturnAddress => 'ingen svarsadress än';

  @override
  String sendNoAddress(String name) {
    return '$name har ingen adress än';
  }

  @override
  String sendTooLong(int count, int max) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'för långt: $count delar, högst $max',
      one: 'för långt: $count del, högst $max',
    );
    return '$_temp0';
  }

  @override
  String get sendCharTooLong => 'ett tecken är för långt för att skickas';

  @override
  String sessionBadValue(String error) {
    return 'Noden skickade ett värde som appen inte kan läsa: $error';
  }

  @override
  String sessionReadFailed(String error) {
    return 'Det gick inte att läsa noden: $error';
  }

  @override
  String get sessionNotConnected => 'inte ansluten';

  @override
  String sessionPairedOnly(String what) {
    return '$what är bara för den parkopplade telefonen';
  }

  @override
  String get sessionHomeTaken =>
      'Ditt hem sparar inte längre din post. Välj det igen i Inställningar.';

  @override
  String sessionGivenBackUnknown(String id) {
    return 'Noden gav tillbaka meddelande $id, som appen inte har skickat.';
  }

  @override
  String sessionSendFailed(String error) {
    return 'Det gick inte att skicka: $error';
  }

  @override
  String sessionConfirmRefused(String error) {
    return 'Noden avvisade bekräftelsen av inkorgen: $error';
  }

  @override
  String sessionInboxFailed(String error) {
    return 'Det gick inte att läsa inkorgen: $error';
  }

  @override
  String sessionPlaceFailed(String error) {
    return 'Det gick inte att ange nodens plats: $error';
  }

  @override
  String get sessionNoNewMail => 'Ingen ny post';

  @override
  String sessionMoreAtHome(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count till väntar i ditt hem: hämta igen',
      one: '$count till väntar i ditt hem: hämta igen',
    );
    return '$_temp0';
  }

  @override
  String sessionClockNone(String offset) {
    return 'ingen tid, tidszon $offset';
  }

  @override
  String sessionClockGps(String time, String offset) {
    return '$time från GPS, tidszon $offset';
  }

  @override
  String sessionClockPhone(String time, String offset) {
    return '$time från telefonen, tidszon $offset';
  }

  @override
  String get card => 'Kort';

  @override
  String get scanTheirCard => 'Skanna deras kort';

  @override
  String get contactsNone =>
      'Inga kontakter än. Tryck på Lägg till kontakt och skanna deras kort.';

  @override
  String contactsArchived(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count arkiverade',
      one: '$count arkiverad',
    );
    return '$_temp0';
  }

  @override
  String get contactsAdd => 'Lägg till kontakt';

  @override
  String get contactsNotVerified => 'Inte verifierad än';

  @override
  String get contactsHasHome => 'Har en hemnod';

  @override
  String get contactsTemporaryNoNode => 'tillfälligt: ingen känd nod än';

  @override
  String contactsTemporaryAt(String ago) {
    return 'tillfälligt: på noden de använde $ago';
  }

  @override
  String get contactsPasteHelp => 'Eller klistra in kortet de skickade dig';

  @override
  String get contactsHomeNode => 'Hemnod';

  @override
  String get contactsNoHomeNoNode =>
      'ingen, och ingen nod än: be om ett nytt kort';

  @override
  String get contactsNoHomeTemporary => 'ingen: ett tillfälligt kort';

  @override
  String contactsBadPaste(String error, String prefix) {
    return '$error. Klistra in ett kort: det börjar med $prefix';
  }

  @override
  String contactsUnnamed(String start) {
    return 'Namnlös ($start…)';
  }

  @override
  String get cardShare => 'Dela';

  @override
  String cardShareFailed(String error) {
    return 'Det gick inte att dela: $error';
  }

  @override
  String get cardNameNotSet => 'inte angivet';

  @override
  String get cardChangeName => 'Ändra';

  @override
  String get cardDetails => 'Detaljer';

  @override
  String get cardMyIdentity => 'Min identitet';

  @override
  String get cardCopyIdentity => 'Kopiera identiteten';

  @override
  String get cardIdentityCopied => 'Identiteten kopierad';

  @override
  String get cardHomeAddress => 'Hemadress';

  @override
  String get cardCopyAddress => 'Kopiera adressen';

  @override
  String get cardAddressCopied => 'Adressen kopierad';

  @override
  String get cardNodeNow => 'Noden jag använder nu';

  @override
  String get cardCopyNode => 'Kopiera noden';

  @override
  String get cardNodeCopied => 'Noden kopierad';

  @override
  String get cardCopyCard => 'Kopiera kortet';

  @override
  String get cardCardCopied => 'Kortet kopierat';

  @override
  String get cardNameFirst =>
      'Ange ditt namn först: den som skannar ditt kort behöver veta det.';

  @override
  String get cardSetName => 'Ange mitt namn';

  @override
  String get cardTemporaryHow =>
      'Tillfälligt kort: utan hemnod anger det noden du använder nu. Välj ett hem i Inställningar.';

  @override
  String get cardHomeHow => 'Det anger adressen till din hemnod.';

  @override
  String get cardTemporary => 'Tillfälligt kort';

  @override
  String get cardTemporaryMark => 'TILLFÄLLIGT';

  @override
  String get cardNot => 'Inte ett kort';

  @override
  String cardNewer(int version) {
    return 'Ett kort från en nyare app (kortversion $version)';
  }

  @override
  String cardOlder(int version) {
    return 'Ett kort från en äldre app (kortversion $version)';
  }

  @override
  String get cardNameNotUtf8 => 'Inte ett kort: namnet är inte UTF-8';

  @override
  String get cardNameTooLong => 'Namnet är för långt';

  @override
  String get scanTitle => 'Skanna ett kort';

  @override
  String get scanNoCamera =>
      'Appen får inte använda kameran. Tillåt det i telefonens inställningar eller klistra in kortet.';

  @override
  String get scanPoint => 'Rikta kameran mot kortet på den andra telefonen';

  @override
  String get factRole =>
      'Ett relä har extern ström, till exempel USB, och lyssnar hela tiden och skickar vidare meddelanden åt noderna runt omkring: ju fler reläer, desto längre når meddelandena. En sömnig nod går på batteri: för att räcka sover den och vaknar bara i korta fönster, och den skickar och tar emot genom ett relä nära sig, sitt ankare. Ett stillastående relä som förlorar strömmen fortsätter som relä på batteri tills batteriet börjar ta slut, och blir då sömnigt. Håller du in nodens knapp byter den roll tills dess ström ändras nästa gång.';

  @override
  String get factMoving =>
      'Noden avgör det själv utifrån sin GPS: efter drygt en minut på andra positioner än där den stod räknas den som buren, och efter fyra timmar på ett ställe är den stillastående igen. En buren nods plats följer dess GPS, och den tar inte emot ny post åt andra, eftersom den snart kan vara borta. Anger du dess plats i appens Inställningar blir den stillastående direkt.';

  @override
  String get factAnchor =>
      'En sömnig nod skickar och tar emot genom ett relä nära sig, sitt ankare. Ankaret sparar den sömniga nodens post och lämnar över den i det korta fönstret efter varje gång den vaknar, och den sömniga nodens adress är ankarets plats. En sömnig nod tar det första relä den hör som sitt ankare; utan något än sparar den och skickar vidare meddelanden själv, vilket belastar kanalen mer.';

  @override
  String get factPlace =>
      'Meddelanden hittar en nod genom dess plats: en adress är en position och nodens ID, och varje relä skickar ett meddelande vidare mot den positionen. Platsen kommer från nodens GPS, om inte någon har angett den i appens Inställningar, vilket går före, som för ett relä inomhus utan fri sikt mot himlen. En nod utan plats skickar inga hjärtslag, så inget meddelande kan hitta den förrän den har en.';

  @override
  String get factPower =>
      'USB betyder extern ström, och med den kan en nod vara relä hela tiden. På batteri räknas värdet ut från cellens spänning, så det är ungefärligt; i den här nodens egen vy är det den råa avläsningen i millivolt. Vid ungefär en femtedel kvar räknas batteriet som lågt, och ett relä på det blir sömnigt för att spara resten.';

  @override
  String get factLink =>
      'Hur väl den här noden hör den andra: hur högt dess signal ligger över radiobruset här. Under ungefär 4 dB går ett helt meddelande ofta förlorat när signalen sviktar, så länken är skör och används bara när ingen annan finns kvar; från ungefär 7 dB är den bra; däremellan medelgod. Den mäts åt ett håll: den andra noden kan höra den här bättre eller sämre.';

  @override
  String get factLastHeard =>
      'Varje nod skickar då och då ett kort hjärtslag som säger vem och var den är. Ett relä skickar ett med 5 till 15 sekunders mellanrum, vart och ett på nästa av 13 kanaler, så den här noden hör det ungefär en gång på en till tre minuter; en sömnig nod vaknar och skickar ett med två och en halv till fem minuters mellanrum. Ett relä som inte hörts på sex och en halv minut, eller en sömnig nod på tio, försvinner från listan.';

  @override
  String get factDistance =>
      'Den raka linjen från den här nodens plats till den andras, uträknad av appen från de två positionerna, så den visas bara när båda har en plats. Hur långt radio når beror på höjd och på vad som står emellan: reläer på hustak hör varandra på ungefär 3 km.';

  @override
  String get factKnown =>
      'Vad noden säger om sina egna grannar, i tre tal. Hörda: noderna vars hjärtslag den hör. Starka: av dem, de som hörs så väl att ett helt meddelande kommer igenom när signalen sviktar, så de hör troligen den också. Utbytta: av dem, de som har tagit emot ett meddelande från den, vilket bevisar att de hör den.';

  @override
  String factGpsSignal(int floor) {
    return 'Hur starkt nodens GPS hör satelliterna, i dB-Hz, enligt dess senaste avläsning. Under $floor tar noden ingen position från den; fri himmel ger ungefär 40. Flytta noden dit den visar mer, eller ange dess plats för hand.';
  }

  @override
  String get factVersion =>
      'Versionen av programvaran som noden kör. En ny version sprids av sig själv: en nod som får den, från en telefon eller via USB, skickar den till reläerna runt sig en bit i taget och lämnar kanalerna åt meddelandena. En nod tar bara emot en nyare version som projektet har signerat, och medan den samlar in en visar den här raden vilken och hur många av dess block den har.';

  @override
  String get factExchanged =>
      'Den här noden har lämnat ett meddelande till den, och den tog emot det. Det bevisar att länken fungerar åt båda hållen: den hör den här noden, och hörs själv.';

  @override
  String get factDemoted =>
      'Den svarade inte på den här nodens tre senaste försök att lämna ett meddelande till den, så den kanske inte längre hör den här noden. Tills den svarar igen, eller i sex och en halv minut, provas den först efter alla andra grannar med tillräckligt bra länk.';

  @override
  String get factAvoided =>
      'Den avböjde ett meddelande och bad om väntan, eller svarade inte på ett. Inget skickas till den på ett tag; meddelanden går runt den så länge och väntar på den bara där det inte finns någon annan väg.';

  @override
  String get factPaired =>
      'Den här telefonen är parkopplad med den här noden: noden skickar ut dina meddelanden i nätet och sparar dem som är till dig tills appen läser dem. Bara den parkopplade telefonen ser nodens grannar på den här kartan.';
}
