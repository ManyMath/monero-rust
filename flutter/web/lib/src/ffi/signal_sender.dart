import 'dart:async';

abstract class SignalSender {
  void sendSignal(String signalName, Map<String, dynamic> data);
  Stream<Map<String, dynamic>> onRawSignal(String typeName);
}

SignalSender? _sender;
SignalSender get signalSender => _sender!;
void setSignalSender(SignalSender sender) => _sender = sender;
