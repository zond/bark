// The emoji a person may react with: Unicode's list in its groups and
// order, with each one's everyday name and keywords to search by and to
// say it with, and the picker's first row, those the person uses most.

import 'package:emojis/emoji.dart';

import 'app_layer.dart';

/// The first row before the person has a history, and what fills it out
/// while they have used fewer: they say yes, love, funny, surprised,
/// sorry, and thanks or please. Six fit one row of 48 dp targets on the
/// narrowest phones, 360 dp.
const List<String> quickReactions = ['👍', '❤️', '😂', '😮', '😢', '🙏'];

/// One emoji the picker offers: the characters, its name, and the words
/// a search finds it by, its name and keywords.
typedef EmojiChoice = ({String emoji, String name, String words});

/// Unicode's five skin tone modifiers, U+1F3FB to U+1F3FF.
bool _toned(String emoji) =>
    emoji.runes.any((r) => r >= 0x1F3FB && r <= 0x1F3FF);

/// What each of Unicode's groups is called; the skin tones and hair
/// styles group is not offered, as it holds parts of emoji.
const Map<EmojiGroup, String> _groupNames = {
  EmojiGroup.smileysEmotion: 'Smileys & emotion',
  EmojiGroup.peopleBody: 'People & body',
  EmojiGroup.animalsNature: 'Animals & nature',
  EmojiGroup.foodDrink: 'Food & drink',
  EmojiGroup.travelPlaces: 'Travel & places',
  EmojiGroup.activities: 'Activities',
  EmojiGroup.objects: 'Objects',
  EmojiGroup.symbols: 'Symbols',
  EmojiGroup.flags: 'Flags',
};

/// Unicode's emoji, by its groups, in its order. Each person and hand is
/// offered once, without a skin tone. One past [maxReaction] bytes is
/// left out; Unicode 17.0 has none.
final List<(String, List<EmojiChoice>)> emojiGroups = [
  for (final MapEntry(key: g, value: title) in _groupNames.entries)
    (
      title,
      [
        for (final e in Emoji.byGroup(g))
          if (!_toned(e.char) && isReaction(e.char))
            (
              emoji: e.char,
              name: e.name,
              words: [e.name, ...e.keywords].join(' ').toLowerCase(),
            ),
      ],
    ),
];

final Map<String, String> _names = {
  for (final e in Emoji.all()) e.char: e.name,
};

/// What `emoji` is called, for a tooltip and a screen reader; the emoji
/// itself if Unicode's list does not have it.
String emojiName(String emoji) => _names[emoji] ?? emoji;

/// The emoji whose name and keywords hold every word of `query`, in the
/// list's order.
List<EmojiChoice> searchEmoji(String query) {
  final words = query.toLowerCase().split(' ').where((w) => w.isNotEmpty);
  return [
    for (final (_, g) in emojiGroups)
      for (final e in g)
        if (words.every(e.words.contains)) e,
  ];
}

/// The picker's first row: the six the person used most, of two used as
/// often the one used last first, then [quickReactions] they have not
/// used. `used`: each emoji's count, the one used last at the end
/// (`Session.reactionsUsed`).
List<String> frequentReactions(Map<String, int> used) {
  final order = used.keys.toList();
  final last = {for (final (i, e) in order.indexed) e: i};
  final ranked = order
    ..sort((a, b) {
      final c = used[b]!.compareTo(used[a]!);
      return c != 0 ? c : last[b]!.compareTo(last[a]!);
    });
  return [
    ...ranked,
    ...quickReactions.where((e) => !used.containsKey(e)),
  ].take(quickReactions.length).toList();
}
