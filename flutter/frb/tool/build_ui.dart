#!/usr/bin/env dart
// ignore_for_file: avoid_print

import 'dart:io';
import 'package:archive/archive_io.dart';
import 'package:path/path.dart' as path;

/// Build script for browser extension UI only (skips Rust/WASM rebuild)
void main() async {
  print('\nBuilding extension UI...');

  final builder = UIBuilder();
  try {
    await builder.build();
  } catch (e) {
    print('\nError: $e');
    exit(1);
  }
}

class UIBuilder {
  late final String projectRoot;
  late final String buildDir;
  late final String extensionDir;
  late final String webBuildDir;

  UIBuilder() {
    final scriptDir = path.dirname(Platform.script.toFilePath());
    projectRoot = path.dirname(scriptDir);
    buildDir = path.join(projectRoot, 'build');
    extensionDir = path.join(buildDir, 'extension');
    webBuildDir = path.join(buildDir, 'web');
  }

  Future<void> build() async {
    await _checkPrerequisites();
    await _buildFlutterWeb();
    await _createExtensionDirectory();
    await _copyFlutterBuild();
    await _copyManifest();
    await _patchForExtension();
    await _verifyBuild();
    await _createPackage();
    _printSuccess();
  }

  Future<void> _checkPrerequisites() async {
    print('\nChecking prerequisites...');

    final flutterResult = await _runCommand('flutter', ['--version'], silent: true);
    if (!flutterResult) {
      throw Exception('Flutter is not installed or not in PATH');
    }

    print('Prerequisites OK');
  }

  Future<void> _buildFlutterWeb() async {
    print('\nBuilding Flutter web...');
    final success = await _runCommand(
      'flutter',
      ['build', 'web', '--csp', '--no-web-resources-cdn', '--release'],
      workingDir: projectRoot,
    );
    if (!success) {
      throw Exception('Failed to build Flutter web app');
    }
  }

  Future<void> _createExtensionDirectory() async {
    print('\nCreating extension directory...');

    final extensionDirectory = Directory(extensionDir);
    if (await extensionDirectory.exists()) {
      await extensionDirectory.delete(recursive: true);
    }

    await extensionDirectory.create(recursive: true);
  }

  Future<void> _copyFlutterBuild() async {
    print('Copying build output...');

    final webBuild = Directory(webBuildDir);
    if (!await webBuild.exists()) {
      throw Exception('build/web directory not found. Did Flutter build succeed?');
    }

    await _copyDirectory(webBuild, Directory(extensionDir));
  }

  Future<void> _copyManifest() async {
    print('Copying manifest...');

    final manifestSource = path.join(projectRoot, 'extension', 'manifest.json');
    final manifestDest = path.join(extensionDir, 'manifest.json');

    final sourceFile = File(manifestSource);
    if (!await sourceFile.exists()) {
      throw Exception('extension/manifest.json not found');
    }

    await sourceFile.copy(manifestDest);

    final backgroundSource = path.join(projectRoot, 'extension', 'background.js');
    final backgroundDest = path.join(extensionDir, 'background.js');

    final backgroundFile = File(backgroundSource);
    if (await backgroundFile.exists()) {
      await backgroundFile.copy(backgroundDest);
    } else {
      print('  Warning: extension/background.js not found');
    }
  }

  Future<void> _patchForExtension() async {
    print('Patching for extension...');

    await _removeServiceWorker();
    await _patchFlutterBootstrap();
    await _copyDisableServiceWorker();
    await _patchIndexHtml();
  }

  Future<void> _removeServiceWorker() async {
    final serviceWorkerPath = path.join(extensionDir, 'flutter_service_worker.js');
    final serviceWorkerFile = File(serviceWorkerPath);

    if (await serviceWorkerFile.exists()) {
      await serviceWorkerFile.delete();
    }
  }

  Future<void> _patchFlutterBootstrap() async {
    final bootstrapPath = path.join(extensionDir, 'flutter_bootstrap.js');
    final bootstrapFile = File(bootstrapPath);

    if (!await bootstrapFile.exists()) {
      print('  Warning: flutter_bootstrap.js not found');
      return;
    }

    String content = await bootstrapFile.readAsString();

    // Remove serviceWorkerSettings from the load call
    // Matches: serviceWorkerSettings: { ... }
    final swPattern = RegExp(r',?\s*serviceWorkerSettings:\s*\{[^}]*\}');
    content = content.replaceAll(swPattern, '');

    await bootstrapFile.writeAsString(content);
  }

  Future<void> _copyDisableServiceWorker() async {
    final sourcePath = path.join(projectRoot, 'extension', 'disable_service_worker.js');
    final destPath = path.join(extensionDir, 'disable_service_worker.js');

    final sourceFile = File(sourcePath);
    if (await sourceFile.exists()) {
      await sourceFile.copy(destPath);
    } else {
      print('  Warning: disable_service_worker.js not found at: $sourcePath');
    }

    final bridgeSource = path.join(projectRoot, 'web', 'extension_bridge.js');
    final bridgeDest = path.join(extensionDir, 'extension_bridge.js');

    final bridgeFile = File(bridgeSource);
    if (await bridgeFile.exists()) {
      await bridgeFile.copy(bridgeDest);
    } else {
      print('  Warning: extension_bridge.js not found at: $bridgeSource');
    }
  }

  Future<void> _patchIndexHtml() async {
    final indexPath = path.join(extensionDir, 'index.html');
    final indexFile = File(indexPath);

    if (!await indexFile.exists()) {
      print('  Warning: index.html not found');
      return;
    }

    String content = await indexFile.readAsString();

    content = content.replaceAll('<base href="/">', '<base href="./">');

    const style = '''
  <style>
    html, body {
      width: 100%;
      height: 100%;
      margin: 0;
      padding: 0;
    }
    #loading {
      display: flex;
      justify-content: center;
      align-items: center;
      height: 100vh;
      font-family: sans-serif;
    }
  </style>
</head>''';

    content = content.replaceAll('</head>', style);

    const bodyScripts = '''<body>
  <div id="loading">Loading Monero Wallet...</div>

  <script src="extension_bridge.js"></script>
  <script src="disable_service_worker.js"></script>
  <script src="flutter_bootstrap.js"></script>
</body>''';

    final bodyPattern = RegExp(r'<body>\s*<script src="flutter_bootstrap\.js"( async)?></script>\s*</body>', dotAll: true);
    content = content.replaceAll(bodyPattern, bodyScripts);

    await indexFile.writeAsString(content);
  }

  Future<void> _verifyBuild() async {
    print('\nVerifying build...');

    final requiredFiles = [
      'manifest.json',
      'index.html',
      'flutter.js',
    ];

    for (final fileName in requiredFiles) {
      final filePath = path.join(extensionDir, fileName);
      final file = File(filePath);

      if (!await file.exists()) {
        throw Exception('Required file missing: $fileName');
      }
    }
  }

  Future<void> _createPackage() async {
    print('Creating zip...');

    final zipPath = path.join(buildDir, 'monero-extension.zip');
    final encoder = ZipFileEncoder();

    encoder.create(zipPath);
    await encoder.addDirectory(Directory(extensionDir), includeDirName: false);
    encoder.close();
  }

  void _printSuccess() {
    print('\nUI built successfully!');
    print('  Location: build/extension/');
    print('  Package: build/monero-extension.zip');
    print('\nFor Rust changes, run: dart tool/build_extension.dart');
    print('\nLoad in Chrome:');
    print('  chrome://extensions -> Developer mode -> Load unpacked -> build/extension/\n');
  }

  Future<bool> _runCommand(
    String command,
    List<String> args, {
    String? workingDir,
    bool silent = false,
  }) async {
    try {
      final result = await Process.run(
        command,
        args,
        workingDirectory: workingDir ?? projectRoot,
        runInShell: true,
      );

      if (!silent) {
        if (result.stdout.toString().isNotEmpty) {
          stdout.write(result.stdout);
        }
        if (result.stderr.toString().isNotEmpty) {
          stderr.write(result.stderr);
        }
      }

      return result.exitCode == 0;
    } catch (e) {
      if (!silent) {
        print('Failed to run command: $command ${args.join(' ')}');
        print('Error: $e');
      }
      return false;
    }
  }

  Future<void> _copyDirectory(Directory source, Directory destination) async {
    await destination.create(recursive: true);

    await for (final entity in source.list(recursive: false)) {
      if (entity is Directory) {
        final newDirectory = Directory(path.join(destination.path, path.basename(entity.path)));
        await _copyDirectory(entity, newDirectory);
      } else if (entity is File) {
        final newFile = File(path.join(destination.path, path.basename(entity.path)));
        await entity.copy(newFile.path);
      }
    }
  }
}
