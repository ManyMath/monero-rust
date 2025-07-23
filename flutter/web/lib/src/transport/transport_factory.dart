import 'hub_transport.dart';

export 'hub_transport.dart';
export 'hub_signal_ids.dart';

// Conditional import: selects web or native transport based on platform.
import 'transport_web.dart' if (dart.library.ffi) 'transport_native.dart'
    as platform;

/// Create a [HubTransport] appropriate for the current platform.
///
/// On web: returns [HubWebTransport] (WorkerBridge-based).
/// On native: returns [HubNativeTransport] (dart:ffi-based).
HubTransport createHubTransport() => platform.createHubTransport();
