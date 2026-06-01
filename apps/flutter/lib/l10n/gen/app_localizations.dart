import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_zh.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'gen/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('zh'),
  ];

  /// No description provided for @appTitle.
  ///
  /// In en, this message translates to:
  /// **'Pocket-Codex'**
  String get appTitle;

  /// No description provided for @webUnsupported.
  ///
  /// In en, this message translates to:
  /// **'Pocket-Codex needs local network and file access; Web is not supported.\nUse Android / iOS / desktop.'**
  String get webUnsupported;

  /// No description provided for @onboardingTitle.
  ///
  /// In en, this message translates to:
  /// **'Connect to a pb-mapper relay'**
  String get onboardingTitle;

  /// No description provided for @importFieldLabel.
  ///
  /// In en, this message translates to:
  /// **'pcx1: share string (one-tap import)'**
  String get importFieldLabel;

  /// No description provided for @importButton.
  ///
  /// In en, this message translates to:
  /// **'Import'**
  String get importButton;

  /// No description provided for @relayFieldLabel.
  ///
  /// In en, this message translates to:
  /// **'relay host:port'**
  String get relayFieldLabel;

  /// No description provided for @keyFieldLabel.
  ///
  /// In en, this message translates to:
  /// **'MSG_HEADER_KEY (32 bytes)'**
  String get keyFieldLabel;

  /// No description provided for @save.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get save;

  /// No description provided for @cancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get cancel;

  /// No description provided for @relayEmpty.
  ///
  /// In en, this message translates to:
  /// **'relay address cannot be empty'**
  String get relayEmpty;

  /// No description provided for @keyLengthError.
  ///
  /// In en, this message translates to:
  /// **'MSG_HEADER_KEY must be exactly 32 bytes'**
  String get keyLengthError;

  /// No description provided for @apiServicesSection.
  ///
  /// In en, this message translates to:
  /// **'API services'**
  String get apiServicesSection;

  /// No description provided for @appServerServices.
  ///
  /// In en, this message translates to:
  /// **'App-server services'**
  String get appServerServices;

  /// No description provided for @appServerSubtitle.
  ///
  /// In en, this message translates to:
  /// **'{device} · sessions in P2'**
  String appServerSubtitle(String device);

  /// No description provided for @selectApiService.
  ///
  /// In en, this message translates to:
  /// **'Select an API service'**
  String get selectApiService;

  /// No description provided for @relayNotConfigured.
  ///
  /// In en, this message translates to:
  /// **'(no relay configured)'**
  String get relayNotConfigured;

  /// No description provided for @noServicesFound.
  ///
  /// In en, this message translates to:
  /// **'No services found on this relay'**
  String get noServicesFound;

  /// No description provided for @retry.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get retry;

  /// No description provided for @discoverFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t reach the relay'**
  String get discoverFailed;

  /// No description provided for @apiServiceTitle.
  ///
  /// In en, this message translates to:
  /// **'API service'**
  String get apiServiceTitle;

  /// No description provided for @localPortLabel.
  ///
  /// In en, this message translates to:
  /// **'Local port'**
  String get localPortLabel;

  /// No description provided for @startSubscription.
  ///
  /// In en, this message translates to:
  /// **'Start subscription'**
  String get startSubscription;

  /// No description provided for @stop.
  ///
  /// In en, this message translates to:
  /// **'Stop'**
  String get stop;

  /// No description provided for @portRangeError.
  ///
  /// In en, this message translates to:
  /// **'Port must be an integer from 1 to 65535'**
  String get portRangeError;

  /// No description provided for @noAuthWarning.
  ///
  /// In en, this message translates to:
  /// **'⚠ The local endpoint has no auth and binds 127.0.0.1 only. Alive only while the app is foregrounded.'**
  String get noAuthWarning;

  /// No description provided for @subscribeFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t start the subscription'**
  String get subscribeFailed;

  /// No description provided for @settingsTitle.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get settingsTitle;

  /// No description provided for @relayRow.
  ///
  /// In en, this message translates to:
  /// **'relay'**
  String get relayRow;

  /// No description provided for @notConfigured.
  ///
  /// In en, this message translates to:
  /// **'(not configured)'**
  String get notConfigured;

  /// No description provided for @keyRow.
  ///
  /// In en, this message translates to:
  /// **'MSG_HEADER_KEY'**
  String get keyRow;

  /// No description provided for @keySet.
  ///
  /// In en, this message translates to:
  /// **'•••••••• (set)'**
  String get keySet;

  /// No description provided for @keyNotSet.
  ///
  /// In en, this message translates to:
  /// **'(not set)'**
  String get keyNotSet;

  /// No description provided for @activeSubscriptions.
  ///
  /// In en, this message translates to:
  /// **'Active subscriptions'**
  String get activeSubscriptions;

  /// No description provided for @none.
  ///
  /// In en, this message translates to:
  /// **'(none)'**
  String get none;

  /// No description provided for @exportShareString.
  ///
  /// In en, this message translates to:
  /// **'Export pcx1: share string'**
  String get exportShareString;

  /// No description provided for @copiedShareString.
  ///
  /// In en, this message translates to:
  /// **'Copied pcx1: share string'**
  String get copiedShareString;

  /// No description provided for @language.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get language;

  /// No description provided for @languageSystem.
  ///
  /// In en, this message translates to:
  /// **'Follow system'**
  String get languageSystem;

  /// No description provided for @languageChinese.
  ///
  /// In en, this message translates to:
  /// **'简体中文'**
  String get languageChinese;

  /// No description provided for @languageEnglish.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get languageEnglish;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'zh'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'zh':
      return AppLocalizationsZh();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
