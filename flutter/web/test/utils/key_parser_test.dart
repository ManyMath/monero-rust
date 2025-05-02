import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/utils/key_parser.dart';

void main() {
  group('KeyParser.parse', () {
    group('empty and whitespace-only input', () {
      test('empty string returns invalid with "Input is empty"', () {
        final result = KeyParser.parse('');
        expect(result.isValid, false);
        expect(result.error, 'Input is empty');
        expect(result.normalizedInput, isNull);
      });

      test('single space returns invalid with "Input is empty"', () {
        final result = KeyParser.parse(' ');
        expect(result.isValid, false);
        expect(result.error, 'Input is empty');
      });

      test('multiple spaces returns invalid with "Input is empty"', () {
        final result = KeyParser.parse('     ');
        expect(result.isValid, false);
        expect(result.error, 'Input is empty');
      });

      test('tab-only input returns invalid with "Input is empty"', () {
        final result = KeyParser.parse('\t\t');
        expect(result.isValid, false);
        expect(result.error, 'Input is empty');
      });

      test('newline-only input returns invalid with "Input is empty"', () {
        final result = KeyParser.parse('\n\n');
        expect(result.isValid, false);
        expect(result.error, 'Input is empty');
      });

      test('mixed whitespace returns invalid with "Input is empty"', () {
        final result = KeyParser.parse(' \t \n ');
        expect(result.isValid, false);
        expect(result.error, 'Input is empty');
      });
    });

    group('valid 16-word polyseed', () {
      test('accepts exactly 16 words', () {
        final seed = List.generate(16, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse(seed);
        expect(result.isValid, true);
        expect(result.error, isNull);
        expect(result.normalizedInput, seed);
      });

      test('normalizedInput matches expected format', () {
        final seed = 'alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa';
        final result = KeyParser.parse(seed);
        expect(result.isValid, true);
        expect(result.normalizedInput, seed);
      });
    });

    group('valid 25-word classic seed', () {
      test('accepts exactly 25 words', () {
        final seed = List.generate(25, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse(seed);
        expect(result.isValid, true);
        expect(result.error, isNull);
        expect(result.normalizedInput, seed);
      });

      test('normalizedInput matches expected format', () {
        final seed = 'one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive';
        final result = KeyParser.parse(seed);
        expect(result.isValid, true);
        expect(result.normalizedInput, seed);
      });
    });

    group('wrong word count', () {
      test('single word returns invalid', () {
        final result = KeyParser.parse('hello');
        expect(result.isValid, false);
        expect(result.error, 'Expected 16 or 25 words, got 1');
      });

      test('15 words returns invalid', () {
        final seed = List.generate(15, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse(seed);
        expect(result.isValid, false);
        expect(result.error, 'Expected 16 or 25 words, got 15');
      });

      test('17 words returns invalid', () {
        final seed = List.generate(17, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse(seed);
        expect(result.isValid, false);
        expect(result.error, 'Expected 16 or 25 words, got 17');
      });

      test('24 words returns invalid', () {
        final seed = List.generate(24, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse(seed);
        expect(result.isValid, false);
        expect(result.error, 'Expected 16 or 25 words, got 24');
      });

      test('26 words returns invalid', () {
        final seed = List.generate(26, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse(seed);
        expect(result.isValid, false);
        expect(result.error, 'Expected 16 or 25 words, got 26');
      });

      test('100 words returns invalid', () {
        final seed = List.generate(100, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse(seed);
        expect(result.isValid, false);
        expect(result.error, 'Expected 16 or 25 words, got 100');
      });
    });

    group('whitespace normalization', () {
      test('leading whitespace is trimmed', () {
        final seed = List.generate(16, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse('   $seed');
        expect(result.isValid, true);
        expect(result.normalizedInput, seed);
      });

      test('trailing whitespace is trimmed', () {
        final seed = List.generate(16, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse('$seed   ');
        expect(result.isValid, true);
        expect(result.normalizedInput, seed);
      });

      test('leading and trailing whitespace is trimmed', () {
        final seed = List.generate(25, (i) => 'word${i + 1}').join(' ');
        final result = KeyParser.parse('  $seed  ');
        expect(result.isValid, true);
        expect(result.normalizedInput, seed);
      });

      test('multiple spaces between words are collapsed to single space', () {
        final words = List.generate(16, (i) => 'word${i + 1}');
        final input = words.join('   ');
        final expected = words.join(' ');
        final result = KeyParser.parse(input);
        expect(result.isValid, true);
        expect(result.normalizedInput, expected);
      });

      test('tabs between words are normalized to single space', () {
        final words = List.generate(16, (i) => 'word${i + 1}');
        final input = words.join('\t');
        final expected = words.join(' ');
        final result = KeyParser.parse(input);
        expect(result.isValid, true);
        expect(result.normalizedInput, expected);
      });

      test('newlines between words are normalized to single space', () {
        final words = List.generate(25, (i) => 'word${i + 1}');
        final input = words.join('\n');
        final expected = words.join(' ');
        final result = KeyParser.parse(input);
        expect(result.isValid, true);
        expect(result.normalizedInput, expected);
      });

      test('mixed whitespace (spaces, tabs, newlines) between words normalizes correctly', () {
        final words = List.generate(16, (i) => 'word${i + 1}');
        final input = words.join(' \t\n ');
        final expected = words.join(' ');
        final result = KeyParser.parse(input);
        expect(result.isValid, true);
        expect(result.normalizedInput, expected);
      });
    });

    group('KeyParseResult constructors', () {
      test('valid result has isValid=true, normalizedInput set, error=null', () {
        final result = KeyParseResult.valid('some normalized input');
        expect(result.isValid, true);
        expect(result.normalizedInput, 'some normalized input');
        expect(result.error, isNull);
      });

      test('invalid result has isValid=false, normalizedInput=null, error set', () {
        final result = KeyParseResult.invalid('some error message');
        expect(result.isValid, false);
        expect(result.normalizedInput, isNull);
        expect(result.error, 'some error message');
      });
    });
  });
}
