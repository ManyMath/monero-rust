/// Minimal structured logger for the Dart side.
///
/// Provides leveled, timestamped log output to the developer console.
/// Usage:
///   Log.info('SignalHub', 'Received KeysDerivedResponse');
///   Log.debug('WorkerBridge', 'Sending signal: DeriveKeysRequest');
library;

enum LogLevel {
  debug,
  info,
  warn,
  error,
}

class Log {
  /// Minimum level that will be printed. Change at runtime if needed.
  static LogLevel minLevel = LogLevel.debug;

  static void debug(String tag, String message) =>
      _log(LogLevel.debug, tag, message);

  static void info(String tag, String message) =>
      _log(LogLevel.info, tag, message);

  static void warn(String tag, String message) =>
      _log(LogLevel.warn, tag, message);

  static void error(String tag, String message) =>
      _log(LogLevel.error, tag, message);

  static void _log(LogLevel level, String tag, String message) {
    if (level.index < minLevel.index) return;

    final now = DateTime.now();
    final ts =
        '${now.hour.toString().padLeft(2, '0')}:'
        '${now.minute.toString().padLeft(2, '0')}:'
        '${now.second.toString().padLeft(2, '0')}.'
        '${now.millisecond.toString().padLeft(3, '0')}';
    final label = level.name.toUpperCase();

    // ignore: avoid_print
    print('$ts [$label] $tag: $message');
  }
}
