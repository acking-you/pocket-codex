import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:record/record.dart';

/// Where a dictation is.
enum DictationPhase {
  /// Not recording.
  idle,

  /// Asking for the microphone and opening it.
  starting,

  /// Recording; the level moves the waveform.
  recording,

  /// Recording stopped; the host is turning it into text.
  transcribing,
}

/// One finished recording, as the recorder hands it back.
typedef DictationAudio = ({Uint8List bytes, String mime, String fileName});

/// The microphone half of dictation, behind an interface so the controller
/// can be tested without a device.
abstract interface class DictationRecorder {
  /// Whether recording is allowed (asks the OS on first use).
  Future<bool> hasPermission();

  /// Start recording.
  Future<void> start();

  /// The current input level, 0..1.
  Future<double> level();

  /// Stop and return what was recorded (null if nothing was).
  Future<DictationAudio?> stop();

  /// Stop and throw the recording away.
  Future<void> cancel();

  /// Release the microphone for good.
  Future<void> dispose();
}

/// [DictationRecorder] over the `record` plugin: mono AAC where the platform
/// encodes it (small uploads), else 16 kHz PCM WAV, to a temp file.
class PlatformDictationRecorder implements DictationRecorder {
  final AudioRecorder _recorder = AudioRecorder();
  String? _path;
  late String _mime;

  @override
  Future<bool> hasPermission() => _recorder.hasPermission();

  @override
  Future<void> start() async {
    final aac = await _recorder.isEncoderSupported(AudioEncoder.aacLc);
    final ext = aac ? 'm4a' : 'wav';
    _mime = aac ? 'audio/mp4' : 'audio/wav';
    final dir = await getTemporaryDirectory();
    final path =
        '${dir.path}${Platform.pathSeparator}'
        'pcx-dictation-${DateTime.now().microsecondsSinceEpoch}.$ext';
    _path = path;
    await _recorder.start(
      RecordConfig(
        encoder: aac ? AudioEncoder.aacLc : AudioEncoder.wav,
        // Speech needs neither stereo nor CD rate; mono 16 kHz is what
        // recognisers resample to anyway, and keeps a long take small.
        numChannels: 1,
        sampleRate: 16000,
        bitRate: 48000,
        autoGain: true,
        noiseSuppress: true,
        echoCancel: true,
      ),
      path: path,
    );
  }

  @override
  Future<double> level() async {
    // dBFS, 0 at full scale and about -160 for silence. Speech sits around
    // -45..-10; map that span to 0..1.
    final db = (await _recorder.getAmplitude()).current;
    if (!db.isFinite) return 0;
    return ((db + 50) / 42).clamp(0.0, 1.0);
  }

  @override
  Future<DictationAudio?> stop() async {
    final path = await _recorder.stop() ?? _path;
    _path = null;
    if (path == null) return null;
    final file = File(path);
    try {
      if (!await file.exists()) return null;
      final bytes = await file.readAsBytes();
      return (
        bytes: bytes,
        mime: _mime,
        fileName: 'codex.${path.split('.').last}',
      );
    } finally {
      unawaited(file.delete().then((_) {}, onError: (_) {}));
    }
  }

  @override
  Future<void> cancel() async {
    await _recorder.cancel();
    final path = _path;
    _path = null;
    if (path != null) {
      unawaited(File(path).delete().then((_) {}, onError: (_) {}));
    }
  }

  @override
  Future<void> dispose() => _recorder.dispose();
}

/// Why a dictation produced no text.
enum DictationFailure {
  /// The OS refused the microphone.
  permission,

  /// Too short to be speech.
  tooShort,

  /// The microphone could not be opened.
  microphone,

  /// The host could not transcribe it.
  transcription,
}

/// Speech-to-text for the composer: record, then transcribe on the host.
///
/// Recording is local; the take is sent once, when it stops, to the host's
/// API proxy, which forwards it to ChatGPT's dictation endpoint with the
/// host's own login (the flow the Codex desktop app's microphone uses).
class DictationController extends ChangeNotifier {
  /// Dictation for [serviceKey], recording with [createRecorder].
  DictationController({
    required this.api,
    required this.serviceKey,
    DictationRecorder Function()? createRecorder,
    Stopwatch Function()? stopwatch,
  }) : _createRecorder = createRecorder ?? PlatformDictationRecorder.new,
       _clock = (stopwatch ?? Stopwatch.new)();

  /// The bridge.
  final BridgeApi api;

  /// The host that transcribes.
  final String serviceKey;

  final DictationRecorder Function() _createRecorder;
  DictationRecorder? _recorder;

  /// Where the dictation is.
  DictationPhase phase = DictationPhase.idle;

  /// The last failure, until the next start.
  DictationFailure? failure;

  /// Detail for [DictationFailure.transcription] and microphone errors.
  String? error;

  /// The input level while recording, 0..1, smoothed.
  final ValueNotifier<double> level = ValueNotifier(0);

  Timer? _levelPoll;
  Timer? _limit;
  // Monotonic, so a system clock change mid-take cannot skew the length.
  // Injectable so tests can drive it from a fake clock.
  final Stopwatch _clock;
  bool _disposed = false;
  int _generation = 0;

  /// Shorter than this is a stray tap, not speech (the desktop app's floor).
  static const minDuration = Duration(milliseconds: 250);

  /// A take stops itself here and is transcribed (the desktop app's cap).
  static const maxDuration = Duration(minutes: 9, seconds: 55);

  /// How long recording has run, for the elapsed label.
  Duration get elapsed => _clock.elapsed;

  /// Whether the microphone is open or a take is being transcribed.
  bool get busy => phase != DictationPhase.idle;

  void _changed() {
    if (!_disposed) notifyListeners();
  }

  /// Open the microphone and start a take. No-op while busy.
  Future<void> start() async {
    if (busy || _disposed) return;
    final generation = ++_generation;
    failure = null;
    error = null;
    phase = DictationPhase.starting;
    _changed();
    final recorder = _recorder ??= _createRecorder();
    try {
      if (!await recorder.hasPermission()) {
        _fail(DictationFailure.permission);
        return;
      }
      if (generation != _generation) return;
      await recorder.start();
    } catch (e) {
      if (generation == _generation) _fail(DictationFailure.microphone, '$e');
      return;
    }
    if (generation != _generation || _disposed) {
      // Cancelled while the microphone was opening.
      unawaited(recorder.cancel().catchError((_) {}));
      return;
    }
    _clock
      ..reset()
      ..start();
    phase = DictationPhase.recording;
    _levelPoll = Timer.periodic(const Duration(milliseconds: 60), (_) async {
      if (phase != DictationPhase.recording) return;
      try {
        final v = math.sqrt(await recorder.level());
        // Fast attack, slower release, like the call waveform.
        final prev = level.value;
        level.value = v > prev
            ? prev + (v - prev) * 0.7
            : prev + (v - prev) * 0.3;
      } catch (_) {
        /* A level read racing stop is harmless. */
      }
    });
    _limit = Timer(maxDuration, () {
      if (phase == DictationPhase.recording) unawaited(finish());
    });
    _changed();
  }

  void _stopTimers() {
    _levelPoll?.cancel();
    _levelPoll = null;
    _limit?.cancel();
    _limit = null;
    level.value = 0;
  }

  void _fail(DictationFailure why, [String? detail]) {
    _stopTimers();
    failure = why;
    error = detail;
    phase = DictationPhase.idle;
    _clock
      ..stop()
      ..reset();
    _changed();
  }

  /// Stop the take and transcribe it. Returns the text, or null when nothing
  /// usable came back ([failure] says why).
  Future<String?> finish({String? language}) async {
    if (phase == DictationPhase.starting) {
      await cancel();
      return null;
    }
    if (phase != DictationPhase.recording) return null;
    final generation = _generation;
    final took = elapsed;
    _clock.stop();
    _stopTimers();
    phase = DictationPhase.transcribing;
    _changed();
    DictationAudio? audio;
    try {
      audio = await _recorder?.stop();
    } catch (e) {
      _fail(DictationFailure.microphone, '$e');
      return null;
    }
    if (generation != _generation) return null;
    if (audio == null || audio.bytes.isEmpty || took < minDuration) {
      _fail(DictationFailure.tooShort);
      return null;
    }
    try {
      final text = await api.dictationTranscribe(
        serviceKey,
        audio.bytes,
        mime: audio.mime,
        fileName: audio.fileName,
        language: language,
      );
      if (generation != _generation || _disposed) return null;
      phase = DictationPhase.idle;
      _clock
        ..stop()
        ..reset();
      _changed();
      return text;
    } catch (e) {
      if (generation == _generation) {
        _fail(DictationFailure.transcription, '$e');
      }
      return null;
    }
  }

  /// Drop the take without transcribing it.
  Future<void> cancel() async {
    if (!busy) return;
    ++_generation;
    final wasRecording =
        phase == DictationPhase.recording || phase == DictationPhase.starting;
    _stopTimers();
    phase = DictationPhase.idle;
    _clock
      ..stop()
      ..reset();
    _changed();
    if (wasRecording) {
      try {
        await _recorder?.cancel();
      } catch (_) {
        /* The microphone is being released either way. */
      }
    }
  }

  @override
  void dispose() {
    _disposed = true;
    ++_generation;
    _stopTimers();
    final recorder = _recorder;
    _recorder = null;
    if (recorder != null) {
      unawaited(
        recorder
            .cancel()
            .catchError((_) {})
            .whenComplete(() => recorder.dispose().catchError((_) {})),
      );
    }
    level.dispose();
    super.dispose();
  }
}
