// What the person's scan in Settings found, and whether it runs.

part of '../app.dart';

mixin _Scan on _AppCore {
  /// The nodes the person's scan found, by device, and whether it runs.
  final Map<String, FoundNode> found = {};
  @override
  bool scanning = false;

  /// The last scan found nothing: it was stopped with none found, or has
  /// run [seenWait], ten of a node's advertising intervals, without one.
  bool get noneFound => found.isEmpty && (_scanEnded || _scanLong);
  bool _scanEnded = false;
  bool _scanLong = false;
  Timer? _scanTimer;

  /// Scans for nodes, after asking for Bluetooth if it is not allowed
  /// yet: the person's tap is where the app asks, and the search runs
  /// from then on.
  Future<void> scan() async {
    if (!bluetoothAllowed) {
      try {
        await _requestBluetooth();
      } finally {
        await checkBluetooth();
      }
    }
    found.clear();
    scanning = true;
    _scanEnded = false;
    _scanLong = false;
    _scanTimer?.cancel();
    _scanTimer = Timer(seenWait, () {
      _scanLong = scanning;
      notifyListeners();
    });
    notifyListeners();
    await _searchStopped();
    try {
      await startScan((n) {
        found[n.deviceId] = n;
        _heard([n]);
        unawaited(_forgetBonds([n]));
        notifyListeners();
      });
    } catch (e) {
      scanning = false;
      _scanTimer?.cancel();
      notifyListeners();
      rethrow;
    }
  }

  @override
  Future<void> stopScan() async {
    await stopBleScan();
    _scanTimer?.cancel();
    _scanEnded = scanning || _scanEnded;
    scanning = false;
    notifyListeners();
  }
}
