// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Ukrainian (`uk`).
class AppLocalizationsUk extends AppLocalizations {
  AppLocalizationsUk([String locale = 'uk']) : super(locale);

  @override
  String get contacts => 'Контакти';

  @override
  String get myCard => 'Моя картка';

  @override
  String get archived => 'Архів';

  @override
  String get nearbyNodes => 'Вузли поблизу';

  @override
  String get settings => 'Налаштування';

  @override
  String get manual => 'Посібник';

  @override
  String get undo => 'Скасувати';

  @override
  String get conversationsNoBrowser => 'Немає браузера, щоб відкрити посібник';

  @override
  String get conversationsBackupTitle => 'Запишіть свої 12 резервних слів';

  @override
  String get conversationsBackupWhy =>
      'Якщо ви втратите цей телефон, вони єдиний шлях назад до ваших контактів.';

  @override
  String get conversationsNone =>
      'Розмов ще немає. Додайте контакт або покажіть свою картку.';

  @override
  String conversationsArchived(int count) {
    return '$count в архіві';
  }

  @override
  String get conversationsNew => 'Нова розмова';

  @override
  String conversationsWasArchived(String name) {
    return '$name: розмову заархівовано';
  }

  @override
  String conversationsYou(String text, String state) {
    return 'Ви: $text, $state';
  }

  @override
  String get startTitle => 'Початок';

  @override
  String get startDone => 'Зроблено';

  @override
  String get startToDo => 'Зробити';

  @override
  String get startName => 'Ваше ім\'я';

  @override
  String get startNameHow => 'Щоб люди знали, хто ви.';

  @override
  String get startNode => 'Вузол';

  @override
  String get startNodeHow => 'Знаходиться сам, коли він поруч.';

  @override
  String get startContact => 'Ваш перший контакт';

  @override
  String get startContactHow => 'Відскануйте їхню картку або покажіть їм свою.';

  @override
  String barHearsNoOne(String node) {
    return '$node нікого не чує після оновлення';
  }

  @override
  String get barHearsNoOneWhy =>
      'Нова прошивка могла зламати його радіо. Натисніть, щоб повернутися.';

  @override
  String barAppOut(int version) {
    return 'Вийшла версія застосунку $version';
  }

  @override
  String get barTapToInstall => 'Натисніть, щоб встановити.';

  @override
  String get barDownloading => 'Завантаження…';

  @override
  String barDownloadingPercent(int percent) {
    return 'Завантаження… $percent%';
  }

  @override
  String get barDownloaded => 'Завантажено. Натисніть, щоб встановити.';

  @override
  String get barDownloadFailed =>
      'Не вдалося завантажити. Натисніть, щоб спробувати знову.';

  @override
  String barFirmwareOut(int version) {
    return 'Вийшла прошивка вузла $version';
  }

  @override
  String get barTapToUpdate => 'Натисніть, щоб оновити вузол.';

  @override
  String get barNotThisVersion => 'Не для цієї версії';

  @override
  String get archivedNothing =>
      'В архіві нічого немає. Проведіть розмову чи контакт убік, щоб заархівувати.';

  @override
  String get archivedConversations => 'Розмови';

  @override
  String get archivedUnarchive => 'Повернути з архіву';

  @override
  String get cancel => 'Скасувати';

  @override
  String get save => 'Зберегти';

  @override
  String get notNow => 'Не зараз';

  @override
  String get timeJustNow => 'щойно';

  @override
  String timeMinutesAgo(int minutes) {
    return '$minutes хв тому';
  }

  @override
  String timeHoursAgo(int hours) {
    return '$hours год тому';
  }

  @override
  String timeDaysAgo(int days) {
    String _temp0 = intl.Intl.pluralLogic(
      days,
      locale: localeName,
      other: '$days дня тому',
      many: '$days днів тому',
      few: '$days дні тому',
      one: '$days день тому',
    );
    return '$_temp0';
  }

  @override
  String get timeNever => 'ніколи';

  @override
  String get timeToday => 'сьогодні';

  @override
  String get timeYesterday => 'вчора';

  @override
  String get sentQueued => 'чекає на вузол';

  @override
  String get sentWriting => 'передається вузлу';

  @override
  String get sentTaken => 'надіслано в мережу';

  @override
  String get sentBusyNow => 'мережа зайнята, надсилаємо знову';

  @override
  String sentBusyIn(String wait) {
    return 'мережа зайнята, знову надішлемо через $wait';
  }

  @override
  String get sentRefused => 'вузол відхилив';

  @override
  String sentPartsSent(int taken, int total) {
    return 'надіслано $taken з $total';
  }

  @override
  String sentPartProblem(int part, String state) {
    return 'частина $part: $state';
  }

  @override
  String get sentQueuedHelp =>
      'На цьому телефоні, доки не підключиться вузол, готовий його прийняти.';

  @override
  String get sentWritingHelp => 'Телефон передає його вузлу.';

  @override
  String get sentTakenHelp =>
      'Вузол отримав його й передає далі. Мережа більше нічого не повідомляє: вона не каже, коли воно дійде.';

  @override
  String get sentBusy => 'Мережа зайнята';

  @override
  String get sentBusyHelp =>
      'Мережа поки що повернула його. Застосунок сам надішле його знову, коли очікування мине.';

  @override
  String get sentRefusedHelp =>
      'Вузол не прийме його в такому вигляді: це помилка застосунку чи вузла. «Скасувати» прибирає його; «Налагодження» показує причину.';

  @override
  String get sentReaction => 'Їхня реакція';

  @override
  String get sentReactionHelp =>
      'Емодзі в куті повідомлення, якщо вони його вибрали.';

  @override
  String get receivedYourReaction => 'Ваша реакція';

  @override
  String get receivedYourReactionHelp =>
      'Натисніть і утримуйте повідомлення, щоб відреагувати емодзі або скопіювати його. Вони побачать реакцію, щойно вона дійде. Виберіть іншу, щоб змінити.';

  @override
  String get receivedFirst => 'Перше повідомлення';

  @override
  String get receivedFirstHelp =>
      'Перше повідомлення міг надіслати будь-хто з вашою карткою, тож не доведено, що це від них.';

  @override
  String get receivedMailbox => 'З вашої поштової скриньки';

  @override
  String get receivedMailboxHelp =>
      'Воно чекало на вашому домашньому вузлі й прийшло, коли ви забрали пошту.';

  @override
  String get receivedParts => 'Частини';

  @override
  String get receivedPartsHelp =>
      'Довге повідомлення надходить частинами, у будь-якому порядку. Курсив позначає частину, якої ще немає.';

  @override
  String get receivedTime => 'Час';

  @override
  String get receivedTimeHelp =>
      'Коли воно прийшло. Коли його написали, сказано в розділі «Це повідомлення».';

  @override
  String get commonThisMessage => 'Це повідомлення';

  @override
  String commonNotPaired(String reason) {
    return '$reason. Ви й далі тут гість.';
  }

  @override
  String commonPairingFailed(String error) {
    return 'Не вдалося створити пару: $error. Ви й далі тут гість.';
  }

  @override
  String commonGift(String name, String node) {
    return '$name пропонує вам дім на $node';
  }

  @override
  String get commonGiftTitle => 'Дім у подарунок';

  @override
  String get commonGiftHelp =>
      'Власник домашнього вузла додав вас туди. Якщо ви ним скористаєтеся, це буде ваш дім: він зберігає повідомлення для вас, поки вас немає, а ваша картка містить його адресу, тож ваші контакти отримають вашу нову картку.\n\nДім, який був у вас раніше, надішле вам вашу пошту, а потім забуде вас.';

  @override
  String get commonUseGift => 'Прийняти';

  @override
  String commonFailed(String error) {
    return 'Не вдалося: $error';
  }

  @override
  String get commonHomeOffer =>
      'Домашнього вузла ще немає. Зробити цей вузол вашим домом? Він зберігає повідомлення для вас, поки вас немає. Спершу створіть пару за кодом на його екрані.';

  @override
  String get commonSetHome => 'Зробити домом';

  @override
  String get commonBluetoothSettings => 'Налаштування Bluetooth';

  @override
  String commonBluetoothSettingsFailed(String error) {
    return 'Не вдалося відкрити налаштування Bluetooth: $error';
  }

  @override
  String get commonFetchQueued => 'Пошту заберемо, коли буде вузол';

  @override
  String commonFetchWaiting(String wait) {
    return 'Чекаємо відповіді вашого домашнього вузла, щонайбільше $wait';
  }

  @override
  String get commonFetch => 'Забрати мою пошту';

  @override
  String commonFetchFailed(String error) {
    return 'Не вдалося забрати пошту: $error';
  }

  @override
  String get commonCardNoHome => 'Ваша картка змінилася: немає дому';

  @override
  String get commonCardNewHome => 'Ваша картка змінилася: нова адреса дому';

  @override
  String commonCardOld(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Стару картку мають $count ваших контактів.',
      many: 'Стару картку мають $count ваших контактів.',
      few: 'Стару картку мають $count ваші контакти.',
      one: 'Стару картку має $count ваш контакт.',
    );
    return '$_temp0';
  }

  @override
  String get commonCardQueued => 'Вашу картку поставлено в чергу';

  @override
  String commonCardFailed(String error) {
    return 'Не вдалося надіслати картку: $error';
  }

  @override
  String get commonSendCard => 'Надіслати мою картку';

  @override
  String get commonMyName => 'Моє ім\'я';

  @override
  String commonAbout(String title) {
    return 'Довідка: $title';
  }

  @override
  String get commonWhyThisNode => 'Чому цей вузол';

  @override
  String get commonWhyBluetooth =>
      'bark надсилає повідомлення через Bluetooth на вузол поруч із вами. Далі запитає Android.';

  @override
  String get commonContinue => 'Продовжити';

  @override
  String get statusPublicNode => 'публічний вузол';

  @override
  String statusLooking(String node) {
    return 'Не підключено. Шукаємо $node; повідомлення чекають на телефоні';
  }

  @override
  String statusBluetoothOff(String node) {
    return 'Не підключено. Bluetooth вимкнено: увімкніть його, щоб знайти $node';
  }

  @override
  String get statusNotLooking =>
      'Не підключено. Повідомлення чекають на телефоні';

  @override
  String statusGuestFor30s(String node, String install) {
    return 'Підключено до $node як гість на 30 с$install';
  }

  @override
  String statusGuest(String node, String install) {
    return 'Підключено до $node як гість$install';
  }

  @override
  String statusAtHome(String node, String install) {
    return 'Підключено до вашого дому, $node$install';
  }

  @override
  String statusPaired(String node, String install) {
    return 'Підключено до спареного вузла $node$install';
  }

  @override
  String statusHomeAt(String node) {
    return 'Дім: $node';
  }

  @override
  String statusStarting(String wait) {
    return 'Запуск, $wait';
  }

  @override
  String get statusHearsNone =>
      'Ще не чує інших вузлів: повідомлення чекають тут';

  @override
  String statusHears(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: 'Чує $count вузла',
      many: 'Чує $count вузлів',
      few: 'Чує $count вузли',
      one: 'Чує $count вузол',
    );
    return '$_temp0';
  }

  @override
  String get signalUnknown => 'сигнал невідомий';

  @override
  String get signalStrong => 'сильний сигнал';

  @override
  String get signalFair => 'середній сигнал';

  @override
  String get signalWeak => 'слабкий сигнал';

  @override
  String connectionLostOne(String node) {
    return '$node не знає цього телефону: створіть із ним пару знову в Налаштуваннях.';
  }

  @override
  String connectionLostMany(String nodes) {
    return '$nodes не знають цього телефону: створіть із ними пару знову в Налаштуваннях.';
  }

  @override
  String connectionUnservedOne(String node) {
    return '$node поки не можна використати: його або цей застосунок треба оновити. Натисніть на нього в Налаштуваннях, щоб спробувати знову.';
  }

  @override
  String connectionUnservedMany(String nodes) {
    return '$nodes поки не можна використати: їх або цей застосунок треба оновити. Натисніть на один із них у Налаштуваннях, щоб спробувати знову.';
  }

  @override
  String get connectionAtHome =>
      'Ваш дім. Телефон лишається підключеним, поки вузол поруч, тож повідомлення приходять одразу, навіть із вимкненим екраном. Телефон завжди підключається спершу до вашого дому.';

  @override
  String get connectionPaired =>
      'Вузол, з яким ви спарені. Телефон лишається підключеним, поки вузол поруч.';

  @override
  String get connectionPairedNoHome =>
      'Без дому ваша картка вказує на цей вузол, поки ви ним користуєтеся.';

  @override
  String get connectionPairedHome =>
      'Повідомлення для вас чекають у вашому домі; «Забрати мою пошту» приносить їх сюди.';

  @override
  String get connectionMovesHome =>
      'Коли ваш дім поруч, телефон переходить до нього протягом хвилини.';

  @override
  String get connectionTappedGuest =>
      'Чужий публічний вузол, який ви вибрали. Він обслуговує одного гостя за раз і розриває зв\'язок гостя через 30 секунд; потім телефон сам вибирає вузол.';

  @override
  String get connectionGuestWeak =>
      'Ваші спарені вузли тут чути надто слабко, тож телефон користується цим публічним вузлом, доки добре не почує один із них.';

  @override
  String get connectionGuestAway =>
      'Жоден із ваших спарених вузлів не був поруч уже 30 секунд, тож телефон користується цим публічним вузлом, доки не почує один із них.';

  @override
  String get connectionGuestNoneNear =>
      'Жодного вашого вузла поруч немає, тож телефон користується цим публічним вузлом, чужим.';

  @override
  String get connectionGuestVisits =>
      'Як гість телефон підключається приблизно раз на хвилину й одразу, коли ви надсилаєте. Відповіді на повідомлення, надіслані звідси, приходять сюди.';

  @override
  String get connectionBluetoothOff =>
      'Bluetooth вимкнено. Повідомлення чекають на телефоні, доки ви його не ввімкнете й поруч не буде вузла.';

  @override
  String get connectionLookPublic =>
      'Немає вашого вузла, тож телефон кожні 15 секунд шукає публічний вузол, щоб користуватися ним як гість.';

  @override
  String get connectionLookPaired =>
      'Телефон кожні 15 секунд шукає ваші спарені вузли, спершу ваш дім. Якщо за 30 секунд жодного немає, він тим часом користується публічним вузлом як гість.';

  @override
  String get connectionNotLooking =>
      'Телефон не шукає: ви зупинили пошук, або йому ще не дозволено Bluetooth. Натисніть на вузол у Налаштуваннях, щоб підключитися.';

  @override
  String get notifyTextsArrive =>
      'Повідомлення приходять і з вимкненим екраном';

  @override
  String get notifyChecksMinute =>
      'Шукає нові повідомлення приблизно раз на хвилину';

  @override
  String get notifyOutInReach =>
      'Повідомлення з черги підуть, щойно він буде поруч';

  @override
  String get notifyOutBluetooth =>
      'Повідомлення з черги підуть, щойно ввімкнеться Bluetooth';

  @override
  String get notifyUnnamed => 'Без імені';

  @override
  String get notifyChannel => 'Повідомлення';

  @override
  String get notifyChannelAbout => 'Повідомлення від контакту';

  @override
  String get notifyYou => 'Ви';

  @override
  String get threadRemovedContact => 'Видалений контакт';

  @override
  String threadPartMissing(int part, int total) {
    return 'частини $part з $total ще немає';
  }

  @override
  String get factRole =>
      'Ретранслятор має зовнішнє живлення, наприклад USB, і слухає весь час, передаючи повідомлення для вузлів довкола: що більше ретрансляторів, то далі сягають повідомлення. Сонний вузол працює від батареї: щоб її вистачило, він спить і прокидається лише на короткі вікна, а надсилає й отримує через один ретранслятор поблизу, свій якір. Нерухомий ретранслятор, що втратив живлення, далі ретранслює від батареї, доки вона не почне сідати, а тоді стає сонним. Якщо утримувати кнопку вузла, він змінює роль до наступної зміни живлення.';

  @override
  String get factMoving =>
      'Вузол визначає це сам за своїм GPS: якщо понад хвилину його позиції віддалені від місця, де він стояв, вважається, що його носять, а після чотирьох годин на одному місці він знову нерухомий. Місце вузла, який носять, береться з його GPS, і він не зберігає нової пошти для інших, бо може скоро зникнути. Якщо задати його місце в Налаштуваннях застосунку, він одразу стане нерухомим.';

  @override
  String get factAnchor =>
      'Сонний вузол надсилає й отримує через один ретранслятор поблизу, свій якір. Якір зберігає пошту сонного вузла й передає її в коротке вікно після кожного його пробудження, а адреса сонного вузла — це місце його якоря. Сонний вузол бере за якір перший ретранслятор, який почує; поки якоря немає, він сам зберігає й передає повідомлення, що більше навантажує канал.';

  @override
  String get factPlace =>
      'Повідомлення знаходять вузол за його місцем: адреса — це позиція та ID вузла, і кожен ретранслятор передає повідомлення далі в бік цієї позиції. Місце береться з GPS вузла, якщо тільки хтось не задав його в Налаштуваннях застосунку; тоді діє задане, як для ретранслятора в приміщенні, звідки не видно неба. Вузол, що ще не має місця, не надсилає сигналу присутності, тож жодне повідомлення не знайде його, доки місце не з\'явиться.';

  @override
  String get factPower =>
      'USB означає зовнішнє живлення, від якого вузол може ретранслювати весь час. Від батареї значення визначається за напругою елемента, тож воно приблизне; на панелі самого цього вузла це сире значення в мілівольтах. Коли лишається близько п\'ятої частини, заряд вважається низьким, і ретранслятор на ній стає сонним, щоб зберегти решту.';

  @override
  String get factLink =>
      'Наскільки добре цей вузол чує інший: наскільки його сигнал вищий за радіошум тут. Нижче приблизно 4 дБ повне повідомлення часто губиться, коли сигнал згасає, тож зв\'язок крихкий і використовується, лише коли іншого не лишилося; від приблизно 7 дБ він добрий; між ними середній. Його виміряно в один бік: інший вузол може чути цей краще чи гірше.';

  @override
  String get factLastHeard =>
      'Кожен вузол час від часу надсилає короткий сигнал присутності, де каже, хто він і де. Ретранслятор надсилає його кожні 5–15 секунд, щоразу на наступному з 13 каналів, тож цей вузол чує його приблизно раз на одну-три хвилини; сонний вузол прокидається, щоб надіслати його, кожні дві з половиною – п\'ять хвилин. Ретранслятор, якого не чути шість з половиною хвилин, або сонний вузол, якого не чути десять, зникає зі списку.';

  @override
  String get factDistance =>
      'Пряма лінія від місця цього вузла до місця іншого, обчислена застосунком за двома позиціями, тож її показано, лише коли обидва мають місце. Дальність радіо залежить від висоти й від того, що стоїть між ними: ретранслятори на дахах чують один одного на відстані близько 3 км.';

  @override
  String get factKnown =>
      'Що вузол каже про своїх сусідів, трьома числами. Чутні: вузли, чий сигнал присутності він чує. Сильні: з них ті, яких чути так добре, що повне повідомлення проходить, навіть коли сигнал згасає, тож вони, ймовірно, теж його чують. Обмінялися: з них ті, що прийняли від нього повідомлення, а це доводить, що вони його чують.';

  @override
  String factGpsSignal(int floor) {
    return 'Наскільки сильно GPS вузла чує супутники, у дБ·Гц, за останнім вимірюванням. Нижче $floor вузол не бере з нього позицію; відкрите небо дає близько 40. Перенесіть вузол туди, де значення вище, або задайте його місце вручну.';
  }

  @override
  String get factVersion =>
      'Версія прошивки, яку виконує вузол. Нова версія поширюється сама: вузол, що отримав її з телефону або через USB, передає її ретрансляторам довкола частинами, лишаючи канали повідомленням. Вузол приймає лише новішу версію, підписану проєктом, і поки збирає її, цей рядок показує, яку саме і скільки її блоків він має.';

  @override
  String get factExchanged =>
      'Цей вузол передав йому повідомлення, і той його прийняв. Це доводить, що зв\'язок працює в обидва боки: той вузол не лише чутно, він і сам чує цей.';

  @override
  String get factDemoted =>
      'Він не відповів на останні три спроби цього вузла передати йому повідомлення, тож, можливо, більше не чує цей вузол. Доки він знову не відповість, або протягом шести з половиною хвилин, його пробують лише після всіх інших сусідів із достатньо добрим зв\'язком.';

  @override
  String get factAvoided =>
      'Він відхилив повідомлення, попросивши зачекати, або не відповів на нього. Якийсь час йому нічого не надсилають; повідомлення тим часом ідуть в обхід і чекають на нього лише там, де іншого шляху немає.';

  @override
  String get factPaired =>
      'Цей телефон спарений із цим вузлом: вузол надсилає ваші повідомлення в мережу й зберігає ті, що для вас, доки застосунок їх не прочитає. Лише спарений телефон бачить сусідів вузла на цій карті.';
}
