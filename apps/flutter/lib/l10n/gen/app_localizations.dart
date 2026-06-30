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

  /// No description provided for @accountSignInTitle.
  ///
  /// In en, this message translates to:
  /// **'Sign in'**
  String get accountSignInTitle;

  /// No description provided for @accountSignInButton.
  ///
  /// In en, this message translates to:
  /// **'Sign in with GitHub'**
  String get accountSignInButton;

  /// No description provided for @accountUseDeviceCode.
  ///
  /// In en, this message translates to:
  /// **'Use a device code instead'**
  String get accountUseDeviceCode;

  /// No description provided for @accountWebFailed.
  ///
  /// In en, this message translates to:
  /// **'Sign-in didn\'t complete. Please try again.'**
  String get accountWebFailed;

  /// No description provided for @accountWebTrouble.
  ///
  /// In en, this message translates to:
  /// **'Browser sign-in didn\'t finish. If the GitHub page wouldn\'t load, use the device code below.'**
  String get accountWebTrouble;

  /// No description provided for @accountSignedIn.
  ///
  /// In en, this message translates to:
  /// **'Signed in'**
  String get accountSignedIn;

  /// No description provided for @accountSignedInAs.
  ///
  /// In en, this message translates to:
  /// **'Signed in as @{login}'**
  String accountSignedInAs(String login);

  /// No description provided for @accountEnterCode.
  ///
  /// In en, this message translates to:
  /// **'Enter this code on GitHub to finish signing in:'**
  String get accountEnterCode;

  /// No description provided for @accountCopyCode.
  ///
  /// In en, this message translates to:
  /// **'Copy code'**
  String get accountCopyCode;

  /// No description provided for @accountOpenGitHub.
  ///
  /// In en, this message translates to:
  /// **'Open GitHub'**
  String get accountOpenGitHub;

  /// No description provided for @accountWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting for you to authorize on GitHub…'**
  String get accountWaiting;

  /// No description provided for @accountCodeExpired.
  ///
  /// In en, this message translates to:
  /// **'The code expired. Please try again.'**
  String get accountCodeExpired;

  /// No description provided for @accountDenied.
  ///
  /// In en, this message translates to:
  /// **'Sign-in was denied on GitHub.'**
  String get accountDenied;

  /// No description provided for @accountAdvancedSelfHost.
  ///
  /// In en, this message translates to:
  /// **'Use a self-hosted relay instead'**
  String get accountAdvancedSelfHost;

  /// No description provided for @accountAdvanced.
  ///
  /// In en, this message translates to:
  /// **'Advanced / self-hosted'**
  String get accountAdvanced;

  /// No description provided for @accountBackendHint.
  ///
  /// In en, this message translates to:
  /// **'Backend URL (blank = default)'**
  String get accountBackendHint;

  /// No description provided for @accountSection.
  ///
  /// In en, this message translates to:
  /// **'Account'**
  String get accountSection;

  /// No description provided for @accountSignOut.
  ///
  /// In en, this message translates to:
  /// **'Sign out'**
  String get accountSignOut;

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

  /// No description provided for @navApi.
  ///
  /// In en, this message translates to:
  /// **'API'**
  String get navApi;

  /// No description provided for @navAppServer.
  ///
  /// In en, this message translates to:
  /// **'App-server'**
  String get navAppServer;

  /// No description provided for @navSessions.
  ///
  /// In en, this message translates to:
  /// **'Sessions'**
  String get navSessions;

  /// No description provided for @navHosting.
  ///
  /// In en, this message translates to:
  /// **'Hosting'**
  String get navHosting;

  /// No description provided for @sessionsHostLabel.
  ///
  /// In en, this message translates to:
  /// **'Host'**
  String get sessionsHostLabel;

  /// No description provided for @sessionsNoHost.
  ///
  /// In en, this message translates to:
  /// **'Connect an app-server first, then its sessions show here.'**
  String get sessionsNoHost;

  /// No description provided for @appServerSubtitle.
  ///
  /// In en, this message translates to:
  /// **'{device} · remote control'**
  String appServerSubtitle(String device);

  /// No description provided for @appServiceTitle.
  ///
  /// In en, this message translates to:
  /// **'App-server'**
  String get appServiceTitle;

  /// No description provided for @connecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting…'**
  String get connecting;

  /// No description provided for @connectFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t connect to the app-server'**
  String get connectFailed;

  /// No description provided for @conversationsSection.
  ///
  /// In en, this message translates to:
  /// **'Conversations'**
  String get conversationsSection;

  /// No description provided for @newConversation.
  ///
  /// In en, this message translates to:
  /// **'New conversation'**
  String get newConversation;

  /// No description provided for @noThreads.
  ///
  /// In en, this message translates to:
  /// **'No conversations yet'**
  String get noThreads;

  /// No description provided for @untitledThread.
  ///
  /// In en, this message translates to:
  /// **'(untitled)'**
  String get untitledThread;

  /// No description provided for @messageHint.
  ///
  /// In en, this message translates to:
  /// **'Message…'**
  String get messageHint;

  /// No description provided for @send.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get send;

  /// No description provided for @interrupt.
  ///
  /// In en, this message translates to:
  /// **'Interrupt'**
  String get interrupt;

  /// No description provided for @thinking.
  ///
  /// In en, this message translates to:
  /// **'Thinking…'**
  String get thinking;

  /// No description provided for @emptyConversation.
  ///
  /// In en, this message translates to:
  /// **'Send a message to start the conversation'**
  String get emptyConversation;

  /// No description provided for @turnFailed.
  ///
  /// In en, this message translates to:
  /// **'Turn didn\'t finish — the connection dropped or the remote codex failed. Retry, or check codex on the host machine (it may need to be logged in again).'**
  String get turnFailed;

  /// No description provided for @disconnect.
  ///
  /// In en, this message translates to:
  /// **'Disconnect'**
  String get disconnect;

  /// No description provided for @connectionLost.
  ///
  /// In en, this message translates to:
  /// **'Connection lost'**
  String get connectionLost;

  /// No description provided for @reconnect.
  ///
  /// In en, this message translates to:
  /// **'Reconnect'**
  String get reconnect;

  /// No description provided for @projectsSection.
  ///
  /// In en, this message translates to:
  /// **'Projects'**
  String get projectsSection;

  /// No description provided for @newProject.
  ///
  /// In en, this message translates to:
  /// **'New project'**
  String get newProject;

  /// No description provided for @currentProject.
  ///
  /// In en, this message translates to:
  /// **'Project'**
  String get currentProject;

  /// No description provided for @remotePathLabel.
  ///
  /// In en, this message translates to:
  /// **'Remote folder path (on the host)'**
  String get remotePathLabel;

  /// No description provided for @remotePathHint.
  ///
  /// In en, this message translates to:
  /// **'e.g. /home/ubuntu/myproject — blank uses the host default'**
  String get remotePathHint;

  /// No description provided for @model.
  ///
  /// In en, this message translates to:
  /// **'Model'**
  String get model;

  /// No description provided for @modelDefault.
  ///
  /// In en, this message translates to:
  /// **'Default model'**
  String get modelDefault;

  /// No description provided for @defaultFolder.
  ///
  /// In en, this message translates to:
  /// **'Default folder'**
  String get defaultFolder;

  /// No description provided for @permissionMode.
  ///
  /// In en, this message translates to:
  /// **'Permission'**
  String get permissionMode;

  /// No description provided for @modeReadOnly.
  ///
  /// In en, this message translates to:
  /// **'Read-only'**
  String get modeReadOnly;

  /// No description provided for @modeReadOnlyDesc.
  ///
  /// In en, this message translates to:
  /// **'Ask before running; no file writes'**
  String get modeReadOnlyDesc;

  /// No description provided for @modeAuto.
  ///
  /// In en, this message translates to:
  /// **'Auto'**
  String get modeAuto;

  /// No description provided for @modeAutoDesc.
  ///
  /// In en, this message translates to:
  /// **'Write in workspace; ask only on failure'**
  String get modeAutoDesc;

  /// No description provided for @modeFull.
  ///
  /// In en, this message translates to:
  /// **'Full access'**
  String get modeFull;

  /// No description provided for @modeFullDesc.
  ///
  /// In en, this message translates to:
  /// **'No sandbox, never ask (use with care)'**
  String get modeFullDesc;

  /// No description provided for @approvalPrompt.
  ///
  /// In en, this message translates to:
  /// **'The agent wants to run a command'**
  String get approvalPrompt;

  /// No description provided for @approvalFilePrompt.
  ///
  /// In en, this message translates to:
  /// **'The agent wants to edit files'**
  String get approvalFilePrompt;

  /// No description provided for @approvalPermissionPrompt.
  ///
  /// In en, this message translates to:
  /// **'The agent requests additional permission'**
  String get approvalPermissionPrompt;

  /// No description provided for @approve.
  ///
  /// In en, this message translates to:
  /// **'Approve'**
  String get approve;

  /// No description provided for @approveForSession.
  ///
  /// In en, this message translates to:
  /// **'Approve for session'**
  String get approveForSession;

  /// No description provided for @deny.
  ///
  /// In en, this message translates to:
  /// **'Deny'**
  String get deny;

  /// No description provided for @userInputTitle.
  ///
  /// In en, this message translates to:
  /// **'The agent needs your input'**
  String get userInputTitle;

  /// No description provided for @userInputSubmit.
  ///
  /// In en, this message translates to:
  /// **'Submit'**
  String get userInputSubmit;

  /// No description provided for @userInputOther.
  ///
  /// In en, this message translates to:
  /// **'Other…'**
  String get userInputOther;

  /// No description provided for @planMode.
  ///
  /// In en, this message translates to:
  /// **'Plan'**
  String get planMode;

  /// No description provided for @planReadyTitle.
  ///
  /// In en, this message translates to:
  /// **'Plan ready'**
  String get planReadyTitle;

  /// No description provided for @implementPlan.
  ///
  /// In en, this message translates to:
  /// **'Implement plan'**
  String get implementPlan;

  /// No description provided for @keepPlanning.
  ///
  /// In en, this message translates to:
  /// **'Keep planning'**
  String get keepPlanning;

  /// No description provided for @implementPlanPrompt.
  ///
  /// In en, this message translates to:
  /// **'Go ahead and implement the plan above.'**
  String get implementPlanPrompt;

  /// No description provided for @noModelForMode.
  ///
  /// In en, this message translates to:
  /// **'Can\'t switch mode: no model is available'**
  String get noModelForMode;

  /// No description provided for @effort.
  ///
  /// In en, this message translates to:
  /// **'Effort'**
  String get effort;

  /// No description provided for @effortMinimal.
  ///
  /// In en, this message translates to:
  /// **'Minimal'**
  String get effortMinimal;

  /// No description provided for @effortMinimalDesc.
  ///
  /// In en, this message translates to:
  /// **'Least thinking, fastest'**
  String get effortMinimalDesc;

  /// No description provided for @effortLow.
  ///
  /// In en, this message translates to:
  /// **'Low'**
  String get effortLow;

  /// No description provided for @effortLowDesc.
  ///
  /// In en, this message translates to:
  /// **'A little thinking'**
  String get effortLowDesc;

  /// No description provided for @effortMedium.
  ///
  /// In en, this message translates to:
  /// **'Medium'**
  String get effortMedium;

  /// No description provided for @effortMediumDesc.
  ///
  /// In en, this message translates to:
  /// **'Balanced (usual default)'**
  String get effortMediumDesc;

  /// No description provided for @effortHigh.
  ///
  /// In en, this message translates to:
  /// **'High'**
  String get effortHigh;

  /// No description provided for @effortHighDesc.
  ///
  /// In en, this message translates to:
  /// **'Thorough'**
  String get effortHighDesc;

  /// No description provided for @effortXhigh.
  ///
  /// In en, this message translates to:
  /// **'Extra high'**
  String get effortXhigh;

  /// No description provided for @effortXhighDesc.
  ///
  /// In en, this message translates to:
  /// **'Most thorough, slowest'**
  String get effortXhighDesc;

  /// No description provided for @openLink.
  ///
  /// In en, this message translates to:
  /// **'Open link'**
  String get openLink;

  /// No description provided for @linkOpenFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t open the link'**
  String get linkOpenFailed;

  /// No description provided for @contextLabel.
  ///
  /// In en, this message translates to:
  /// **'Context'**
  String get contextLabel;

  /// No description provided for @contextUsageTitle.
  ///
  /// In en, this message translates to:
  /// **'Context & usage'**
  String get contextUsageTitle;

  /// No description provided for @quota5h.
  ///
  /// In en, this message translates to:
  /// **'5-hour limit'**
  String get quota5h;

  /// No description provided for @quotaWeekly.
  ///
  /// In en, this message translates to:
  /// **'Weekly limit'**
  String get quotaWeekly;

  /// No description provided for @quotaUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Quota information is unavailable.'**
  String get quotaUnavailable;

  /// No description provided for @resetsIn.
  ///
  /// In en, this message translates to:
  /// **'resets in {span}'**
  String resetsIn(String span);

  /// No description provided for @moreActions.
  ///
  /// In en, this message translates to:
  /// **'More'**
  String get moreActions;

  /// No description provided for @backToProjects.
  ///
  /// In en, this message translates to:
  /// **'Back to projects'**
  String get backToProjects;

  /// No description provided for @stateReady.
  ///
  /// In en, this message translates to:
  /// **'Ready'**
  String get stateReady;

  /// No description provided for @stateWorking.
  ///
  /// In en, this message translates to:
  /// **'Working…'**
  String get stateWorking;

  /// No description provided for @statePlanning.
  ///
  /// In en, this message translates to:
  /// **'Planning…'**
  String get statePlanning;

  /// No description provided for @statePlanMode.
  ///
  /// In en, this message translates to:
  /// **'Plan mode'**
  String get statePlanMode;

  /// No description provided for @stateDisconnected.
  ///
  /// In en, this message translates to:
  /// **'Disconnected'**
  String get stateDisconnected;

  /// No description provided for @stateReconnecting.
  ///
  /// In en, this message translates to:
  /// **'Reconnecting…'**
  String get stateReconnecting;

  /// No description provided for @compacted.
  ///
  /// In en, this message translates to:
  /// **'Conversation compacted'**
  String get compacted;

  /// No description provided for @turnStopped.
  ///
  /// In en, this message translates to:
  /// **'Stopped'**
  String get turnStopped;

  /// No description provided for @turnElapsed.
  ///
  /// In en, this message translates to:
  /// **'Took {duration}'**
  String turnElapsed(String duration);

  /// No description provided for @completedAt.
  ///
  /// In en, this message translates to:
  /// **'Completed at {time}'**
  String completedAt(String time);

  /// No description provided for @refreshStatus.
  ///
  /// In en, this message translates to:
  /// **'Refresh status'**
  String get refreshStatus;

  /// No description provided for @statusOnline.
  ///
  /// In en, this message translates to:
  /// **'Online'**
  String get statusOnline;

  /// No description provided for @statusConnected.
  ///
  /// In en, this message translates to:
  /// **'Connected'**
  String get statusConnected;

  /// No description provided for @statusChecking.
  ///
  /// In en, this message translates to:
  /// **'Checking…'**
  String get statusChecking;

  /// No description provided for @statusUnreachable.
  ///
  /// In en, this message translates to:
  /// **'Unreachable'**
  String get statusUnreachable;

  /// No description provided for @unreachableReason.
  ///
  /// In en, this message translates to:
  /// **'Still registered on the relay, but the remote app-server isn\'t responding — it may not be running, or has crashed.'**
  String get unreachableReason;

  /// No description provided for @apiUnreachableReason.
  ///
  /// In en, this message translates to:
  /// **'Still registered on the relay, but the remote API service isn\'t responding — it may not be running, or has crashed.'**
  String get apiUnreachableReason;

  /// No description provided for @subscribedAlive.
  ///
  /// In en, this message translates to:
  /// **'Subscribed'**
  String get subscribedAlive;

  /// No description provided for @subscribedDead.
  ///
  /// In en, this message translates to:
  /// **'Dropped'**
  String get subscribedDead;

  /// No description provided for @runningSessions.
  ///
  /// In en, this message translates to:
  /// **'{count} running'**
  String runningSessions(int count);

  /// No description provided for @compact.
  ///
  /// In en, this message translates to:
  /// **'Compact conversation'**
  String get compact;

  /// No description provided for @compactConfirm.
  ///
  /// In en, this message translates to:
  /// **'Summarise and shrink this conversation to free up context? This can\'t be undone.'**
  String get compactConfirm;

  /// No description provided for @viewDiff.
  ///
  /// In en, this message translates to:
  /// **'View changes'**
  String get viewDiff;

  /// No description provided for @changesTitle.
  ///
  /// In en, this message translates to:
  /// **'Changes'**
  String get changesTitle;

  /// No description provided for @noChanges.
  ///
  /// In en, this message translates to:
  /// **'No changes vs the main branch.'**
  String get noChanges;

  /// No description provided for @start.
  ///
  /// In en, this message translates to:
  /// **'Start'**
  String get start;

  /// No description provided for @create.
  ///
  /// In en, this message translates to:
  /// **'Create'**
  String get create;

  /// No description provided for @copy.
  ///
  /// In en, this message translates to:
  /// **'Copy'**
  String get copy;

  /// No description provided for @copied.
  ///
  /// In en, this message translates to:
  /// **'Copied'**
  String get copied;

  /// No description provided for @toolSearched.
  ///
  /// In en, this message translates to:
  /// **'Searched the web'**
  String get toolSearched;

  /// No description provided for @toolRan.
  ///
  /// In en, this message translates to:
  /// **'Ran command'**
  String get toolRan;

  /// No description provided for @toolEdited.
  ///
  /// In en, this message translates to:
  /// **'Edited files'**
  String get toolEdited;

  /// No description provided for @toolCalled.
  ///
  /// In en, this message translates to:
  /// **'Used a tool'**
  String get toolCalled;

  /// No description provided for @toolThinking.
  ///
  /// In en, this message translates to:
  /// **'Thinking'**
  String get toolThinking;

  /// No description provided for @toolPlan.
  ///
  /// In en, this message translates to:
  /// **'Plan'**
  String get toolPlan;

  /// No description provided for @toolActivity.
  ///
  /// In en, this message translates to:
  /// **'Activity'**
  String get toolActivity;

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

  /// No description provided for @trayShow.
  ///
  /// In en, this message translates to:
  /// **'Show window'**
  String get trayShow;

  /// No description provided for @trayQuit.
  ///
  /// In en, this message translates to:
  /// **'Quit'**
  String get trayQuit;

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

  /// No description provided for @newSessionTitle.
  ///
  /// In en, this message translates to:
  /// **'What should the remote Codex work on?'**
  String get newSessionTitle;

  /// No description provided for @newSessionSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Pick a starting point, or just type your task below.'**
  String get newSessionSubtitle;

  /// No description provided for @suggestExploreTitle.
  ///
  /// In en, this message translates to:
  /// **'Explore the project'**
  String get suggestExploreTitle;

  /// No description provided for @suggestExplorePrompt.
  ///
  /// In en, this message translates to:
  /// **'Give me an overview of this project — its structure, main modules, and tech stack.'**
  String get suggestExplorePrompt;

  /// No description provided for @suggestTestsTitle.
  ///
  /// In en, this message translates to:
  /// **'Run & fix tests'**
  String get suggestTestsTitle;

  /// No description provided for @suggestTestsPrompt.
  ///
  /// In en, this message translates to:
  /// **'Run the test suite and fix any failing tests.'**
  String get suggestTestsPrompt;

  /// No description provided for @suggestDiffTitle.
  ///
  /// In en, this message translates to:
  /// **'Review changes'**
  String get suggestDiffTitle;

  /// No description provided for @suggestDiffPrompt.
  ///
  /// In en, this message translates to:
  /// **'Summarize the current working-tree changes against the main branch.'**
  String get suggestDiffPrompt;

  /// No description provided for @suggestPlanTitle.
  ///
  /// In en, this message translates to:
  /// **'Plan a feature'**
  String get suggestPlanTitle;

  /// No description provided for @suggestPlanPrompt.
  ///
  /// In en, this message translates to:
  /// **'Help me plan out a new feature before writing any code.'**
  String get suggestPlanPrompt;

  /// No description provided for @searchConversations.
  ///
  /// In en, this message translates to:
  /// **'Search conversations'**
  String get searchConversations;

  /// No description provided for @searchLocalSessions.
  ///
  /// In en, this message translates to:
  /// **'Search content / path / source'**
  String get searchLocalSessions;

  /// No description provided for @noMatchingThreads.
  ///
  /// In en, this message translates to:
  /// **'No matching conversations'**
  String get noMatchingThreads;

  /// No description provided for @groupActive.
  ///
  /// In en, this message translates to:
  /// **'Active'**
  String get groupActive;

  /// No description provided for @groupToday.
  ///
  /// In en, this message translates to:
  /// **'Today'**
  String get groupToday;

  /// No description provided for @groupEarlier.
  ///
  /// In en, this message translates to:
  /// **'Earlier'**
  String get groupEarlier;

  /// No description provided for @running.
  ///
  /// In en, this message translates to:
  /// **'Running…'**
  String get running;

  /// No description provided for @timeJustNow.
  ///
  /// In en, this message translates to:
  /// **'just now'**
  String get timeJustNow;

  /// No description provided for @timeMinutesAgo.
  ///
  /// In en, this message translates to:
  /// **'{n}m ago'**
  String timeMinutesAgo(int n);

  /// No description provided for @timeHoursAgo.
  ///
  /// In en, this message translates to:
  /// **'{n}h ago'**
  String timeHoursAgo(int n);

  /// No description provided for @timeYesterday.
  ///
  /// In en, this message translates to:
  /// **'yesterday'**
  String get timeYesterday;

  /// No description provided for @timeDaysAgo.
  ///
  /// In en, this message translates to:
  /// **'{n}d ago'**
  String timeDaysAgo(int n);

  /// No description provided for @modelLabel.
  ///
  /// In en, this message translates to:
  /// **'Model'**
  String get modelLabel;

  /// No description provided for @permissionLabel.
  ///
  /// In en, this message translates to:
  /// **'Permission'**
  String get permissionLabel;

  /// No description provided for @localSessions.
  ///
  /// In en, this message translates to:
  /// **'Local sessions'**
  String get localSessions;

  /// No description provided for @localSessionsTitle.
  ///
  /// In en, this message translates to:
  /// **'Local sessions'**
  String get localSessionsTitle;

  /// No description provided for @localSessionsHint.
  ///
  /// In en, this message translates to:
  /// **'Sessions under this CODEX_HOME, including ones the desktop app or CLI created. Resume a finished one here.'**
  String get localSessionsHint;

  /// No description provided for @noLocalSessions.
  ///
  /// In en, this message translates to:
  /// **'No local sessions'**
  String get noLocalSessions;

  /// No description provided for @sessionResumable.
  ///
  /// In en, this message translates to:
  /// **'Resumable'**
  String get sessionResumable;

  /// No description provided for @sessionUnfinished.
  ///
  /// In en, this message translates to:
  /// **'Last turn interrupted'**
  String get sessionUnfinished;

  /// No description provided for @sessionRunningElsewhere.
  ///
  /// In en, this message translates to:
  /// **'Running elsewhere'**
  String get sessionRunningElsewhere;

  /// No description provided for @sessionInUseElsewhere.
  ///
  /// In en, this message translates to:
  /// **'In use elsewhere'**
  String get sessionInUseElsewhere;

  /// No description provided for @sessionReadOnly.
  ///
  /// In en, this message translates to:
  /// **'Read-only'**
  String get sessionReadOnly;

  /// No description provided for @readOnlyViewing.
  ///
  /// In en, this message translates to:
  /// **'Read-only — another client is using this session'**
  String get readOnlyViewing;

  /// No description provided for @sessionTranscriptEmpty.
  ///
  /// In en, this message translates to:
  /// **'Nothing to show yet'**
  String get sessionTranscriptEmpty;

  /// No description provided for @resumeSession.
  ///
  /// In en, this message translates to:
  /// **'Resume'**
  String get resumeSession;

  /// No description provided for @forceTakeover.
  ///
  /// In en, this message translates to:
  /// **'Force takeover'**
  String get forceTakeover;

  /// No description provided for @takeoverTitle.
  ///
  /// In en, this message translates to:
  /// **'Force takeover?'**
  String get takeoverTitle;

  /// No description provided for @takeoverBody.
  ///
  /// In en, this message translates to:
  /// **'This session is held open by {n} other process(es). Pocket-Codex will try to terminate them, then resume it here. Any unsaved work in those processes will be lost.'**
  String takeoverBody(int n);

  /// No description provided for @takeoverWillTerminate.
  ///
  /// In en, this message translates to:
  /// **'Will terminate'**
  String get takeoverWillTerminate;

  /// No description provided for @takeoverConfirm.
  ///
  /// In en, this message translates to:
  /// **'Terminate & resume'**
  String get takeoverConfirm;

  /// No description provided for @takeoverResumed.
  ///
  /// In en, this message translates to:
  /// **'Session resumed'**
  String get takeoverResumed;

  /// No description provided for @takeoverKilled.
  ///
  /// In en, this message translates to:
  /// **'Terminated {n} process(es)'**
  String takeoverKilled(int n);

  /// No description provided for @takeoverStillHeld.
  ///
  /// In en, this message translates to:
  /// **'Still held open — resumed anyway'**
  String get takeoverStillHeld;

  /// No description provided for @takeoverResumeFailed.
  ///
  /// In en, this message translates to:
  /// **'Resume failed: {error}'**
  String takeoverResumeFailed(String error);

  /// No description provided for @takeoverNoTarget.
  ///
  /// In en, this message translates to:
  /// **'Connect to an app-server service first to resume.'**
  String get takeoverNoTarget;

  /// No description provided for @holderRow.
  ///
  /// In en, this message translates to:
  /// **'{name} · PID {pid}'**
  String holderRow(String name, int pid);

  /// No description provided for @localHostingSection.
  ///
  /// In en, this message translates to:
  /// **'Local hosting'**
  String get localHostingSection;

  /// No description provided for @localHostTitle.
  ///
  /// In en, this message translates to:
  /// **'Local codex'**
  String get localHostTitle;

  /// No description provided for @localHostStopped.
  ///
  /// In en, this message translates to:
  /// **'Stopped'**
  String get localHostStopped;

  /// No description provided for @localHostRunning.
  ///
  /// In en, this message translates to:
  /// **'Hosting'**
  String get localHostRunning;

  /// No description provided for @localHostStarting.
  ///
  /// In en, this message translates to:
  /// **'Starting…'**
  String get localHostStarting;

  /// No description provided for @startHosting.
  ///
  /// In en, this message translates to:
  /// **'Start hosting'**
  String get startHosting;

  /// No description provided for @stopHosting.
  ///
  /// In en, this message translates to:
  /// **'Stop hosting'**
  String get stopHosting;

  /// No description provided for @localHostPort.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get localHostPort;

  /// No description provided for @localHostName.
  ///
  /// In en, this message translates to:
  /// **'Instance name'**
  String get localHostName;

  /// No description provided for @codexBinaryPath.
  ///
  /// In en, this message translates to:
  /// **'codex binary path'**
  String get codexBinaryPath;

  /// No description provided for @codexNotFound.
  ///
  /// In en, this message translates to:
  /// **'codex wasn\'t found on PATH — enter its full path below, or install it and tap Re-detect.'**
  String get codexNotFound;

  /// No description provided for @codexRedetect.
  ///
  /// In en, this message translates to:
  /// **'Re-detect'**
  String get codexRedetect;

  /// No description provided for @localHostDialogTitle.
  ///
  /// In en, this message translates to:
  /// **'Host a local app-server'**
  String get localHostDialogTitle;

  /// No description provided for @localHostHint.
  ///
  /// In en, this message translates to:
  /// **'Runs codex on this machine and registers it to your account, so your other devices can drive it.'**
  String get localHostHint;

  /// No description provided for @localHostListening.
  ///
  /// In en, this message translates to:
  /// **'Listening on {addr}'**
  String localHostListening(String addr);

  /// No description provided for @localHostStartError.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t start hosting: {error}'**
  String localHostStartError(String error);

  /// No description provided for @codexFoundAt.
  ///
  /// In en, this message translates to:
  /// **'codex found: {path}'**
  String codexFoundAt(String path);

  /// No description provided for @chooseCodexPath.
  ///
  /// In en, this message translates to:
  /// **'Choose codex binary…'**
  String get chooseCodexPath;

  /// No description provided for @codexPathRequired.
  ///
  /// In en, this message translates to:
  /// **'Choose the codex binary to continue.'**
  String get codexPathRequired;

  /// No description provided for @useProxy.
  ///
  /// In en, this message translates to:
  /// **'Use a proxy'**
  String get useProxy;

  /// No description provided for @proxyLabel.
  ///
  /// In en, this message translates to:
  /// **'Proxy'**
  String get proxyLabel;

  /// No description provided for @proxyRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter a proxy, or turn off “Use a proxy”.'**
  String get proxyRequired;

  /// No description provided for @noProxyWarning.
  ///
  /// In en, this message translates to:
  /// **'Without a proxy, codex on this machine may fail to reach chatgpt.com.'**
  String get noProxyWarning;

  /// No description provided for @addLocalHost.
  ///
  /// In en, this message translates to:
  /// **'Host another…'**
  String get addLocalHost;

  /// No description provided for @customizeCodexPath.
  ///
  /// In en, this message translates to:
  /// **'Change path'**
  String get customizeCodexPath;

  /// No description provided for @appServerSubtitleLocal.
  ///
  /// In en, this message translates to:
  /// **'{device} · hosted here'**
  String appServerSubtitleLocal(String device);

  /// No description provided for @deregister.
  ///
  /// In en, this message translates to:
  /// **'Deregister'**
  String get deregister;

  /// No description provided for @reregister.
  ///
  /// In en, this message translates to:
  /// **'Re-register'**
  String get reregister;

  /// No description provided for @deregisterTitle.
  ///
  /// In en, this message translates to:
  /// **'Deregister this service?'**
  String get deregisterTitle;

  /// No description provided for @deregisterWarning.
  ///
  /// In en, this message translates to:
  /// **'Remove “{name}” from your account\'s relay listing. If a host is still running it, it will re-register within seconds — stop that host to remove it for good.'**
  String deregisterWarning(String name);

  /// No description provided for @deregisterLocalWarning.
  ///
  /// In en, this message translates to:
  /// **'Take “{name}” off the relay. codex and the API proxy keep running — re-register it from the Local hosting card anytime.'**
  String deregisterLocalWarning(String name);

  /// No description provided for @deregisterFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t deregister the service'**
  String get deregisterFailed;

  /// No description provided for @tunnelAppLabel.
  ///
  /// In en, this message translates to:
  /// **'App-server'**
  String get tunnelAppLabel;

  /// No description provided for @tunnelApiLabel.
  ///
  /// In en, this message translates to:
  /// **'API'**
  String get tunnelApiLabel;

  /// No description provided for @tunnelMetaLabel.
  ///
  /// In en, this message translates to:
  /// **'Sessions (meta)'**
  String get tunnelMetaLabel;

  /// No description provided for @tunnelOffline.
  ///
  /// In en, this message translates to:
  /// **'Offline'**
  String get tunnelOffline;
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
