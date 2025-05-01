import 'package:flutter_test/flutter_test.dart';
import 'package:fake_async/fake_async.dart';
import 'package:monero_extension/services/wallet_polling_service.dart';

void main() {
  // Use a fresh instance for each test to avoid singleton state leaking
  late WalletPollingService service;

  setUp(() {
    service = WalletPollingService();
    service.stopPolling(); // ensure clean state
  });

  tearDown(() {
    service.stopPolling();
  });

  group('WalletPollingService', () {
    test('countdowns initialize to correct values on startPolling', () {
      fakeAsync((async) {
        service.startPolling(
          onBlockRefresh: () {},
          onMempoolPoll: () {},
        );

        expect(service.blockRefreshCountdown.value, 90);
        expect(service.mempoolCountdown.value, 45);

        service.stopPolling();
      });
    });

    test('countdowns decrement each second', () {
      fakeAsync((async) {
        service.startPolling(
          onBlockRefresh: () {},
          onMempoolPoll: () {},
        );

        async.elapse(const Duration(seconds: 3));

        expect(service.blockRefreshCountdown.value, 87);
        expect(service.mempoolCountdown.value, 42);

        service.stopPolling();
      });
    });

    test('stopPolling resets countdowns to zero', () {
      fakeAsync((async) {
        service.startPolling(
          onBlockRefresh: () {},
          onMempoolPoll: () {},
        );

        async.elapse(const Duration(seconds: 5));
        service.stopPolling();

        expect(service.blockRefreshCountdown.value, 0);
        expect(service.mempoolCountdown.value, 0);
      });
    });

    test('block refresh callback fires at 90s interval', () {
      fakeAsync((async) {
        int blockRefreshCount = 0;

        service.startPolling(
          onBlockRefresh: () => blockRefreshCount++,
          onMempoolPoll: () {},
        );

        async.elapse(const Duration(seconds: 89));
        expect(blockRefreshCount, 0);

        async.elapse(const Duration(seconds: 1)); // 90s total
        expect(blockRefreshCount, 1);
        expect(service.blockRefreshCountdown.value, 90); // reset

        async.elapse(const Duration(seconds: 90)); // 180s total
        expect(blockRefreshCount, 2);

        service.stopPolling();
      });
    });

    test('mempool callback fires at 45s offset then every 90s', () {
      fakeAsync((async) {
        int mempoolPollCount = 0;

        service.startPolling(
          onBlockRefresh: () {},
          onMempoolPoll: () => mempoolPollCount++,
        );

        async.elapse(const Duration(seconds: 44));
        expect(mempoolPollCount, 0);

        async.elapse(const Duration(seconds: 1)); // 45s total
        expect(mempoolPollCount, 1);
        expect(service.mempoolCountdown.value, 90); // reset to 90s interval

        async.elapse(const Duration(seconds: 90)); // 135s total
        expect(mempoolPollCount, 2);

        service.stopPolling();
      });
    });

    test('isPolling reflects timer state', () {
      fakeAsync((async) {
        expect(service.isPolling, false);

        service.startPolling(
          onBlockRefresh: () {},
          onMempoolPoll: () {},
        );
        expect(service.isPolling, true);

        service.stopPolling();
        expect(service.isPolling, false);
      });
    });

    test('ValueNotifier notifies listeners on countdown tick', () {
      fakeAsync((async) {
        final values = <int>[];
        service.blockRefreshCountdown.addListener(() {
          values.add(service.blockRefreshCountdown.value);
        });

        service.startPolling(
          onBlockRefresh: () {},
          onMempoolPoll: () {},
        );

        // Initial value set triggers one notification (90)
        expect(values.last, 90);

        async.elapse(const Duration(seconds: 3));

        // Should have ticked down: 89, 88, 87
        expect(values.contains(89), true);
        expect(values.contains(88), true);
        expect(values.contains(87), true);

        service.stopPolling();
      });
    });

    test('startPolling cancels previous timers', () {
      fakeAsync((async) {
        int count1 = 0;
        int count2 = 0;

        service.startPolling(
          onBlockRefresh: () => count1++,
          onMempoolPoll: () {},
        );

        async.elapse(const Duration(seconds: 10));

        // Start again with different callback
        service.startPolling(
          onBlockRefresh: () => count2++,
          onMempoolPoll: () {},
        );

        async.elapse(const Duration(seconds: 90));

        expect(count1, 0); // old callback should not fire
        expect(count2, 1);

        service.stopPolling();
      });
    });
  });
}
