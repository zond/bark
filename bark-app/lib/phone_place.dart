// Where the phone is, from Android's location service, asked only when
// the person asks for it: to set a node's place, or to see themselves on
// the map. The position is used where it is asked for and never sent or
// kept.

import 'package:geolocator_platform_interface/geolocator_platform_interface.dart'
    show GeolocatorPlatform, LocationPermission;

import 'l10n.dart';
import 'link.dart';

/// No position: why, in a few words, which a screen says.
class NoPlace implements Exception, Spoken {
  final String Function(AppLocalizations) _why;
  const NoPlace(this._why);
  @override
  String words(AppLocalizations w) => _why(w);
  @override
  String toString() => words(english);
}

abstract class PhonePlace {
  /// The phone's position now, after asking for the location permission
  /// if it is not granted; throws [NoPlace] without one.
  Future<Position> here();
}

/// The geolocator plugin's, which the app has for the node's place
/// already: the foreground location Android grants while the app is in
/// use.
class AndroidPlace implements PhonePlace {
  const AndroidPlace();

  @override
  Future<Position> here() async {
    final g = GeolocatorPlatform.instance;
    try {
      var p = await g.checkPermission();
      if (p == LocationPermission.denied) p = await g.requestPermission();
      if (p == LocationPermission.denied ||
          p == LocationPermission.deniedForever) {
        throw NoPlace((w) => w.placeDenied);
      }
      final here = await g.getCurrentPosition();
      return Position.fromDegrees(here.latitude, here.longitude);
    } on NoPlace {
      rethrow;
    } catch (e) {
      // The plugin's reason is the phone's own words, in English.
      throw NoPlace((w) => w.placeNone('$e'));
    }
  }
}
