// The phone's position for the tests: a fixed one, or none, counting the
// times it was asked.

import 'package:bark_app/link.dart';
import 'package:bark_app/phone_place.dart';

class FakePlace implements PhonePlace {
  /// Where the phone is; null for no permission.
  final Position? at;
  FakePlace(this.at);

  int asked = 0;

  @override
  Future<Position> here() async {
    asked++;
    return at ?? (throw NoPlace((w) => w.placeDenied));
  }
}

/// A phone whose position is never asked for in the test.
class NoPhonePlace implements PhonePlace {
  const NoPhonePlace();
  @override
  Future<Position> here() => throw StateError('the position was asked for');
}
