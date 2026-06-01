// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'Pocket-Codex';

  @override
  String get webUnsupported =>
      'Pocket-Codex needs local network and file access; Web is not supported.\nUse Android / iOS / desktop.';

  @override
  String get onboardingTitle => 'Connect to a pb-mapper relay';

  @override
  String get importFieldLabel => 'pcx1: share string (one-tap import)';

  @override
  String get importButton => 'Import';

  @override
  String get relayFieldLabel => 'relay host:port';

  @override
  String get keyFieldLabel => 'MSG_HEADER_KEY (32 bytes)';

  @override
  String get save => 'Save';

  @override
  String get cancel => 'Cancel';

  @override
  String get relayEmpty => 'relay address cannot be empty';

  @override
  String get keyLengthError => 'MSG_HEADER_KEY must be exactly 32 bytes';

  @override
  String get apiServicesSection => 'API services';

  @override
  String get appServerServices => 'App-server services';

  @override
  String appServerSubtitle(String device) {
    return '$device · sessions in P2';
  }

  @override
  String get selectApiService => 'Select an API service';

  @override
  String get relayNotConfigured => '(no relay configured)';

  @override
  String get noServicesFound => 'No services found on this relay';

  @override
  String get retry => 'Retry';

  @override
  String get discoverFailed => 'Couldn\'t reach the relay';

  @override
  String get apiServiceTitle => 'API service';

  @override
  String get localPortLabel => 'Local port';

  @override
  String get startSubscription => 'Start subscription';

  @override
  String get stop => 'Stop';

  @override
  String get portRangeError => 'Port must be an integer from 1 to 65535';

  @override
  String get noAuthWarning =>
      '⚠ The local endpoint has no auth and binds 127.0.0.1 only. Alive only while the app is foregrounded.';

  @override
  String get subscribeFailed => 'Couldn\'t start the subscription';

  @override
  String get settingsTitle => 'Settings';

  @override
  String get relayRow => 'relay';

  @override
  String get notConfigured => '(not configured)';

  @override
  String get keyRow => 'MSG_HEADER_KEY';

  @override
  String get keySet => '•••••••• (set)';

  @override
  String get keyNotSet => '(not set)';

  @override
  String get activeSubscriptions => 'Active subscriptions';

  @override
  String get none => '(none)';

  @override
  String get exportShareString => 'Export pcx1: share string';

  @override
  String get copiedShareString => 'Copied pcx1: share string';

  @override
  String get language => 'Language';

  @override
  String get languageSystem => 'Follow system';

  @override
  String get languageChinese => '简体中文';

  @override
  String get languageEnglish => 'English';
}
