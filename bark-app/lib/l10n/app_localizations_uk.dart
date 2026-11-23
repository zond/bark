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
}
