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
  String get usersChannel => 'Telegramkanal';

  @override
  String get undo => 'Ångra';

  @override
  String get conversationsNoBrowser => 'Ingen webbläsare att öppna sidan i';

  @override
  String get conversationsBackupTitle => 'Skriv ner dina 12 reservord';

  @override
  String get conversationsBackupWhy =>
      'Om du tappar bort telefonen är de enda sättet att få tillbaka dina kontakter.';

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
  String get startNodeHow => 'Appen hittar en själv när den är i närheten.';

  @override
  String get startContact => 'Din första kontakt';

  @override
  String get startContactHow => 'Skanna personens kort, eller visa ditt.';

  @override
  String barHearsNoOne(String node) {
    return '$node hör ingen sedan uppdateringen';
  }

  @override
  String get barHearsNoOneWhy =>
      'Den nya programvaran kan ha slagit ut radion. Tryck för att gå tillbaka.';

  @override
  String barAppOut(int version) {
    return 'Appversion $version finns nu';
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
    return 'Nodprogramvara $version finns nu';
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
  String get sentWriting => 'lämnas över till noden';

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
  String get sentNoNeighbour =>
      'noden hör ingen annan, skickar när den gör det';

  @override
  String get sentNoWay => 'hittade ingen väg dit';

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
      'Väntar i telefonen tills en nod är ansluten och kan ta emot det.';

  @override
  String get sentWritingHelp => 'Telefonen lämnar över det till noden.';

  @override
  String get sentTakenHelp =>
      'Noden har det och skickar det vidare. Nätet säger inte när det kommer fram; det lämnar tillbaka det om det inte hittar någon väg dit.';

  @override
  String get sentBusy => 'Nätet är upptaget';

  @override
  String get sentBusyHelp =>
      'Nätet lämnade tillbaka det för stunden. Appen skickar det igen av sig själv när väntetiden är slut.';

  @override
  String get sentNoNeighbourHelp =>
      'Noden hör ingen annan nod just nu. Appen skickar det igen av sig själv så fort noden hör en.';

  @override
  String get sentNoWayHelp =>
      'Nätet hittade ingen väg till adressen: noden kan vara avstängd eller borta, eller adressen inaktuell. Skicka igen försöker en gång till, som ett nytt meddelande; Avbryt tar bort det.';

  @override
  String get sentRefusedHelp =>
      'Noden tar inte emot meddelandet som det är. Det beror på ett fel i appen eller noden. Avbryt tar bort det; Felsökning visar orsaken.';

  @override
  String get sentReaction => 'Mottagarens reaktion';

  @override
  String get sentReactionHelp =>
      'Emojin i meddelandets hörn, om mottagaren valde en.';

  @override
  String get receivedYourReaction => 'Din reaktion';

  @override
  String get receivedYourReactionHelp =>
      'Håll in ett meddelande för att reagera med en emoji eller kopiera det. Avsändaren ser reaktionen när den kommer fram. Välj en annan för att byta.';

  @override
  String get receivedFirst => 'Första meddelandet';

  @override
  String get receivedFirstHelp =>
      'Vem som helst med ditt kort kan ha skickat ett första meddelande, så det är inte säkert vem det här kommer från.';

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
    return '$reason. Du är fortfarande gäst här.';
  }

  @override
  String commonPairingFailed(String error) {
    return 'Parkopplingen misslyckades: $error. Du är fortfarande gäst här.';
  }

  @override
  String commonGift(String name, String node) {
    return '$name gav dig ett hem på $node';
  }

  @override
  String get commonGiftTitle => 'Ett hem i gåva';

  @override
  String get commonGiftHelp =>
      'Ägaren till en hemnod har lagt till dig där. Om du använder det blir det ditt hem: det sparar meddelanden till dig medan du är borta, och ditt kort får hemmets adress, så dina kontakter får ditt nya kort.\n\nEtt hem du hade förut skickar dig din post och glömmer sedan bort dig.';

  @override
  String get commonUseGift => 'Använd det';

  @override
  String failed(String error) {
    return 'Misslyckades: $error';
  }

  @override
  String get commonHomeOffer =>
      'Du har ingen hemnod än. Vill du göra den här till ditt hem? Den sparar meddelanden till dig medan du är borta. Först parkopplar du med koden på nodens skärm.';

  @override
  String get setHome => 'Gör till hem';

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
  String get commonCardQueued => 'Ditt kort väntar på att skickas';

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
      'bark skickar meddelanden via en nod i närheten, med Bluetooth. Android frågar strax om lov.';

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
  String statusGuestFor30s(String node) {
    return 'Ansluten till $node som gäst i 30 s';
  }

  @override
  String statusGuestFor30sInstalling(String node, int version) {
    return 'Ansluten till $node som gäst i 30 s, version $version installeras';
  }

  @override
  String statusGuestFor30sUpdated(String node, int version) {
    return 'Ansluten till $node som gäst i 30 s, version $version är installerad';
  }

  @override
  String statusGuest(String node) {
    return 'Ansluten till $node som gäst';
  }

  @override
  String statusGuestInstalling(String node, int version) {
    return 'Ansluten till $node som gäst, version $version installeras';
  }

  @override
  String statusGuestUpdated(String node, int version) {
    return 'Ansluten till $node som gäst, version $version är installerad';
  }

  @override
  String statusAtHome(String node) {
    return 'Ansluten till ditt hem, $node';
  }

  @override
  String statusAtHomeInstalling(String node, int version) {
    return 'Ansluten till ditt hem, $node, version $version installeras';
  }

  @override
  String statusAtHomeUpdated(String node, int version) {
    return 'Ansluten till ditt hem, $node, version $version är installerad';
  }

  @override
  String statusPaired(String node) {
    return 'Ansluten till $node, parkopplad';
  }

  @override
  String statusPairedInstalling(String node, int version) {
    return 'Ansluten till $node, parkopplad, version $version installeras';
  }

  @override
  String statusPairedUpdated(String node, int version) {
    return 'Ansluten till $node, parkopplad, version $version är installerad';
  }

  @override
  String statusHomeAt(String node) {
    return 'Hem: $node';
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
      'Ditt hem. Telefonen är ansluten så länge hemmet är inom räckhåll, så meddelanden kommer direkt, även när skärmen är släckt. Telefonen väljer alltid ditt hem i första hand.';

  @override
  String get connectionPaired =>
      'En nod du är parkopplad med. Telefonen är ansluten så länge noden är inom räckhåll.';

  @override
  String get connectionPairedNoHome =>
      'Eftersom du inte har något hem pekar ditt kort på den här noden så länge du använder den.';

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
      'Som gäst ansluter telefonen ungefär en gång i minuten, och direkt när du skickar. Svar på meddelanden som skickats härifrån kommer hit.';

  @override
  String get connectionBluetoothOff =>
      'Bluetooth är av. Meddelanden väntar på telefonen tills det är på och en nod är inom räckhåll.';

  @override
  String get connectionLookPublic =>
      'Ingen egen nod att använda, så telefonen letar var 15:e sekund efter en publik nod att använda som gäst.';

  @override
  String get connectionLookPaired =>
      'Telefonen letar var 15:e sekund efter dina parkopplade noder, ditt hem först. Hittar den ingen på 30 sekunder använder den så länge en publik nod som gäst.';

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
      'Väntande meddelanden skickas när noden är inom räckhåll';

  @override
  String get notifyOutBluetooth =>
      'Väntande meddelanden skickas när Bluetooth är på';

  @override
  String get notifyUnnamed => 'Namnlös';

  @override
  String get notifyChannel => 'Meddelanden';

  @override
  String get notifyChannelAbout => 'Ett meddelande från en kontakt';

  @override
  String get notifyLinkChannel => 'Anslutning till noden';

  @override
  String get notifyLinkChannelAbout =>
      'Visas medan appen är ansluten till en nod eller letar efter den';

  @override
  String get you => 'Du';

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
  String get threadSealedForThem => 'bara för mottagaren';

  @override
  String get threadSealedIntroOut => 'som ett första meddelande, med ditt kort';

  @override
  String get threadSealedForYou => 'av avsändaren, bara för dig';

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
  String get threadThereHow => 'Snabbt om personen är kvar där';

  @override
  String get threadTheirHome => 'Personens hemnod';

  @override
  String get threadHomeHow => 'Väntar i hemmet tills personen hämtar det';

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
      'Meddelandena blir kvar. Lägg till kortet igen om du vill skriva.';

  @override
  String get remove => 'Ta bort';

  @override
  String get threadRemoveContact => 'Ta bort kontakten';

  @override
  String get threadNotVerified =>
      'Inte verifierad än: det första svaret visar att det är rätt person.';

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
    return 'Mottagarens reaktion: $emoji';
  }

  @override
  String threadYouReacted(String emoji) {
    return 'Din reaktion: $emoji';
  }

  @override
  String get threadCopy => 'Kopiera text';

  @override
  String get threadToWhere => 'Dit personen var';

  @override
  String get threadToHome => 'Till personens hem';

  @override
  String get threadCancelSending => 'Avbryt sändningen';

  @override
  String get threadReact => 'Reagera';

  @override
  String get threadSendAgain => 'Skicka igen';

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
  String sendTextNoWay(String name) {
    return 'Ditt meddelande till $name hittade ingen väg dit.';
  }

  @override
  String sendReactionNoWay(String emoji, String name) {
    return 'Din reaktion $emoji hittade ingen väg till $name.';
  }

  @override
  String sendCardNoWay(String name) {
    return 'Ditt kort hittade ingen väg till $name.';
  }

  @override
  String sendHomeNoWay(String name) {
    return 'Hemmet du gav $name hittade ingen väg fram.';
  }

  @override
  String sessionBadValue(String error) {
    return 'Noden skickade ett värde som appen inte kan läsa: $error';
  }

  @override
  String sessionReadFailed(String error) {
    return 'Det gick inte att läsa från noden: $error';
  }

  @override
  String get notConnected => 'inte ansluten';

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
  String sendFailed(String error) {
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
  String get scanTheirCard => 'Skanna personens kort';

  @override
  String get contactsNone =>
      'Inga kontakter än. Tryck på Lägg till kontakt och skanna personens kort.';

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
    return 'tillfälligt: på noden som användes $ago';
  }

  @override
  String get contactsPasteHelp => 'Eller klistra in ett kort du har fått';

  @override
  String get homeNode => 'Hemnod';

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
  String get nameNotSet => 'inte angivet';

  @override
  String get changeName => 'Ändra';

  @override
  String get cardDetails => 'Detaljer';

  @override
  String get myIdentity => 'Min identitet';

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
      'Tillfälligt kort: utan hemnod pekar det på noden du använder nu. Välj ett hem i Inställningar.';

  @override
  String get cardHomeHow => 'Det innehåller adressen till din hemnod.';

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
      'Ett relä har extern ström, till exempel USB, och lyssnar hela tiden och skickar vidare meddelanden åt noderna runt omkring: ju fler reläer, desto längre når meddelandena. En sömnig nod går på batteri: för att batteriet ska räcka sover den och vaknar bara korta stunder, och den skickar och tar emot via ett relä i närheten, sitt ankare. Ett relä som står still och förlorar strömmen fortsätter som relä på batteri tills batteriet börjar ta slut, och blir då sömnigt. Om du håller in nodens knapp byter den roll tills strömförsörjningen ändras nästa gång.';

  @override
  String get factMoving =>
      'Noden avgör det själv med sin GPS: efter drygt en minut på andra positioner än där den stod räknas den som en nod som bärs omkring, och efter fyra timmar på samma ställe räknas den som stillastående igen. Platsen för en nod som bärs omkring tas från dess GPS, och den tar inte emot ny post åt andra, eftersom den snart kan vara borta. Om du anger platsen i appens Inställningar räknas noden som stillastående direkt.';

  @override
  String get factAnchor =>
      'En sömnig nod skickar och tar emot via ett relä i närheten, sitt ankare. Ankaret sparar den sömniga nodens post och lämnar över den under den korta stund efter att noden har vaknat, och den sömniga nodens adress är ankarets plats. En sömnig nod tar det första relä den hör som sitt ankare; innan den har något sparar den och skickar vidare meddelanden själv, vilket belastar kanalen mer.';

  @override
  String get factPlace =>
      'Meddelanden hittar en nod via dess plats: en adress är en position och nodens ID, och varje relä skickar ett meddelande vidare mot den positionen. Platsen kommer från nodens GPS, om ingen har angett den i appens Inställningar. En angiven plats gäller i stället för GPS, till exempel för ett relä inomhus utan fri sikt mot himlen. En nod utan plats skickar inga hjärtslag, så inget meddelande kan hitta den förrän den har en.';

  @override
  String get factPower =>
      'USB betyder extern ström, och med den kan en nod vara relä hela tiden. På batteri räknas värdet ut från cellens spänning, så det är ungefärligt; i den här nodens egen vy visas den råa avläsningen i millivolt. När ungefär en femtedel är kvar räknas batteriet som lågt, och ett relä på batteri blir då sömnigt för att spara resten.';

  @override
  String get factLink =>
      'Hur bra den här noden hör den andra: hur högt dess signal ligger över radiobruset här. Under ungefär 4 dB går ett helt meddelande ofta förlorat när signalen sviktar, så länken är skör och används bara när ingen annan finns kvar; från ungefär 7 dB är den bra; däremellan medelgod. Den mäts åt ett håll: den andra noden kan höra den här bättre eller sämre.';

  @override
  String get factLastHeard =>
      'Varje nod skickar då och då ett kort hjärtslag som säger vem och var den är. Ett relä skickar ett med 5 till 15 sekunders mellanrum, vart och ett på nästa av 13 kanaler, så den här noden hör det ungefär en gång på en till tre minuter; en sömnig nod vaknar och skickar ett med två och en halv till fem minuters mellanrum. Ett relä som inte hörts på sex och en halv minut, eller en sömnig nod på tio, försvinner från listan.';

  @override
  String get factDistance =>
      'Den raka linjen från den här nodens plats till den andras, uträknad av appen från de två positionerna, så den visas bara när båda har en plats. Hur långt radio når beror på höjd och på vad som står emellan: reläer på hustak hör varandra på ungefär 3 km.';

  @override
  String get factKnown =>
      'Vad noden säger om sina egna grannar, i tre tal. Hörda: noderna vars hjärtslag den hör. Starka: av dem, de som hörs så väl att ett helt meddelande kommer igenom när signalen sviktar, så de hör troligen den också. Med utbyte: av dem, de som har tagit emot ett meddelande från den, vilket bevisar att de hör den.';

  @override
  String factGpsSignal(int floor) {
    return 'Hur starkt nodens GPS hör satelliterna, i dB-Hz, vid den senaste avläsningen, och hur länge sedan den var: en nod som står still låter sin GPS vila, så avläsningen kan vara gammal. Under $floor tar noden ingen position från den; fri himmel ger ungefär 40. Flytta noden till en plats där värdet är högre, eller ange platsen för hand.';
  }

  @override
  String get factVersion =>
      'Versionen av programvaran som noden kör. En ny version sprids av sig själv: en nod som får den, från en telefon eller via USB, skickar den till reläerna runt omkring en bit i taget och lämnar kanalerna åt meddelandena. En nod tar bara emot en nyare version som projektet har signerat, och medan den hämtar en visar den här raden vilken version och hur många block den har fått.';

  @override
  String get factExchanged =>
      'Den här noden har lämnat ett meddelande till den, och den tog emot det. Det bevisar att länken fungerar åt båda hållen: den hör den här noden, och hörs själv.';

  @override
  String get factDemoted =>
      'Den svarade inte på den här nodens tre senaste försök att lämna ett meddelande till den, så den kanske inte längre hör den här noden. Tills den svarar igen, eller i sex och en halv minut, provas den först efter alla andra grannar med tillräckligt bra länk.';

  @override
  String get factAvoided =>
      'Den tackade nej till ett meddelande och bad avsändaren vänta, eller svarade inte på ett. Inget skickas till den på ett tag; meddelanden går runt den så länge och väntar på den bara där det inte finns någon annan väg.';

  @override
  String get factPaired =>
      'Den här telefonen är parkopplad med den här noden: noden skickar ut dina meddelanden i nätet och sparar dem som är till dig tills appen läser dem. Bara den parkopplade telefonen ser nodens grannar på den här kartan.';

  @override
  String get factRoleLabel => 'Roll';

  @override
  String get factMovingLabel => 'Rörelse';

  @override
  String get factAnchorLabel => 'Ankare';

  @override
  String get factPlaceLabel => 'Plats';

  @override
  String get factPowerLabel => 'Ström';

  @override
  String get factLinkLabel => 'Länk';

  @override
  String get factLastHeardLabel => 'Senast hörd';

  @override
  String get factDistanceLabel => 'Avstånd';

  @override
  String get factKnownLabel => 'Kända grannar';

  @override
  String get gpsSignal => 'GPS-signal';

  @override
  String get factVersionLabel => 'Version';

  @override
  String get factExchangedLabel => 'Utbyte';

  @override
  String get factDemotedLabel => 'Nedprioriterad';

  @override
  String get factAvoidedLabel => 'Undviks';

  @override
  String get factPairedLabel => 'Parkopplad';

  @override
  String get factRoleSleepyName => 'roll, sömnig';

  @override
  String get factRoleRelayName => 'roll, relä';

  @override
  String get factMovingName => 'rörelse';

  @override
  String get factAnchorName => 'ankare';

  @override
  String get factPlaceName => 'plats';

  @override
  String get factPlaceByPersonName => 'plats, angiven för hand';

  @override
  String get factPlaceFromGpsName => 'plats, från GPS';

  @override
  String get factPlaceAnchorsName => 'plats, ankarets';

  @override
  String get factPowerName => 'ström';

  @override
  String get factBatteryName => 'batteri';

  @override
  String get factLinkGoodName => 'länk, bra';

  @override
  String get factLinkFairName => 'länk, medelgod';

  @override
  String get factLinkFragileName => 'länk, skör';

  @override
  String get factLastHeardName => 'senast hörd';

  @override
  String get factDistanceName => 'avstånd';

  @override
  String get factKnownName => 'kända grannar';

  @override
  String get factVersionName => 'programvaruversion';

  @override
  String get factPairedName => 'parkopplad';

  @override
  String get factExchangedName => 'utbyte';

  @override
  String get factDemotedName => 'nedprioriterad';

  @override
  String get factAvoidedName => 'undviks';

  @override
  String get factRoleSleepy =>
      'sömnig: vaknar korta stunder, kommunicerar via sitt ankare';

  @override
  String get factRoleRelay => 'relä: skickar vidare andras meddelanden';

  @override
  String get factCarried => 'bärs omkring';

  @override
  String get factFixed => 'står still';

  @override
  String get factAnchored => 'förankrad';

  @override
  String factAnchoredTo(String node) {
    return 'förankrad vid $node';
  }

  @override
  String get factNoAnchor => 'inget ankare än';

  @override
  String get factThisNodesAnchor => 'den här nodens ankare';

  @override
  String get factNoPlace => 'ingen plats än';

  @override
  String factPlaceByPerson(String place) {
    return 'angiven för hand\n$place';
  }

  @override
  String factPlaceFromGps(String place) {
    return 'från GPS\n$place';
  }

  @override
  String factPlaceAnchors(String place) {
    return 'ankarets\n$place';
  }

  @override
  String get factBatteryLow => 'lågt batteri';

  @override
  String get notRead => 'inte avläst';

  @override
  String factPercent(int percent) {
    return '$percent %';
  }

  @override
  String millivolts(int voltage) {
    return '$voltage mV';
  }

  @override
  String get noReading => 'ingen avläsning';

  @override
  String factGpsTooWeak(int signal) {
    return '$signal dB-Hz · för svag för en position';
  }

  @override
  String factGpsEnough(int signal) {
    return '$signal dB-Hz · räcker för en position';
  }

  @override
  String get factGpsNoSatellites => 'inga satelliter';

  @override
  String factGpsAgo(String reading, String ago) {
    return '$reading, $ago';
  }

  @override
  String get factLinkGood => 'bra';

  @override
  String get factLinkFair => 'medelgod';

  @override
  String get factLinkFragile => 'skör';

  @override
  String factLinkDetail(String rssi, String margin) {
    return 'Den här länken: $rssi dBm, $margin dB över radiobruset.';
  }

  @override
  String timeSecondsAgo(int seconds) {
    return 'för $seconds s sedan';
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
    return 'hörda $heard · starka $strong · med utbyte $exchanged';
  }

  @override
  String factVersionGetting(
    String version,
    String gathers,
    int complete,
    int blocks,
  ) {
    return '$version · hämtar $gathers $complete/$blocks';
  }

  @override
  String get factExchangedValue =>
      'den tog emot ett meddelande från den här noden';

  @override
  String get factDemotedValue => 'den svarade inte på de senaste meddelandena';

  @override
  String get factAvoidedValue =>
      'tills vidare: den avböjde ett meddelande eller svarade inte';

  @override
  String get factPairedWithThis => 'med den här telefonen';

  @override
  String get mapShowMe => 'Visa var jag är';

  @override
  String get mapReadAgain => 'Läs igen';

  @override
  String get mapNotConnected =>
      'Inte ansluten till en nod: anslut för att se dess grannar.';

  @override
  String get mapGuest =>
      'Ansluten som gäst: bara telefonen som är parkopplad med noden ser dess grannar.';

  @override
  String mapReadFailed(String error) {
    return 'Det gick inte att läsa grannarna: $error';
  }

  @override
  String get mapNothing =>
      'Noden har ingen plats än och har inte hört någon granne: det finns inget att visa.';

  @override
  String get mapAttribution => '© OpenStreetMap-bidragsgivare';

  @override
  String get mapNoPlaceNoLines =>
      'Noden har ingen egen plats: dess grannar visas utan linjer till den.';

  @override
  String get mapNoPlaceAtYou =>
      'Noden har ingen egen plats: den visas där du är.';

  @override
  String mapNeighboursNoPlace(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count grannar har ingen plats: de ritas vid den här noden',
      one: '$count granne har ingen plats: den ritas vid den här noden',
    );
    return '$_temp0';
  }

  @override
  String get mapNoTiles =>
      'Ingen karta här: telefonen har inget internet och har inte sparat det här området sedan tidigare. Noderna och länkarna visas på en tom bakgrund.';

  @override
  String mapLastReadFailed(String error) {
    return 'Den senaste läsningen misslyckades: $error';
  }

  @override
  String get mapKindSleepy => 'sömnig';

  @override
  String get mapKindRelay => 'relä';

  @override
  String get mapThisNode => 'den här noden';

  @override
  String get mapAsking => 'Frågar den om ström, grannar och version…';

  @override
  String get mapNoAnswerSleepy =>
      'Den svarade inte: den sover mellan sina vakna stunder.';

  @override
  String get mapNoAnswer => 'Den svarade inte.';

  @override
  String mapNodesHere(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count noder här',
      one: '$count nod här',
    );
    return '$_temp0';
  }

  @override
  String mapNodesHereNamed(int count, String names) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count noder här: $names',
      one: '$count nod här: $names',
    );
    return '$_temp0';
  }

  @override
  String mapNeighboursRead(int count, String time) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count grannar, lästa $time',
      one: '$count granne, läst $time',
    );
    return '$_temp0';
  }

  @override
  String mapBandGood(String good) {
    return 'bra ≥ $good dB';
  }

  @override
  String mapBandFair(String fair, String good) {
    return 'medelgod $fair–$good dB';
  }

  @override
  String mapBandFragile(String fair) {
    return 'skör < $fair dB över radiobruset';
  }

  @override
  String get mapLegendRelay => 'ett relä';

  @override
  String get mapLegendSleepy => 'en sömnig nod';

  @override
  String get mapLegendCarried => 'en nod som bärs omkring';

  @override
  String get mapLegendShared => 'noder på samma plats';

  @override
  String get mapLegendYou =>
      'du: din position stannar på telefonen och skickas eller sparas aldrig';

  @override
  String mapHomeOf(String names) {
    return 'Hem: $names';
  }

  @override
  String get mapYou => 'du';

  @override
  String mapLastSeen(String names) {
    return 'Sågs senast: $names';
  }

  @override
  String mapSeenAgo(String name, String ago) {
    return '$name $ago';
  }

  @override
  String get ok => 'OK';

  @override
  String get debug => 'Felsökning';

  @override
  String get debugMe => 'Jag';

  @override
  String get debugBench => 'Testbänk';

  @override
  String get debugStatus => 'Status';

  @override
  String get debugGivenBackHeading => 'Återlämnat';

  @override
  String get debugConnectPaired =>
      'Anslut en parkopplad nod för att välja en programvarufil.';

  @override
  String get debugPickFirmware => 'Välj en programvarufil';

  @override
  String get debugValue => 'Värde';

  @override
  String get debugEmpty => 'tomt';

  @override
  String debugGivenBack(String id, String why, int wait) {
    return '$id, $why, vänta $wait s';
  }

  @override
  String get debugSent => 'Skickade';

  @override
  String get debugReceived => 'Mottagna';

  @override
  String get debugLog => 'Logg';

  @override
  String get debugNone => 'Inga';

  @override
  String get debugNothingYet => 'Inget än';

  @override
  String get debugIdentity => 'Identitet';

  @override
  String get debugCardAddress => 'Kortets adress';

  @override
  String get debugNoneNoHome => 'ingen: inget hem';

  @override
  String get debugNoValue => 'ingen';

  @override
  String get debugUnsentAtHome => 'Brevlåda: kvar på hemnoden';

  @override
  String get debugFetchRounds => 'Hämtningar';

  @override
  String get debugLastSent => 'Senast skickade';

  @override
  String get debugAppVersion => 'Appversion';

  @override
  String get debugDevBuild => 'utvecklingsversion';

  @override
  String get debugLatestRelease => 'Senaste utgåvan';

  @override
  String get debugNotKnown => 'okänd';

  @override
  String debugRelease(int release, int app, int firmware, String checked) {
    return '$release, app $app, programvara $firmware, kontrollerad $checked';
  }

  @override
  String get debugName => 'Namn';

  @override
  String get debugNodeId => 'Nod-ID';

  @override
  String get debugLink => 'Anslutning';

  @override
  String get debugLinkGuest => 'en gästs';

  @override
  String get debugLinkPaired => 'den parkopplade telefonens';

  @override
  String get debugLinkVersion => 'Anslutningens version';

  @override
  String get debugFirmwareImage => 'Programvara';

  @override
  String get debugClock => 'Klocka';

  @override
  String get debugPublic => 'Publik';

  @override
  String get debugYes => 'ja';

  @override
  String get debugNo => 'nej';

  @override
  String get debugPublicGuest => 'ja: en gäst hittade den';

  @override
  String get debugHome => 'Hem';

  @override
  String get debugAddress => 'Adress';

  @override
  String get debugNoPlace => 'ingen än: ingen plats';

  @override
  String get debugAddressCopied => 'Adressen kopierad';

  @override
  String get debugStatusNotRead => 'Inte avläst';

  @override
  String get debugReady => 'Redo att ta emot ett meddelande';

  @override
  String get debugExternalPower => 'Extern ström';

  @override
  String get debugSleepy => 'Sömnig';

  @override
  String get debugCarried => 'Bärs omkring';

  @override
  String get debugMapPoint => 'Position angiven för hand';

  @override
  String get debugPlaced => 'Har en plats';

  @override
  String get debugAnchorless => 'Utan ankare';

  @override
  String get debugFirstRotation => 'Första rotationen';

  @override
  String debugRotationRunning(String left) {
    return 'pågår, $left kvar';
  }

  @override
  String get debugRotationOver => 'klar';

  @override
  String get debugRelaysKnown => 'Kända reläer';

  @override
  String get debugNeighboursHeard => 'Hörda grannar';

  @override
  String get debugAddressPosition => 'Adressens position';

  @override
  String get debugBattery => 'Batteri';

  @override
  String get debugBatteryNone => 'inte avläst eller på USB';

  @override
  String get debugInbox => 'Meddelanden i inkorgen';

  @override
  String dbHz(int signal) {
    return '$signal dB-Hz';
  }

  @override
  String get pairAgain => 'Parkoppla igen';

  @override
  String get staysYourHome => 'Noden är fortfarande ditt hem.';

  @override
  String get forgetInBluetoothToo =>
      'Glöm noden i Bluetooth-inställningarna också: telefonen har kvar sin parkoppling.';

  @override
  String get settingsBackupAnnounce => 'Reservord: skriv ner dina 12 ord';

  @override
  String get settingsPlaceHere => 'Använd telefonens plats';

  @override
  String settingsPlaceSet(String place) {
    return 'Platsen är nu $place.';
  }

  @override
  String get settingsDegreesPlease => 'Ange latitud och longitud i grader';

  @override
  String get settingsNameHow => 'På ditt kort, så att andra ser det';

  @override
  String get settingsDebugHow => 'Vad anslutningen säger, för en felrapport';

  @override
  String get settingsThisNode => 'Den här noden';

  @override
  String get settingsPublic => 'Publik';

  @override
  String get settingsPublicHow =>
      'Alla telefoner i närheten kan skicka meddelanden via den';

  @override
  String settingsPublicFailed(String error) {
    return 'Det gick inte att ändra Publik: $error';
  }

  @override
  String get settingsUnpair => 'Ta bort parkoppling';

  @override
  String settingsUnpairTitle(String node) {
    return 'Ta bort parkopplingen med $node?';
  }

  @override
  String get settingsUnpairHow =>
      'Noden och den här telefonen glömmer varandra; meddelanden och kontakter finns kvar.';

  @override
  String get settingsUnpairLast =>
      'När ingen telefon är kvar blir noden publik.';

  @override
  String get settingsUnpairOthers =>
      'Telefonen är fortfarande parkopplad med dina andra noder.';

  @override
  String get settingsUnpaired => 'Parkopplingen borttagen.';

  @override
  String get settingsUnpairedPublic =>
      'Parkopplingen borttagen. Noden är publik.';

  @override
  String settingsUnpairFailed(String error) {
    return 'Det gick inte att ta bort parkopplingen: $error';
  }

  @override
  String get settingsPhonesPaired => 'Telefoner parkopplade med den här noden';

  @override
  String get settingsPhonesHelp =>
      'En nod kan ha upp till åtta telefoner. Vem som helst som står vid noden kan parkoppla med koden på skärmen, och varje parkopplad telefon kan ändra nodens inställningar. Ta därför bort telefoner du inte känner igen eller som inte längre används. För att ta bort en telefon krävs också koden på nodens skärm, som visar att du är på plats.\n\nEn telefon med hem här får sin post sparad på den här noden.\n\nEn av dem är ägare och bestämmer vem som har hem här, även på avstånd: ägaren kan ge en kontakt ett hem här, ta bort ett hem eller göra någon annan till ägare. Den som först gör noden till sitt hem blir ägare. Vid noden kan varje parkopplad telefon göra en annan till ägare med koden på skärmen.';

  @override
  String get settingsPhonesFull =>
      'Full: ta bort en innan en annan kan parkoppla.';

  @override
  String get settingsPhonesHome => 'Telefoner med hem här';

  @override
  String get settingsNoOwner => 'Den här noden har ingen ägare';

  @override
  String get settingsBecomeOwner => 'Bli ägare';

  @override
  String get settingsBattery => 'Batteri';

  @override
  String get settingsBatteryExempt => 'bark får köras med skärmen av.';

  @override
  String get settingsBatteryNotExempt =>
      'Med skärmen av kan Android stänga appens anslutning efter några timmar.';

  @override
  String get settingsLetRun => 'Låt bark köras';

  @override
  String get settingsBackup => 'Reservord';

  @override
  String get settingsBackedUp =>
      'Med dina 12 ord får du tillbaka din identitet på en annan telefon.';

  @override
  String get settingsNotBackedUp =>
      'Skriv ner dina 12 ord: tappar du telefonen utan dem förlorar du din identitet.';

  @override
  String get settingsShowWords => 'Visa mina ord';

  @override
  String get settingsRestore => 'Återställ från ord';

  @override
  String get settingsMyWords => 'Mina 12 ord';

  @override
  String get settingsWroteThem => 'Jag har skrivit ner dem';

  @override
  String get settingsRestoreTitle => 'Återställa från ord?';

  @override
  String get settingsRestoreHow =>
      'Telefonen får den identitet som orden ger. Dina kontakter når dig först när de har fått det nya kortet.';

  @override
  String get restore => 'Återställ';

  @override
  String get settingsYourWords => 'Dina 12 ord';

  @override
  String get settingsNotWords => 'Det är inte 12 reservord';

  @override
  String get restoredIdentity => 'Identiteten återställd';

  @override
  String get settingsPlace => 'Plats';

  @override
  String get settingsNoPlace => 'Noden har ingen plats än.';

  @override
  String settingsPlaceByPerson(String address) {
    return 'Angiven för hand: $address.';
  }

  @override
  String settingsPlaceByGps(String address) {
    return 'Från nodens GPS: $address.';
  }

  @override
  String get settingsPlaceHow =>
      'För en nod inomhus, utom räckhåll för GPS. Platsen gäller i stället för GPS tills du rensar den.';

  @override
  String get settingsLatitude => 'Latitud';

  @override
  String get settingsLongitude => 'Longitud';

  @override
  String get settingsPlaceSetButton => 'Ange';

  @override
  String get settingsPlaceClear => 'Rensa';

  @override
  String get settingsNotifications => 'Aviseringar';

  @override
  String get settingsNotificationsWhy =>
      'bark säger till när ett meddelande kommer. Android frågar strax om lov.';

  @override
  String settingsPairingFailed(String error) {
    return 'Parkopplingen misslyckades: $error';
  }

  @override
  String settingsConnectingFailed(String error) {
    return 'Anslutningen misslyckades: $error';
  }

  @override
  String get settingsNotPaired => 'Inte parkopplad';

  @override
  String get settingsNotOurs => 'Den här noden är inte längre din';

  @override
  String settingsNotOursHow(String node) {
    return '$node känner inte längre igen den här telefonen. Parkoppla igen med koden på nodens skärm.';
  }

  @override
  String get nodeConnecting => 'Ansluter…';

  @override
  String get nodeTypeCode => 'Skriv koden från nodens skärm';

  @override
  String get nodePairedConnecting => 'Parkopplad, ansluter…';

  @override
  String get node => 'Nod';

  @override
  String get nodeDisconnect => 'Koppla från';

  @override
  String get nodeStopLooking => 'Sluta leta';

  @override
  String get nodeScan => 'Sök';

  @override
  String get nodeStopScan => 'Stoppa';

  @override
  String nodeScanFailed(String error) {
    return 'Sökningen misslyckades: $error';
  }

  @override
  String get nodeNoneFound => 'Ingen nod hittades i närheten.';

  @override
  String get nodeNoneFoundHow => 'Slå på en eller gå närmare.';

  @override
  String get nodePairedWith => 'Parkopplad med';

  @override
  String get nodeForget => 'Glöm';

  @override
  String nodeLostTitle(String node) {
    return '$node känner inte igen den här telefonen';
  }

  @override
  String get nodeLostHelp =>
      'Någon som är parkopplad med noden har tagit bort den här telefonen, noden har tappat sitt minne, eller så har du flyttat hit från en annan telefon: en parkoppling finns bara på den telefon där den gjordes.\n\nParkoppla igen: parkoppla på nytt med koden på nodens skärm. Visas när noden är inom räckhåll.\n\nGlöm: noden visas inte längre här.';

  @override
  String get nodeConnected => 'Ansluten';

  @override
  String get nodeHomeConnected => 'Ditt hem, ansluten';

  @override
  String get nodeLost => 'Känner inte igen den här telefonen';

  @override
  String get nodeHomeLost => 'Ditt hem, känner inte igen den här telefonen';

  @override
  String get nodeUnserved => 'Kan inte användas än. Tryck för att försöka igen';

  @override
  String get nodeHomeUnserved =>
      'Ditt hem, kan inte användas än. Tryck för att försöka igen';

  @override
  String get nodeAway => 'Inte inom räckhåll';

  @override
  String get nodeHomeAway => 'Ditt hem, inte inom räckhåll';

  @override
  String get nodeNotLooking => 'Letar inte. Tryck för att ansluta';

  @override
  String get nodeHomeNotLooking =>
      'Ditt hem, letar inte. Tryck för att ansluta';

  @override
  String get nodeYourHome => 'Ditt hem';

  @override
  String get nodeFoundPublic => 'publik';

  @override
  String get nodeFoundUnserved => 'kan inte användas än';

  @override
  String get nodePair => 'Parkoppla';

  @override
  String nodePairTitle(String node) {
    return 'Parkoppla med $node?';
  }

  @override
  String get nodePairHow =>
      'Sedan kan du ändra nodens inställningar och uppdatera den. Skriv koden från nodens skärm.';

  @override
  String nodeForgetTitle(String node) {
    return 'Glömma $node?';
  }

  @override
  String get nodeForgetHow => 'Telefonen slutar visa den.';

  @override
  String get nodeForgotten => 'Glömd.';

  @override
  String get nodeForgetLater =>
      'Telefonen tar bort sin parkoppling nästa gång den hör noden, eller så kan du glömma den i Bluetooth-inställningarna nu.';

  @override
  String nodeForgetFailed(String error) {
    return 'Det gick inte att glömma noden: $error';
  }

  @override
  String get peopleMakeOwner => 'Gör till ägare';

  @override
  String get peopleThisPhone => 'den här telefonen';

  @override
  String get peopleYou => 'du';

  @override
  String get peopleOwner => 'ägare';

  @override
  String get peoplePairedToo => 'även parkopplad';

  @override
  String get peopleHomeToo => 'har även hem här';

  @override
  String get peopleLinked => 'ansluten nu';

  @override
  String peopleSeen(String when) {
    return 'sågs $when';
  }

  @override
  String get peopleNeverSeen => 'har aldrig setts';

  @override
  String get peopleChoices => 'Alternativ';

  @override
  String get peopleNoName => 'En telefon utan namn';

  @override
  String peopleRemovePhoneAndHome(String name) {
    return 'Ta bort telefonen som tillhör $name och dess hem?';
  }

  @override
  String get peopleRemoveUnnamedAndHome =>
      'Ta bort telefonen utan namn och dess hem?';

  @override
  String peopleRemovePhone(String name) {
    return 'Ta bort telefonen som tillhör $name?';
  }

  @override
  String get peopleRemoveUnnamed => 'Ta bort telefonen utan namn?';

  @override
  String get peopleRemoveThisHome => 'Ta bort det här hemmet?';

  @override
  String peoplePhoneGoes(String name) {
    return 'Telefonen som tillhör $name kan inte längre använda noden om den inte parkopplas igen.';
  }

  @override
  String get peopleUnnamedGoes =>
      'Telefonen utan namn kan inte längre använda noden om den inte parkopplas igen.';

  @override
  String get peopleNodeStopsMail => 'Noden slutar spara personens post.';

  @override
  String peopleRemoveHome(String name) {
    return 'Ta bort hemmet för $name?';
  }

  @override
  String get peopleRemoveTheirHome => 'Ta bort den här personens hem?';

  @override
  String get peopleHomeStopsMail => 'Ditt hem slutar spara personens post.';

  @override
  String peopleMakeOwnerTitle(String name) {
    return 'Göra $name till ägare?';
  }

  @override
  String get peopleMakeThemOwner => 'Göra den här personen till ägare?';

  @override
  String get peopleOwnerChooses => 'Ägaren väljer vem som har hem här.';

  @override
  String get peopleOwnerChoosesNotYou =>
      'Ägaren väljer vem som har hem här. Det kan du inte längre.';

  @override
  String get peopleTypeCodeNext => 'Sedan skriver du koden som noden visar.';

  @override
  String get peopleShowCode => 'Visa koden';

  @override
  String get peopleGone => 'Redan borta.';

  @override
  String peopleCodeFailed(String error) {
    return 'Det gick inte att be noden om en kod: $error';
  }

  @override
  String get peopleRemoved => 'Borttagen';

  @override
  String peopleIsOwner(String name) {
    return '$name är nu ägare';
  }

  @override
  String get peopleThemOwner => 'Den här personen är nu ägare';

  @override
  String get peopleNotChanged => 'Inget ändrades';

  @override
  String get peopleNotRemoved => 'Inget togs bort';

  @override
  String get peopleWrongCode =>
      'Fel kod, eller för sent. Varje kod fungerar en gång.';

  @override
  String get peopleNewCode => 'Ny kod';

  @override
  String peopleItFailed(String error) {
    return 'Det misslyckades: $error';
  }

  @override
  String get peopleCodeTitle => 'Koden på noden';

  @override
  String peopleCodeHow(String word) {
    return 'Skriv de sex siffrorna under $word på nodens skärm inom 30 s.';
  }

  @override
  String get peopleCode => 'Kod';

  @override
  String linkNeedsFirmware(String node) {
    return '$node behöver nyare programvara. Uppdatera noden.';
  }

  @override
  String linkNeedsApp(String node) {
    return '$node behöver en nyare app. Uppdatera appen.';
  }

  @override
  String linkMalformed(String what) {
    return '$what har fel format';
  }

  @override
  String get linkRefusedHeader =>
      'filen är inte programvara som noden kan läsa';

  @override
  String get linkRefusedVersion =>
      'programvaran är inte nyare än versionen som noden kör';

  @override
  String get linkRefusedBusy =>
      'noden har redan ny kontrollerad programvara: starta om den först';

  @override
  String get linkRefusedIncomplete =>
      'noden har inte fått alla byte av programvaran';

  @override
  String get linkRefusedHash =>
      'bytena som noden har stämmer inte med programvaran; skicka den igen';

  @override
  String get linkRefusedBattery =>
      'noden har lågt batteri; den sparar programvaran och fortsätter när den är på USB eller laddad';

  @override
  String get linkRefusedFlash => 'fel i nodens flashminne';

  @override
  String linkRefusedSignature(String command) {
    return 'programvaran är inte signerad med en nyckel som noden har: en fil som byggts här signeras med `$command`, en från en utgåva är redan signerad';
  }

  @override
  String linkAttError(String code) {
    return 'ATT-fel 0x$code';
  }

  @override
  String linkNotOurs(String node) {
    return '$node känner inte längre igen den här telefonen';
  }

  @override
  String linkBondKept(String node) {
    return 'Telefonen behöll sin gamla parkoppling med $node, så den kan inte använda noden. Glöm den i Bluetooth-inställningarna.';
  }

  @override
  String linkEnded(String node) {
    return 'Anslutningen till $node bröts. Försök igen närmare den';
  }

  @override
  String linkNotPaired(String full) {
    return 'Inte parkopplad: avbruten, fel kod eller så är noden full (skärmen visar $full)';
  }

  @override
  String linkNotSeen(String node) {
    return '$node hittades inte: gå närmare och försök igen';
  }

  @override
  String get linkImageTooShort => 'filen är för kort';

  @override
  String get linkImageNotOne => 'filen är ingen programvara för en nod';

  @override
  String get linkImageFormat => 'filen har ett okänt format';

  @override
  String get linkImageLength => 'filens längd stämmer inte';

  @override
  String get linkImageHash => 'filens kontrollsumma stämmer inte';

  @override
  String get fullNode =>
      'Den här noden är full: åtta telefoner är parkopplade med den.';

  @override
  String get homeNone => 'Ingen: ditt kort är tillfälligt.';

  @override
  String homeAt(String address) {
    return 'Adress: $address.';
  }

  @override
  String get homeNoPlace =>
      'Den här noden har ingen plats än, så den kan inte bli ett hem.';

  @override
  String get homeNoPlaceSetIt =>
      'Den här noden har ingen plats än, så den kan inte bli ett hem. Ange platsen under Den här noden.';

  @override
  String get homeNeedsPower =>
      'Ett hem måste ha ström hela tiden och stå på ett ställe.';

  @override
  String get homeKeyGone =>
      'Ditt hem sparar inte längre din post. Gör noden till ditt hem igen eller välj ett annat.';

  @override
  String get homeSilent => 'Ditt hem svarade inte på din senaste hämtning.';

  @override
  String get homeHoldsOtherKey =>
      'Den här noden sparar post åt den här telefonen men är inte ditt hem.';

  @override
  String get homeOthers => 'Den här noden är någon annans hem.';

  @override
  String get homeRemove => 'Ta bort hemmet';

  @override
  String get homeForgetMe => 'Glöm mig här';

  @override
  String get homeUseExisting => 'Använd mitt befintliga hem';

  @override
  String homeNoAnswer(String node) {
    return '$node svarade inte. Inget ändrades.';
  }

  @override
  String homeNoWay(String node) {
    return 'Ingen väg hittades till $node. Inget ändrades.';
  }

  @override
  String get mailboxFetchNoWay => 'Ingen väg hittades till ditt hem';

  @override
  String homeAsking(String node) {
    return 'Frågar $node efter din post…';
  }

  @override
  String homeLeaving(String node) {
    return 'Lämnar $node: noden skickar dig din post och glömmer sedan bort dig.';
  }

  @override
  String get homePeople => 'Personer med hem här';

  @override
  String get homeYouOwner => 'Du är ägare till hemmet.';

  @override
  String get homeShowThem => 'Visa listan';

  @override
  String get askAgain => 'Fråga igen';

  @override
  String homeAskFailed(String error) {
    return 'Det gick inte att fråga: $error';
  }

  @override
  String homeSetFailed(String error) {
    return 'Det gick inte att ändra hemmet: $error';
  }

  @override
  String get homeRemoveTitle => 'Ta bort hemmet?';

  @override
  String get homeSetTitle => 'Göra den här noden till hem?';

  @override
  String get homePairFirst =>
      'Först parkopplar du: skriv koden från nodens skärm.';

  @override
  String get homeCardWaits =>
      'Ditt kort ändras inte, men den senaste ändringen väntar fortfarande på Skicka mitt kort.';

  @override
  String get homeCardSame => 'Ditt kort ändras inte.';

  @override
  String get homeCardChanges => 'Ditt kort ändras.';

  @override
  String homeCardGoes(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Ditt nya kort går till dina $count kontakter.',
      one: 'Ditt nya kort går till din kontakt.',
    );
    return '$_temp0';
  }

  @override
  String get homeSet => 'Gör till hem';

  @override
  String get homeHelp =>
      'Ditt hem är en nod som har ström hela tiden och står på ett ställe. Noden sparar meddelanden till dig medan telefonen är borta, och ditt kort anger nodens adress, så kortet fungerar var du än är. Inkorgsknappen på första skärmen, Hämta min post, hämtar meddelandena till noden du är på.\n\nUtan hem är ditt kort tillfälligt: kortet anger noden du använder nu, som sparar din post bara så länge du kommer tillbaka till den.\n\nFör att göra en nod till ditt hem parkopplas telefonen med noden. En vän som äger en hemnod kan också ge dig ett hem där på distans.\n\nNär du väljer ett annat hem eller inget skickar ditt gamla hem dig posten som finns kvar och glömmer sedan bort dig.\n\nEtt hem som slutar svara kan vara avstängt, vara utom räckhåll eller ha glömt bort dig. Hämta igen senare. Svarar hemmet fortfarande inte, gör en annan nod till ditt hem eller välj Ta bort hemmet.\n\nAnvänd mitt befintliga hem är till för en telefon som har fått tillbaka din identitet från dina 12 ord: ange adressen till ditt hem, så blir noden ditt hem igen när den svarar.';

  @override
  String get homePeopleHelp =>
      'Som ägare väljer du, var du än är, vem som har ett hem här: ge en kontakt ett hem här från menyn i samtalet med kontakten, ta bort en persons hem eller gör någon annan till ägare.\n\nVarje ändring går till ditt hem med en hämtning av din post, och svaret kommer tillbaka med posten.';

  @override
  String get homeAddressTitle => 'Adressen till ditt hem';

  @override
  String get homeAddressHow =>
      'De 26 hexadecimala siffrorna i adressen eller ett av dina kort som anger den.';

  @override
  String get homeAddressLabel => 'Adress eller kort';

  @override
  String get homeCardNoHome => 'Det kortet anger inget hem';

  @override
  String get homeNotAddress => 'Varken adress eller kort';

  @override
  String get homeScanCard => 'Skanna ett kort';

  @override
  String get homeAskIt => 'Fråga noden';

  @override
  String get mailboxNoHomeToFetch => 'Ingen hemnod att hämta från';

  @override
  String get mailboxFetchQueued => 'Hämtningen väntar på en nod';

  @override
  String get mailboxAtHome => 'Hemma kommer posten av sig själv';

  @override
  String get mailboxStillWaiting =>
      'Väntar fortfarande på svar på förra hämtningen';

  @override
  String get mailboxNoPlace =>
      'Den här noden har ingen plats än, så posten kan inte komma tillbaka hit';

  @override
  String get mailboxFetching => 'Hämtar din post';

  @override
  String get mailboxNotConnected => 'Inte ansluten till en nod';

  @override
  String get mailboxRefused => 'Noden sa nej. Inget ändrades.';

  @override
  String get mailboxIsHome => 'Den här noden är ditt hem';

  @override
  String get mailboxIsHomeLeft =>
      'Den här noden är ditt hem. Ditt gamla hem skickar dig din post och glömmer sedan bort dig.';

  @override
  String get mailboxNoHome => 'Inget hem';

  @override
  String get mailboxRefusedStillHome =>
      'Noden sa nej och är fortfarande ditt hem.';

  @override
  String get mailboxHomeRemoved => 'Hemmet har tagits bort';

  @override
  String get mailboxHomeRemovedLeft =>
      'Hemmet har tagits bort. Ditt gamla hem skickar dig din post och glömmer sedan bort dig.';

  @override
  String get mailboxNoGift => 'Inget hem att använda';

  @override
  String mailboxNodeIsHome(String node) {
    return '$node är ditt hem';
  }

  @override
  String mailboxNodeIsHomeLeft(String node) {
    return '$node är ditt hem. Ditt gamla hem skickar dig din post och glömmer sedan bort dig.';
  }

  @override
  String get mailboxHasHome => 'Du har redan ett hem';

  @override
  String get mailboxConnectFirst => 'Anslut till en nod först';

  @override
  String mailboxAsking(String node) {
    return 'Frågar $node efter din post';
  }

  @override
  String mailboxKeepsNot(String node) {
    return '$node sparar inte din post. Inget ändrades.';
  }

  @override
  String get mailboxHomeGone =>
      'Ditt hem sparar inte längre din post. Välj ett nytt hem i Inställningar.';

  @override
  String get mailboxRefusedStillKeeps =>
      'Noden sa nej och sparar fortfarande din post.';

  @override
  String get mailboxForgotYou => 'Noden glömde bort dig';

  @override
  String get mailboxNotPaired => 'Inte parkopplad, så noden blev inte ditt hem';

  @override
  String get mailboxNeedsPower =>
      'Parkopplad, men ett hem måste ha ström hela tiden och stå på ett ställe';

  @override
  String get mailboxNoHomeNode => 'Ingen hemnod';

  @override
  String get mailboxSentHome =>
      'Skickat till ditt hem; svaret kommer med din post';

  @override
  String get mailboxNotAtHome => 'Inte ansluten till ditt hem';

  @override
  String get mailboxYouOwner => 'Du är nu ägare';

  @override
  String get mailboxNotTaken => 'Noden gjorde dig inte till ägare';

  @override
  String get mailboxOnlyOwner => 'Bara ägaren till ditt hem kan dela det';

  @override
  String get mailboxNotOwner => 'Enligt ditt hem är du inte ägare';

  @override
  String mailboxShared(String name) {
    return '$name har nu ett hem på din hemnod och får besked om det';
  }

  @override
  String get mailboxHomeFull => 'Ditt hem är fullt: det rymmer åtta personer';

  @override
  String get mailboxRemoved => 'Togs bort från ditt hem';

  @override
  String get mailboxNewOwner => 'Ditt hem har en ny ägare';

  @override
  String mailboxIsOwner(String name) {
    return '$name är nu ägare till ditt hem';
  }

  @override
  String get mailboxGone => 'Redan borta';

  @override
  String get placeDenied => 'Appen får inte använda platsen';

  @override
  String placeNone(String error) {
    return 'Ingen plats: $error';
  }

  @override
  String version(int version) {
    return 'Version $version';
  }

  @override
  String get sendToNode => 'Skicka till noden';

  @override
  String installInstalling(String node, int version) {
    return '$node installerar version $version och är tillbaka om ungefär en minut';
  }

  @override
  String installInstallingLate(String node, int version) {
    return '$node installerar version $version och är inte tillbaka än';
  }

  @override
  String installSentHelp(String node, int version) {
    return '$node startar om med version $version, som den här telefonen skickade, och är borta ungefär en minut. Telefonen väntar på noden och ansluter igen av sig själv.';
  }

  @override
  String installMeshHelp(String node, int version) {
    return '$node fick version $version från noderna runt omkring. Noden skickar först vidare meddelandena den har, i högst tio minuter, och startar sedan om, vilket tar ungefär en minut. Telefonen ansluter igen av sig själv; inget går förlorat.';
  }

  @override
  String get updateTitle => 'Uppdatera programvaran';

  @override
  String get updateNotConnected => 'Inte ansluten till en nod.';

  @override
  String updateRuns(String node, int version) {
    return '$node kör version $version.';
  }

  @override
  String updateRunsSince(String node, int version, int from, String ago) {
    return '$node kör version $version, uppdaterad från version $from $ago.';
  }

  @override
  String get updatesInstall => 'Installera';

  @override
  String get debugCopyLog => 'Kopiera loggen';

  @override
  String get debugLogCopied => 'Loggen kopierad';

  @override
  String get updateLatest => 'Ladda ner senaste utgåvan';

  @override
  String get updatePickFile => 'Välj en fil';

  @override
  String get updatePickedFile => 'en vald fil';

  @override
  String get updateNoFile => 'ingen fil vald';

  @override
  String get updateDownloadFailed =>
      'filen kunde inte laddas ner: kontrollera internetanslutningen';

  @override
  String updateNoFirmware(String error) {
    return 'Det gick inte att hämta programvaran: $error';
  }

  @override
  String get updateHearsNoOne => 'Noden hör ingen';

  @override
  String updateHearsNoOneWarning(int version, String file) {
    return 'Sedan version $version hör noden ingen annan nod: den nya programvaran kan ha slagit ut radion. För att gå tillbaka dubbeltrycker du på nodens återställningsknapp och kopierar $file från den utgåvan till noden via USB; telefonen kan inte skicka en äldre version.';
  }

  @override
  String get updateNoRelease => 'Ingen känd utgåva än.';

  @override
  String updateReleaseNoFirmware(int release) {
    return 'Utgåva $release innehåller ingen programvara för noder.';
  }

  @override
  String updateRunsLatest(int version) {
    return 'Noden kör redan programvaran från den senaste utgåvan, version $version.';
  }

  @override
  String updateRunsNewer(int version) {
    return 'Noden kör en nyare version än programvaran från den senaste utgåvan, $version.';
  }

  @override
  String updateImage(String source, int kib) {
    return '$source, $kib KiB';
  }

  @override
  String updateImageNotNewer(String source, int kib) {
    return '$source, $kib KiB: inte nyare än nodens';
  }

  @override
  String updateOnNode(int held, int total) {
    return '$held av $total KiB på noden';
  }

  @override
  String get updateSending => 'Skickar…';

  @override
  String get updateChecking => 'Noden kontrollerar programvaran…';

  @override
  String get updateGoOn => 'Fortsätt';

  @override
  String get updateTryAgain => 'Försök igen';

  @override
  String updateReboot(int version) {
    return 'Starta om med version $version';
  }

  @override
  String get updateStalled => 'Noden tar inte emot några delar';

  @override
  String get updatePaused =>
      'Anslutningen bröts; uppdateringen fortsätter när noden är tillbaka';

  @override
  String updateHolds(int version) {
    return 'Noden har tagit emot version $version';
  }

  @override
  String get updateLost => 'Noden har förlorat programvaran';

  @override
  String updateChecked(int version) {
    return 'Noden har kontrollerat version $version';
  }

  @override
  String updateCommitFailed(String error) {
    return 'Det gick inte att starta kontrollen av programvaran: $error';
  }

  @override
  String updateAnswered(String error) {
    return 'Noden svarade med $error';
  }

  @override
  String updateRefused(String reason) {
    return 'Noden sa nej: $reason';
  }

  @override
  String get updateRefusedNoReason => 'Noden sa nej utan att säga varför';

  @override
  String get updateRestarts => 'Noden startar om, det tar ungefär en minut';

  @override
  String get updateNotBack =>
      'Noden är inte tillbaka och kan vara utom räckhåll';

  @override
  String updateRebootFailed(String error) {
    return 'Omstarten misslyckades: $error';
  }

  @override
  String updateOtherNode(String node) {
    return 'ansluten till en annan nod än $node';
  }

  @override
  String get updatePairedOnly =>
      'bara en telefon som är parkopplad med noden kan uppdatera den';

  @override
  String updateDidNotStart(int version, int running) {
    return 'Version $version startade inte; noden är tillbaka på version $running';
  }

  @override
  String get updatesHeading => 'Uppdateringar';

  @override
  String get updatesCheck => 'Sök efter uppdateringar';

  @override
  String get updatesUnreached => 'Det gick inte att nå listan över utgåvor';

  @override
  String get updatesApp => 'App';

  @override
  String get updatesDevBuild => 'En utvecklingsversion';

  @override
  String updatesAppOut(int version, int latest) {
    return 'Version $version; $latest finns nu';
  }

  @override
  String updatesAppDownloading(int version, int latest) {
    return 'Version $version; laddar ner $latest…';
  }

  @override
  String updatesAppDownloadingPercent(int version, int latest, int percent) {
    return 'Version $version; laddar ner $latest… $percent %';
  }

  @override
  String updatesAppReady(int version, int latest) {
    return 'Version $version; $latest är nedladdad';
  }

  @override
  String updatesAppFailed(int version, int latest) {
    return 'Version $version; $latest kunde inte laddas ner';
  }

  @override
  String get updatesNodeNewer =>
      'Uppdatera appen för att få nodens nya funktioner';

  @override
  String get updatesNodeFirmware => 'Nodens programvara';

  @override
  String get nodeNotConnected => 'Inte ansluten';

  @override
  String get updatesGuest =>
      'Bara nodens parkopplade telefoner kan uppdatera noden';

  @override
  String updatesFirmwareOut(int version, int latest) {
    return 'Version $version; $latest finns nu';
  }

  @override
  String releaseLine(String app, String node) {
    return '$app; $node';
  }

  @override
  String get releaseAppDev => 'Appen är en utvecklingsversion';

  @override
  String releaseAppOut(int version) {
    return 'Appversion $version finns nu: installera den nedan';
  }

  @override
  String releaseAppUpToDate(int version) {
    return 'Appen är den senaste versionen ($version)';
  }

  @override
  String get releaseNoNode => 'ingen nod ansluten';

  @override
  String get releaseNotYours => 'du kan inte uppdatera noden';

  @override
  String get releaseBench => 'nodens programvara är en testversion';

  @override
  String releaseNoneCarried(int version) {
    return 'noden kör version $version; utgåvan innehåller ingen programvara för noder';
  }

  @override
  String releaseFirmwareOut(int version) {
    return 'nodprogramvara $version finns nu: skicka den nedan';
  }

  @override
  String releaseFirmwareUpToDate(int version) {
    return 'nodens programvara $version är den senaste';
  }

  @override
  String get identityHelp =>
      'Filen innehåller din identitet, ditt namn, ditt hem, dina kontakter och noderna du har parkopplat med, och även dina meddelanden om du kryssar i rutan. Ditt lösenord låser filen: utan lösenordet kan ingen öppna den, inte ens du.\n\nPå den nya telefonen trycker du på Återställ från en fil och parkopplar sedan med dina noder igen: en parkoppling finns bara på telefonen där den gjordes. Ditt hem sparar din post under tiden, och du kan hämta posten från vilken annan nod som helst.\n\nAnvänd en telefon i taget: två telefoner med samma identitet skulle var och en ta emot post som den andra aldrig ser.\n\nDina 12 reservord ger bara tillbaka din identitet.';

  @override
  String get identitySaveHow =>
      'Spara identiteten i en fil för att flytta till en ny telefon.';

  @override
  String get identitySave => 'Spara i en fil';

  @override
  String get identityRestore => 'Återställ från en fil';

  @override
  String get identitySaveTitle => 'Spara min identitet';

  @override
  String get identitySaved => 'Sparat';

  @override
  String get identityNotSaved => 'Inte sparat';

  @override
  String identitySaveFailed(String error) {
    return 'Det gick inte att spara: $error';
  }

  @override
  String identityNoStream(String uri) {
    return 'det gick inte att skriva till $uri';
  }

  @override
  String get identitySaveBusy => 'en sparning pågår redan';

  @override
  String get identityRestoreTitle => 'Återställa från en fil?';

  @override
  String get identityRestoreHow =>
      'Detta ersätter identiteten, kontakterna och meddelandena på den här telefonen.';

  @override
  String get identityChooseFile => 'Välj fil';

  @override
  String identityOpenFailed(String error) {
    return 'Det gick inte att öppna filen: $error';
  }

  @override
  String get identityPasswordTitle => 'Filens lösenord';

  @override
  String get identityRestoredPairAgain =>
      'Identiteten återställd. Parkoppla igen med dina noder.';

  @override
  String identityRefused(String reason) {
    return '$reason Inget ändrades.';
  }

  @override
  String get identityNotFile => 'Det här är ingen identitetsfil från bark.';

  @override
  String get identityOtherLayout =>
      'Filen kommer från en annan version av appen.';

  @override
  String get identityWrongPassword => 'Fel lösenord eller så är filen skadad.';

  @override
  String get identityPassword => 'Lösenord';

  @override
  String get identityPasswordAgain => 'Upprepa lösenordet';

  @override
  String get identityNotSame => 'Lösenorden är olika';

  @override
  String get identityAddMessages => 'Ta med mina meddelanden';

  @override
  String get identityOnePhone => 'Använd identiteten på en telefon i taget.';
}
