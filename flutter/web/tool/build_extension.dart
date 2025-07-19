#!/usr/bin/env dart
// ignore_for_file: avoid_print

import 'dart:io';
import 'package:archive/archive_io.dart';
import 'package:path/path.dart' as path;

/// Build script for browser extension
void main() async {
  print('\nBuilding extension...');

  final builder = ExtensionBuilder();
  try {
    await builder.build();
  } catch (e) {
    print('\nError: $e');
    exit(1);
  }
}

class ExtensionBuilder {
  late final String projectRoot;
  late final String buildDir;
  late final String extensionDir;
  late final String webBuildDir;
  late final String wasmCrateDir;

  ExtensionBuilder() {
    final scriptDir = path.dirname(Platform.script.toFilePath());
    projectRoot = path.dirname(scriptDir);
    buildDir = path.join(projectRoot, 'build');
    extensionDir = path.join(buildDir, 'extension');
    webBuildDir = path.join(buildDir, 'web');
    wasmCrateDir = path.join(projectRoot, 'native', 'monero_wasm');
  }

  Future<void> build() async {
    await _checkPrerequisites();
    await _buildWasm();
    await _buildFlutterWeb();
    await _createExtensionDirectory();
    await _copyFlutterBuild();
    await _copyWasmPkg();
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

    final wasmPackResult = await _runCommand('wasm-pack', ['--version'], silent: true);
    if (!wasmPackResult) {
      throw Exception('wasm-pack is not installed. Install with: cargo install wasm-pack');
    }

    print('Prerequisites OK');
  }

  Future<void> _buildWasm() async {
    print('\nBuilding WASM with wasm-pack...');
    final success = await _runCommand(
      'wasm-pack',
      ['build', '--target', 'web', '--release', '--out-dir', path.join(projectRoot, 'build', 'pkg')],
      workingDir: wasmCrateDir,
    );
    if (!success) {
      throw Exception('Failed to build WASM modules');
    }
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

  Future<void> _copyWasmPkg() async {
    print('Copying WASM package...');

    final pkgSource = Directory(path.join(buildDir, 'pkg'));
    if (!await pkgSource.exists()) {
      throw Exception('build/pkg directory not found. Did wasm-pack build succeed?');
    }

    final pkgDest = Directory(path.join(extensionDir, 'pkg'));
    await pkgDest.create(recursive: true);

    // Copy the essential wasm-pack artifacts
    for (final name in ['monero_wasm_bg.wasm', 'monero_wasm.js']) {
      final src = File(path.join(pkgSource.path, name));
      if (await src.exists()) {
        await src.copy(path.join(pkgDest.path, name));
      } else {
        throw Exception('Required WASM artifact missing: $name');
      }
    }
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

    // Copy background.js for full page mode
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

  Future<void> _copyDisableServiceWorker() async {
    final sourcePath = path.join(projectRoot, 'extension', 'disable_service_worker.js');
    final destPath = path.join(extensionDir, 'disable_service_worker.js');

    final sourceFile = File(sourcePath);
    if (await sourceFile.exists()) {
      await sourceFile.copy(destPath);
    } else {
      print('  Warning: disable_service_worker.js not found at: $sourcePath');
    }

    // Copy extension_bridge.js
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

    // Fix base href for extension (must be relative, not absolute)
    content = content.replaceAll('<base href="/">', '<base href="./">');

    // Add CSS for extension full page mode
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

    // Replace body: load wasm-bindgen glue via external module script
    // (inline scripts are blocked by extension CSP).
    const bodyScripts = '''<body>
  <div id="loading">Loading Monero Wallet...</div>

  <script src="extension_bridge.js"></script>
  <script src="disable_service_worker.js"></script>
  <script src="wasm_loader.js" type="module"></script>
</body>''';

    final bodyPattern = RegExp(
      r'<body>\s*<script src="flutter_bootstrap\.js"( async)?></script>\s*</body>',
      dotAll: true,
    );
    content = content.replaceAll(bodyPattern, bodyScripts);

    await indexFile.writeAsString(content);

    // Create the external wasm_loader.js module
    await _createWasmLoader();
  }

  Future<void> _createWasmLoader() async {
    final loaderPath = path.join(extensionDir, 'wasm_loader.js');
    const loaderContent = '''import init, * as wasmBindings from './pkg/monero_wasm.js';
// Spread into a plain object — ES module namespace objects are frozen,
// so Dart @JS() annotations can't reliably access properties on them.
globalThis.wasm_bindgen = { ...wasmBindings, default: init };
// Start Flutter after wasm_bindgen is on the global scope
const s = document.createElement('script');
s.src = 'flutter_bootstrap.js';
document.body.appendChild(s);
''';
    await File(loaderPath).writeAsString(loaderContent);
  }

  Future<void> _verifyBuild() async {
    print('\nVerifying build...');

    final requiredFiles = [
      'manifest.json',
      'index.html',
      'flutter.js',
      'pkg/monero_wasm_bg.wasm',
      'pkg/monero_wasm.js',
      'wasm_loader.js',
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
    print('\nExtension built successfully!');
    print('  Location: build/extension/');
    print('  Package: build/monero-extension.zip');
    print('\nLoad in Chrome:');
    print('  chrome://extensions -> Developer mode -> Load unpacked -> build/extension/');
    print('  See docs/extension_testing.md for more\n');
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
