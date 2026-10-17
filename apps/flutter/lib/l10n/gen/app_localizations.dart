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
  /// **'PocketCodex'**
  String get appTitle;

  /// No description provided for @webUnsupported.
  ///
  /// In en, this message translates to:
  /// **'PocketCodex needs local network and file access; Web is not supported.\nUse Android / iOS / desktop.'**
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

  /// No description provided for @accountWelcomeHeadline.
  ///
  /// In en, this message translates to:
  /// **'Welcome to PocketCodex'**
  String get accountWelcomeHeadline;

  /// No description provided for @accountWelcomeSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Sign in to pick up your conversations on any device.'**
  String get accountWelcomeSubtitle;

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

  /// No description provided for @accountSessionExpiredTitle.
  ///
  /// In en, this message translates to:
  /// **'Your session expired'**
  String get accountSessionExpiredTitle;

  /// No description provided for @accountSessionExpiredMessage.
  ///
  /// In en, this message translates to:
  /// **'Your saved sign-in can no longer be renewed. Sign in again to reconnect to your services.'**
  String get accountSessionExpiredMessage;

  /// No description provided for @accountSignInAgain.
  ///
  /// In en, this message translates to:
  /// **'Sign in again'**
  String get accountSignInAgain;

  /// No description provided for @accountWebFailed.
  ///
  /// In en, this message translates to:
  /// **'Sign-in didn\'t complete. Please try again.'**
  String get accountWebFailed;

  /// No description provided for @accountWebTrouble.
  ///
  /// In en, this message translates to:
  /// **'Browser sign-in didn\'t finish — either the GitHub page wouldn\'t load, or the browser couldn\'t hand the result back to this app. The device code below avoids the hand-off entirely.'**
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

  /// No description provided for @connecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting…'**
  String get connecting;

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

  /// No description provided for @newConversationInProject.
  ///
  /// In en, this message translates to:
  /// **'New conversation in {project}'**
  String newConversationInProject(String project);

  /// No description provided for @copyProjectPath.
  ///
  /// In en, this message translates to:
  /// **'Copy project path'**
  String get copyProjectPath;

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

  /// No description provided for @renameConversation.
  ///
  /// In en, this message translates to:
  /// **'Rename'**
  String get renameConversation;

  /// No description provided for @renameConversationHint.
  ///
  /// In en, this message translates to:
  /// **'Conversation name'**
  String get renameConversationHint;

  /// No description provided for @renameHint.
  ///
  /// In en, this message translates to:
  /// **'Click to rename'**
  String get renameHint;

  /// No description provided for @retryingAttempt.
  ///
  /// In en, this message translates to:
  /// **'Retrying… {attempt}/{max}'**
  String retryingAttempt(int attempt, int max);

  /// No description provided for @renameFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t rename the conversation'**
  String get renameFailed;

  /// No description provided for @showMoreCount.
  ///
  /// In en, this message translates to:
  /// **'Show {count} more'**
  String showMoreCount(int count);

  /// No description provided for @showLess.
  ///
  /// In en, this message translates to:
  /// **'Show less'**
  String get showLess;

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

  /// No description provided for @voiceLive.
  ///
  /// In en, this message translates to:
  /// **'Live voice'**
  String get voiceLive;

  /// No description provided for @voiceSessionEnded.
  ///
  /// In en, this message translates to:
  /// **'Live voice · ended'**
  String get voiceSessionEnded;

  /// No description provided for @voiceYou.
  ///
  /// In en, this message translates to:
  /// **'You'**
  String get voiceYou;

  /// No description provided for @voiceAgent.
  ///
  /// In en, this message translates to:
  /// **'Agent'**
  String get voiceAgent;

  /// No description provided for @addAttachment.
  ///
  /// In en, this message translates to:
  /// **'Add'**
  String get addAttachment;

  /// No description provided for @turnSettings.
  ///
  /// In en, this message translates to:
  /// **'Turn settings'**
  String get turnSettings;

  /// No description provided for @prevTurn.
  ///
  /// In en, this message translates to:
  /// **'Previous turn'**
  String get prevTurn;

  /// No description provided for @nextTurn.
  ///
  /// In en, this message translates to:
  /// **'Next turn'**
  String get nextTurn;

  /// No description provided for @jumpToLatest.
  ///
  /// In en, this message translates to:
  /// **'Jump to latest'**
  String get jumpToLatest;

  /// No description provided for @attachImage.
  ///
  /// In en, this message translates to:
  /// **'Attach image'**
  String get attachImage;

  /// No description provided for @removeImage.
  ///
  /// In en, this message translates to:
  /// **'Remove image'**
  String get removeImage;

  /// No description provided for @imagePickFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t read the selected image'**
  String get imagePickFailed;

  /// No description provided for @imagesNotAccepted.
  ///
  /// In en, this message translates to:
  /// **'This agent doesn\'t accept images'**
  String get imagesNotAccepted;

  /// No description provided for @attachmentRevoked.
  ///
  /// In en, this message translates to:
  /// **'Not attached: the conversation changed while the file was read. Attach it again.'**
  String get attachmentRevoked;

  /// No description provided for @imageTooMany.
  ///
  /// In en, this message translates to:
  /// **'At most {count} images per message'**
  String imageTooMany(int count);

  /// No description provided for @imageSave.
  ///
  /// In en, this message translates to:
  /// **'Save image'**
  String get imageSave;

  /// No description provided for @previewImage.
  ///
  /// In en, this message translates to:
  /// **'Click to preview'**
  String get previewImage;

  /// No description provided for @imageLoadFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t load this image'**
  String get imageLoadFailed;

  /// No description provided for @imageSaved.
  ///
  /// In en, this message translates to:
  /// **'Saved to {path}'**
  String imageSaved(String path);

  /// No description provided for @imageSaveFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t save the image: {error}'**
  String imageSaveFailed(String error);

  /// No description provided for @imageOnHost.
  ///
  /// In en, this message translates to:
  /// **'Image file on the host machine: {path}'**
  String imageOnHost(String path);

  /// No description provided for @imageOnlyMessage.
  ///
  /// In en, this message translates to:
  /// **'[Image]'**
  String get imageOnlyMessage;

  /// No description provided for @attachFile.
  ///
  /// In en, this message translates to:
  /// **'Attach file'**
  String get attachFile;

  /// No description provided for @dropToAttach.
  ///
  /// In en, this message translates to:
  /// **'Drop to attach'**
  String get dropToAttach;

  /// No description provided for @queuedCount.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 message queued} other{{count} messages queued}}'**
  String queuedCount(int count);

  /// No description provided for @queuedHint.
  ///
  /// In en, this message translates to:
  /// **'Sends after the current reply. Esc puts the last one back.'**
  String get queuedHint;

  /// No description provided for @removeQueued.
  ///
  /// In en, this message translates to:
  /// **'Remove from queue'**
  String get removeQueued;

  /// No description provided for @removeFile.
  ///
  /// In en, this message translates to:
  /// **'Remove file'**
  String get removeFile;

  /// No description provided for @filePickFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t read the selected file'**
  String get filePickFailed;

  /// No description provided for @fileTooMany.
  ///
  /// In en, this message translates to:
  /// **'At most {count} files per message'**
  String fileTooMany(int count);

  /// No description provided for @fileTooLarge.
  ///
  /// In en, this message translates to:
  /// **'File exceeds the {mb} MB limit'**
  String fileTooLarge(int mb);

  /// No description provided for @fileOnlyMessage.
  ///
  /// In en, this message translates to:
  /// **'[File]'**
  String get fileOnlyMessage;

  /// No description provided for @emptyConversation.
  ///
  /// In en, this message translates to:
  /// **'Send a message to start the conversation'**
  String get emptyConversation;

  /// Leading row of a long conversation whose earlier history hasn't been loaded yet; scrolling to it fetches the previous page.
  ///
  /// In en, this message translates to:
  /// **'Load earlier messages'**
  String get olderHistoryHint;

  /// No description provided for @historyStart.
  ///
  /// In en, this message translates to:
  /// **'Start of conversation'**
  String get historyStart;

  /// No description provided for @historyLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading history…'**
  String get historyLoading;

  /// No description provided for @historyRetry.
  ///
  /// In en, this message translates to:
  /// **'Couldn’t load history. Retry'**
  String get historyRetry;

  /// No description provided for @turnFailed.
  ///
  /// In en, this message translates to:
  /// **'Turn didn\'t finish — the connection dropped or the remote codex failed. Retry, or check codex on the host machine (it may need to be logged in again).'**
  String get turnFailed;

  /// Shown when an embedded (in-process) codex turn fails because the OS sandbox helpers aren't available; guides the user to the no-sandbox Full mode or an external host.
  ///
  /// In en, this message translates to:
  /// **'This built-in (自带) session can\'t start its command sandbox, so the agent can\'t run commands or read files here. Switch to \"Full access\" mode to run without a sandbox, or connect to an external/remote codex host.'**
  String get sandboxHelperUnavailable;

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

  /// No description provided for @effortFaster.
  ///
  /// In en, this message translates to:
  /// **'Faster'**
  String get effortFaster;

  /// No description provided for @effortSmarter.
  ///
  /// In en, this message translates to:
  /// **'Smarter'**
  String get effortSmarter;

  /// No description provided for @searchProjects.
  ///
  /// In en, this message translates to:
  /// **'Search projects'**
  String get searchProjects;

  /// No description provided for @noMatchingProjects.
  ///
  /// In en, this message translates to:
  /// **'No matching projects'**
  String get noMatchingProjects;

  /// No description provided for @workOutsideProject.
  ///
  /// In en, this message translates to:
  /// **'Work outside a project'**
  String get workOutsideProject;

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
  /// **'Let Codex review approval requests for you, within the workspace sandbox.'**
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

  /// No description provided for @modelWithdrawn.
  ///
  /// In en, this message translates to:
  /// **'This host no longer offers the selected model. Choose a model and send again.'**
  String get modelWithdrawn;

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

  /// No description provided for @quotaRemaining.
  ///
  /// In en, this message translates to:
  /// **'Usage left'**
  String get quotaRemaining;

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

  /// No description provided for @backToSessions.
  ///
  /// In en, this message translates to:
  /// **'Back to sessions'**
  String get backToSessions;

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

  /// No description provided for @compactingContext.
  ///
  /// In en, this message translates to:
  /// **'Compacting context…'**
  String get compactingContext;

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

  /// No description provided for @modelErrorTitle.
  ///
  /// In en, this message translates to:
  /// **'The model couldn\'t respond'**
  String get modelErrorTitle;

  /// No description provided for @modelErrorModel.
  ///
  /// In en, this message translates to:
  /// **'Model: {model}'**
  String modelErrorModel(String model);

  /// No description provided for @modelErrorStatus.
  ///
  /// In en, this message translates to:
  /// **'HTTP {status}'**
  String modelErrorStatus(String status);

  /// No description provided for @modelErrorRegion.
  ///
  /// In en, this message translates to:
  /// **'This model isn\'t available in your region. Choose another model, then retry.'**
  String get modelErrorRegion;

  /// No description provided for @modelErrorAuthentication.
  ///
  /// In en, this message translates to:
  /// **'The provider couldn\'t sign you in. Check its API key or sign-in, then retry.'**
  String get modelErrorAuthentication;

  /// No description provided for @modelErrorPermission.
  ///
  /// In en, this message translates to:
  /// **'The provider denied access to this model. Choose another model or check your account.'**
  String get modelErrorPermission;

  /// No description provided for @modelErrorUnavailable.
  ///
  /// In en, this message translates to:
  /// **'The model or its endpoint is unavailable. Choose another model or check the provider configuration.'**
  String get modelErrorUnavailable;

  /// No description provided for @modelErrorRateLimited.
  ///
  /// In en, this message translates to:
  /// **'The provider is receiving too many requests. Wait a moment, then retry.'**
  String get modelErrorRateLimited;

  /// No description provided for @modelErrorQuota.
  ///
  /// In en, this message translates to:
  /// **'The provider\'s usage allowance is used up. Check your account or choose another model.'**
  String get modelErrorQuota;

  /// No description provided for @modelErrorContext.
  ///
  /// In en, this message translates to:
  /// **'The conversation is longer than this model accepts. Choose a model with a larger context or start a new conversation.'**
  String get modelErrorContext;

  /// No description provided for @modelErrorOutput.
  ///
  /// In en, this message translates to:
  /// **'The model reached its output limit before finishing. Ask it to continue.'**
  String get modelErrorOutput;

  /// No description provided for @modelErrorProvider.
  ///
  /// In en, this message translates to:
  /// **'The provider is temporarily unavailable. Retry later or choose another model.'**
  String get modelErrorProvider;

  /// No description provided for @modelErrorInvalid.
  ///
  /// In en, this message translates to:
  /// **'The provider rejected this request. Check the model and the message, then retry.'**
  String get modelErrorInvalid;

  /// No description provided for @modelErrorUnknown.
  ///
  /// In en, this message translates to:
  /// **'The model couldn\'t complete the response. Retry, or choose another model.'**
  String get modelErrorUnknown;

  /// No description provided for @modelErrorRetry.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get modelErrorRetry;

  /// No description provided for @modelErrorChangeModel.
  ///
  /// In en, this message translates to:
  /// **'Change model'**
  String get modelErrorChangeModel;

  /// No description provided for @modelErrorIncomplete.
  ///
  /// In en, this message translates to:
  /// **'The failed message isn\'t fully available here, so it can\'t be sent again as recorded. Edit it or send a new message.'**
  String get modelErrorIncomplete;

  /// No description provided for @modelChangeFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t switch the model: {reason}. The previous model stays selected and your draft is unchanged.'**
  String modelChangeFailed(String reason);

  /// No description provided for @modelErrorRetryHint.
  ///
  /// In en, this message translates to:
  /// **'Sends this message again with the model selected now.'**
  String get modelErrorRetryHint;

  /// No description provided for @reasoningOutputNote.
  ///
  /// In en, this message translates to:
  /// **'The model returned this as reasoning, without a separate answer.'**
  String get reasoningOutputNote;

  /// No description provided for @queuedPausedHint.
  ///
  /// In en, this message translates to:
  /// **'Paused. Retry or send a message to continue.'**
  String get queuedPausedHint;

  /// No description provided for @turnElapsed.
  ///
  /// In en, this message translates to:
  /// **'Took {duration}'**
  String turnElapsed(String duration);

  /// Collapsed header for a turn's tool calls and reasoning.
  ///
  /// In en, this message translates to:
  /// **'Worked for {duration}'**
  String turnProcessed(String duration);

  /// Same header while the turn is still running, when no duration exists yet.
  ///
  /// In en, this message translates to:
  /// **'Working, {count} steps so far'**
  String turnProcessing(int count);

  /// No description provided for @turnActivityCount.
  ///
  /// In en, this message translates to:
  /// **'{count} steps'**
  String turnActivityCount(int count);

  /// No description provided for @completedAt.
  ///
  /// In en, this message translates to:
  /// **'Completed at {time}'**
  String completedAt(String time);

  /// No description provided for @activeModelTooltip.
  ///
  /// In en, this message translates to:
  /// **'Active model & settings — tap for details'**
  String get activeModelTooltip;

  /// No description provided for @runtimeSheetTitle.
  ///
  /// In en, this message translates to:
  /// **'Runtime configuration'**
  String get runtimeSheetTitle;

  /// No description provided for @runtimeEffortModelDefault.
  ///
  /// In en, this message translates to:
  /// **'Model default'**
  String get runtimeEffortModelDefault;

  /// No description provided for @runtimeCollabDefault.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get runtimeCollabDefault;

  /// No description provided for @runtimeConfirmedAt.
  ///
  /// In en, this message translates to:
  /// **'Confirmed by the server · {time}'**
  String runtimeConfirmedAt(String time);

  /// No description provided for @runtimeFromSnapshot.
  ///
  /// In en, this message translates to:
  /// **'From the server\'s session snapshot · {time}'**
  String runtimeFromSnapshot(String time);

  /// No description provided for @runtimeUnavailable.
  ///
  /// In en, this message translates to:
  /// **'This server doesn\'t report runtime settings; showing what the app sends.'**
  String get runtimeUnavailable;

  /// No description provided for @runtimeConfirmed.
  ///
  /// In en, this message translates to:
  /// **'Confirmed by the server'**
  String get runtimeConfirmed;

  /// No description provided for @runtimeUnconfirmed.
  ///
  /// In en, this message translates to:
  /// **'As sent by the app — the server didn\'t report back'**
  String get runtimeUnconfirmed;

  /// No description provided for @turnHandledBy.
  ///
  /// In en, this message translates to:
  /// **'Handled by {model}'**
  String turnHandledBy(String model);

  /// No description provided for @modelReroutedNote.
  ///
  /// In en, this message translates to:
  /// **'The server switched to this model mid-turn'**
  String get modelReroutedNote;

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

  /// No description provided for @unreachableAuthRejected.
  ///
  /// In en, this message translates to:
  /// **'The relay refused the connection: the authentication code is missing or no longer valid. The tunnel itself works and the remote host is not down — sign in again, or re-enter the relay key, under Manage services.'**
  String get unreachableAuthRejected;

  /// No description provided for @unreachableSilent.
  ///
  /// In en, this message translates to:
  /// **'The tunnel is up, but the remote app-server didn\'t answer before the timeout — it may still be starting, wedged, or gone.'**
  String get unreachableSilent;

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

  /// No description provided for @cancelDiffLoad.
  ///
  /// In en, this message translates to:
  /// **'Cancel loading changes'**
  String get cancelDiffLoad;

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

  /// No description provided for @envTitle.
  ///
  /// In en, this message translates to:
  /// **'Environment'**
  String get envTitle;

  /// No description provided for @envLocal.
  ///
  /// In en, this message translates to:
  /// **'Local'**
  String get envLocal;

  /// No description provided for @envSource.
  ///
  /// In en, this message translates to:
  /// **'Source'**
  String get envSource;

  /// No description provided for @envRefresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get envRefresh;

  /// No description provided for @diffTruncated.
  ///
  /// In en, this message translates to:
  /// **'{count} more lines not shown — copy the path to view the full diff'**
  String diffTruncated(int count);

  /// No description provided for @diffUnmodified.
  ///
  /// In en, this message translates to:
  /// **'{count} unmodified lines'**
  String diffUnmodified(int count);

  /// No description provided for @diffExpandFailed.
  ///
  /// In en, this message translates to:
  /// **'{count} unmodified lines (couldn\'t load)'**
  String diffExpandFailed(int count);

  /// No description provided for @reviewTitle.
  ///
  /// In en, this message translates to:
  /// **'Review'**
  String get reviewTitle;

  /// No description provided for @reviewNoFiles.
  ///
  /// In en, this message translates to:
  /// **'No changed files.'**
  String get reviewNoFiles;

  /// No description provided for @reviewPickFile.
  ///
  /// In en, this message translates to:
  /// **'Pick a file to review its changes.'**
  String get reviewPickFile;

  /// No description provided for @envFilesChanged.
  ///
  /// In en, this message translates to:
  /// **'{count} files changed'**
  String envFilesChanged(int count);

  /// No description provided for @turnProgressStep.
  ///
  /// In en, this message translates to:
  /// **'Step {current} of {total}'**
  String turnProgressStep(int current, int total);

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

  /// No description provided for @toolCollaborated.
  ///
  /// In en, this message translates to:
  /// **'Coordinated agents'**
  String get toolCollaborated;

  /// No description provided for @toolSubAgent.
  ///
  /// In en, this message translates to:
  /// **'Subagent activity'**
  String get toolSubAgent;

  /// No description provided for @toolViewedImage.
  ///
  /// In en, this message translates to:
  /// **'Viewed image'**
  String get toolViewedImage;

  /// No description provided for @toolGeneratedImage.
  ///
  /// In en, this message translates to:
  /// **'Generated image'**
  String get toolGeneratedImage;

  /// No description provided for @toolWaited.
  ///
  /// In en, this message translates to:
  /// **'Waited'**
  String get toolWaited;

  /// No description provided for @toolHook.
  ///
  /// In en, this message translates to:
  /// **'Hook prompt'**
  String get toolHook;

  /// No description provided for @toolEnteredReview.
  ///
  /// In en, this message translates to:
  /// **'Entered review'**
  String get toolEnteredReview;

  /// No description provided for @toolExitedReview.
  ///
  /// In en, this message translates to:
  /// **'Exited review'**
  String get toolExitedReview;

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

  /// No description provided for @relayNotConfigured.
  ///
  /// In en, this message translates to:
  /// **'(no relay configured)'**
  String get relayNotConfigured;

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

  /// No description provided for @utilityChat.
  ///
  /// In en, this message translates to:
  /// **'Conversation'**
  String get utilityChat;

  /// No description provided for @utilityPages.
  ///
  /// In en, this message translates to:
  /// **'Pages'**
  String get utilityPages;

  /// No description provided for @utilitySwitchPage.
  ///
  /// In en, this message translates to:
  /// **'Switch page'**
  String get utilitySwitchPage;

  /// No description provided for @settingsGeneral.
  ///
  /// In en, this message translates to:
  /// **'General'**
  String get settingsGeneral;

  /// No description provided for @settingsAccountConnection.
  ///
  /// In en, this message translates to:
  /// **'Account & connection'**
  String get settingsAccountConnection;

  /// No description provided for @settingsServicesSubscriptions.
  ///
  /// In en, this message translates to:
  /// **'Services & subscriptions'**
  String get settingsServicesSubscriptions;

  /// No description provided for @settingsAdvanced.
  ///
  /// In en, this message translates to:
  /// **'Advanced'**
  String get settingsAdvanced;

  /// No description provided for @settingsSelfHostedRelay.
  ///
  /// In en, this message translates to:
  /// **'Self-hosted relay fallback'**
  String get settingsSelfHostedRelay;

  /// No description provided for @settingsSelfHostedKey.
  ///
  /// In en, this message translates to:
  /// **'Self-hosted relay key'**
  String get settingsSelfHostedKey;

  /// No description provided for @settingsExportUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Configure a self-hosted relay and 32-byte key first.'**
  String get settingsExportUnavailable;

  /// No description provided for @settingsOperationFailed.
  ///
  /// In en, this message translates to:
  /// **'Operation failed: {error}'**
  String settingsOperationFailed(String error);

  /// No description provided for @settingsDiagnostics.
  ///
  /// In en, this message translates to:
  /// **'Diagnostics'**
  String get settingsDiagnostics;

  /// No description provided for @settingsConfigure.
  ///
  /// In en, this message translates to:
  /// **'Configure'**
  String get settingsConfigure;

  /// No description provided for @settingsEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get settingsEdit;

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

  /// No description provided for @windowMinimize.
  ///
  /// In en, this message translates to:
  /// **'Minimize'**
  String get windowMinimize;

  /// No description provided for @windowMaximize.
  ///
  /// In en, this message translates to:
  /// **'Maximize'**
  String get windowMaximize;

  /// No description provided for @windowRestore.
  ///
  /// In en, this message translates to:
  /// **'Restore'**
  String get windowRestore;

  /// No description provided for @windowClose.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get windowClose;

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

  /// No description provided for @appearance.
  ///
  /// In en, this message translates to:
  /// **'Appearance'**
  String get appearance;

  /// No description provided for @appearanceSystem.
  ///
  /// In en, this message translates to:
  /// **'Follow system'**
  String get appearanceSystem;

  /// No description provided for @appearanceLight.
  ///
  /// In en, this message translates to:
  /// **'Light'**
  String get appearanceLight;

  /// No description provided for @appearanceDark.
  ///
  /// In en, this message translates to:
  /// **'Dark'**
  String get appearanceDark;

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

  /// No description provided for @groupYesterday.
  ///
  /// In en, this message translates to:
  /// **'Yesterday'**
  String get groupYesterday;

  /// No description provided for @groupEarlier.
  ///
  /// In en, this message translates to:
  /// **'Earlier'**
  String get groupEarlier;

  /// No description provided for @activityView.
  ///
  /// In en, this message translates to:
  /// **'Activity'**
  String get activityView;

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

  /// No description provided for @noLocalSessions.
  ///
  /// In en, this message translates to:
  /// **'No local sessions'**
  String get noLocalSessions;

  /// No description provided for @localSessionsUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Local sessions aren’t available on this device'**
  String get localSessionsUnavailable;

  /// No description provided for @localSessionsMobileHint.
  ///
  /// In en, this message translates to:
  /// **'This page reads Codex sessions saved on this device. To browse your computer’s sessions, connect to it in Services. Cached conversations remain in Chats.'**
  String get localSessionsMobileHint;

  /// No description provided for @noLocalSessionsHint.
  ///
  /// In en, this message translates to:
  /// **'This computer has no readable Codex sessions yet. Create a session with Codex, then refresh, or connect to another device.'**
  String get noLocalSessionsHint;

  /// No description provided for @noHostSessions.
  ///
  /// In en, this message translates to:
  /// **'No sessions on this host'**
  String get noHostSessions;

  /// No description provided for @noHostSessionsHint.
  ///
  /// In en, this message translates to:
  /// **'This host has no sessions to display yet. Create a Codex session on it, then refresh.'**
  String get noHostSessionsHint;

  /// No description provided for @sessionsReadFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn’t read sessions'**
  String get sessionsReadFailed;

  /// No description provided for @localSessionsReadHint.
  ///
  /// In en, this message translates to:
  /// **'Check that Codex has saved sessions for this user and that its session folder is readable, then retry.'**
  String get localSessionsReadHint;

  /// No description provided for @remoteSessionsReadHint.
  ///
  /// In en, this message translates to:
  /// **'The host’s sessions are temporarily unavailable. Check its connection, then retry.'**
  String get remoteSessionsReadHint;

  /// No description provided for @errorDetails.
  ///
  /// In en, this message translates to:
  /// **'Error details'**
  String get errorDetails;

  /// No description provided for @moveTurnNavigation.
  ///
  /// In en, this message translates to:
  /// **'Drag to move conversation controls'**
  String get moveTurnNavigation;

  /// No description provided for @historyLoaded.
  ///
  /// In en, this message translates to:
  /// **'Loaded {count} earlier items'**
  String historyLoaded(int count);

  /// No description provided for @historyAlreadyVisible.
  ///
  /// In en, this message translates to:
  /// **'This history is already displayed'**
  String get historyAlreadyVisible;

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
  /// **'This session is held open by {n} other process(es). PocketCodex will try to terminate them, then resume it here. Any unsaved work in those processes will be lost.'**
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

  /// No description provided for @localHostUnresponsive.
  ///
  /// In en, this message translates to:
  /// **'Not responding'**
  String get localHostUnresponsive;

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

  /// No description provided for @localHostStopFailed.
  ///
  /// In en, this message translates to:
  /// **'Did not stop: stop again'**
  String get localHostStopFailed;

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

  /// No description provided for @codexSourceExternal.
  ///
  /// In en, this message translates to:
  /// **'External Codex'**
  String get codexSourceExternal;

  /// No description provided for @codexSourceBuiltin.
  ///
  /// In en, this message translates to:
  /// **'Built-in engine'**
  String get codexSourceBuiltin;

  /// No description provided for @codexBuiltinNote.
  ///
  /// In en, this message translates to:
  /// **'Not implemented yet. Use external Codex.'**
  String get codexBuiltinNote;

  /// No description provided for @codexBuiltinUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Not implemented yet'**
  String get codexBuiltinUnavailable;

  /// No description provided for @hostRuntimeInfo.
  ///
  /// In en, this message translates to:
  /// **'Runtime'**
  String get hostRuntimeInfo;

  /// No description provided for @hostCodexPath.
  ///
  /// In en, this message translates to:
  /// **'Path'**
  String get hostCodexPath;

  /// No description provided for @hostProxyLabel.
  ///
  /// In en, this message translates to:
  /// **'Proxy'**
  String get hostProxyLabel;

  /// No description provided for @hostProxyInherit.
  ///
  /// In en, this message translates to:
  /// **'inherits the app\'s environment'**
  String get hostProxyInherit;

  /// No description provided for @localHostDialogTitle.
  ///
  /// In en, this message translates to:
  /// **'Host on this device'**
  String get localHostDialogTitle;

  /// No description provided for @localHostHint.
  ///
  /// In en, this message translates to:
  /// **'Runs codex on this machine and publishes it on your relay, so your other devices can drive it.'**
  String get localHostHint;

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

  /// No description provided for @deregisterOrphanTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove this unreachable service?'**
  String get deregisterOrphanTitle;

  /// No description provided for @deregisterOrphanWarning.
  ///
  /// In en, this message translates to:
  /// **'“{name}” isn\'t responding — its registration is lingering on the relay with no reachable host. This removes it from your list (and keeps it hidden even if the stale registration lingers), and asks the backend to drop it.'**
  String deregisterOrphanWarning(String name);

  /// No description provided for @remove.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get remove;

  /// No description provided for @deregisterFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t deregister the service'**
  String get deregisterFailed;

  /// No description provided for @hostNameConflict.
  ///
  /// In en, this message translates to:
  /// **'This server name is already online from another instance. Stop that instance first, or host under a different name. (A just-stopped server frees its name within ~15 seconds.)'**
  String get hostNameConflict;

  /// No description provided for @batchRemoveEnter.
  ///
  /// In en, this message translates to:
  /// **'Clean up unreachable'**
  String get batchRemoveEnter;

  /// No description provided for @batchRemoveTitle.
  ///
  /// In en, this message translates to:
  /// **'Remove these unreachable services?'**
  String get batchRemoveTitle;

  /// No description provided for @batchRemoveWarning.
  ///
  /// In en, this message translates to:
  /// **'Remove {count} unreachable services from this device\'s list. Any that recover will re-appear.'**
  String batchRemoveWarning(int count);

  /// No description provided for @batchRemovedSnack.
  ///
  /// In en, this message translates to:
  /// **'Removed {count} services'**
  String batchRemovedSnack(int count);

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

  /// No description provided for @tunnelOffline.
  ///
  /// In en, this message translates to:
  /// **'Offline'**
  String get tunnelOffline;

  /// No description provided for @tunnelNotConnected.
  ///
  /// In en, this message translates to:
  /// **'Not connected'**
  String get tunnelNotConnected;

  /// No description provided for @logsTitle.
  ///
  /// In en, this message translates to:
  /// **'Runtime logs'**
  String get logsTitle;

  /// No description provided for @logsLive.
  ///
  /// In en, this message translates to:
  /// **'Live capture'**
  String get logsLive;

  /// No description provided for @logsLevelAll.
  ///
  /// In en, this message translates to:
  /// **'All'**
  String get logsLevelAll;

  /// No description provided for @logsKeywordHint.
  ///
  /// In en, this message translates to:
  /// **'error, host, tunnel…'**
  String get logsKeywordHint;

  /// No description provided for @logsCopy.
  ///
  /// In en, this message translates to:
  /// **'Copy visible logs'**
  String get logsCopy;

  /// No description provided for @logsClear.
  ///
  /// In en, this message translates to:
  /// **'Clear logs'**
  String get logsClear;

  /// No description provided for @logsScrollBottom.
  ///
  /// In en, this message translates to:
  /// **'Scroll to bottom'**
  String get logsScrollBottom;

  /// No description provided for @logsEmpty.
  ///
  /// In en, this message translates to:
  /// **'No logs yet'**
  String get logsEmpty;

  /// No description provided for @logsVisible.
  ///
  /// In en, this message translates to:
  /// **'{visible}/{total} shown'**
  String logsVisible(int visible, int total);

  /// No description provided for @logsCopied.
  ///
  /// In en, this message translates to:
  /// **'Copied {count} log lines'**
  String logsCopied(int count);

  /// No description provided for @manageServices.
  ///
  /// In en, this message translates to:
  /// **'Manage services'**
  String get manageServices;

  /// No description provided for @servicesHostThisDevice.
  ///
  /// In en, this message translates to:
  /// **'Host this device'**
  String get servicesHostThisDevice;

  /// No description provided for @servicesDevices.
  ///
  /// In en, this message translates to:
  /// **'Devices'**
  String get servicesDevices;

  /// No description provided for @servicesDeviceCapabilityCount.
  ///
  /// In en, this message translates to:
  /// **'{count} capabilities'**
  String servicesDeviceCapabilityCount(int count);

  /// No description provided for @servicesLocalDevice.
  ///
  /// In en, this message translates to:
  /// **'This device'**
  String get servicesLocalDevice;

  /// No description provided for @servicesPlatformUnknown.
  ///
  /// In en, this message translates to:
  /// **'Platform unknown'**
  String get servicesPlatformUnknown;

  /// No description provided for @servicesPublisherInstance.
  ///
  /// In en, this message translates to:
  /// **'Publisher instance'**
  String get servicesPublisherInstance;

  /// No description provided for @servicesHostInstance.
  ///
  /// In en, this message translates to:
  /// **'Hosting instance'**
  String get servicesHostInstance;

  /// No description provided for @servicesCapabilities.
  ///
  /// In en, this message translates to:
  /// **'Available capabilities'**
  String get servicesCapabilities;

  /// No description provided for @servicesChatCapability.
  ///
  /// In en, this message translates to:
  /// **'Conversation service'**
  String get servicesChatCapability;

  /// No description provided for @servicesApiCapability.
  ///
  /// In en, this message translates to:
  /// **'Responses API'**
  String get servicesApiCapability;

  /// No description provided for @servicesSessionsCapability.
  ///
  /// In en, this message translates to:
  /// **'Session sharing'**
  String get servicesSessionsCapability;

  /// No description provided for @servicesDefault.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get servicesDefault;

  /// No description provided for @servicesSetDefault.
  ///
  /// In en, this message translates to:
  /// **'Set as default'**
  String get servicesSetDefault;

  /// No description provided for @servicesOpen.
  ///
  /// In en, this message translates to:
  /// **'Open'**
  String get servicesOpen;

  /// No description provided for @servicesManage.
  ///
  /// In en, this message translates to:
  /// **'Manage'**
  String get servicesManage;

  /// No description provided for @servicesBrowse.
  ///
  /// In en, this message translates to:
  /// **'Browse'**
  String get servicesBrowse;

  /// No description provided for @servicesNoCapabilities.
  ///
  /// In en, this message translates to:
  /// **'No capabilities are currently available on this device'**
  String get servicesNoCapabilities;

  /// No description provided for @hostSessions.
  ///
  /// In en, this message translates to:
  /// **'Host session history'**
  String get hostSessions;

  /// No description provided for @homeConnecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting to your Codex host…'**
  String get homeConnecting;

  /// No description provided for @homeRestoringHost.
  ///
  /// In en, this message translates to:
  /// **'Restoring hosting…'**
  String get homeRestoringHost;

  /// No description provided for @homeNoServiceTitle.
  ///
  /// In en, this message translates to:
  /// **'No Codex host available'**
  String get homeNoServiceTitle;

  /// No description provided for @homeNoServiceDesktopHint.
  ///
  /// In en, this message translates to:
  /// **'Start hosting on this computer and the chat opens right here — your phone can connect to it too.'**
  String get homeNoServiceDesktopHint;

  /// No description provided for @homeNoServiceMobileHint.
  ///
  /// In en, this message translates to:
  /// **'Open PocketCodex on your computer and start hosting (app-server); this screen will enter the chat automatically.'**
  String get homeNoServiceMobileHint;

  /// No description provided for @homeAutoRetryNote.
  ///
  /// In en, this message translates to:
  /// **'Keeps checking for hosts automatically.'**
  String get homeAutoRetryNote;

  /// No description provided for @switchServiceFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t reach that host — staying on the current one.'**
  String get switchServiceFailed;

  /// No description provided for @hostSwitchTitle.
  ///
  /// In en, this message translates to:
  /// **'Switch host'**
  String get hostSwitchTitle;

  /// No description provided for @acpSavedAgentsFull.
  ///
  /// In en, this message translates to:
  /// **'This configuration was not saved: {count} configurations are already saved.'**
  String acpSavedAgentsFull(int count);

  /// No description provided for @hostSwitchManage.
  ///
  /// In en, this message translates to:
  /// **'Manage hosts…'**
  String get hostSwitchManage;

  /// No description provided for @agentChoiceNative.
  ///
  /// In en, this message translates to:
  /// **'Native app-server'**
  String get agentChoiceNative;

  /// No description provided for @agentChoiceSaved.
  ///
  /// In en, this message translates to:
  /// **'Saved · ACP'**
  String get agentChoiceSaved;

  /// No description provided for @agentChoicePreset.
  ///
  /// In en, this message translates to:
  /// **'Preset · ACP'**
  String get agentChoicePreset;

  /// No description provided for @agentChoiceNew.
  ///
  /// In en, this message translates to:
  /// **'New configuration'**
  String get agentChoiceNew;

  /// No description provided for @agentChoiceBuiltinTitle.
  ///
  /// In en, this message translates to:
  /// **'Built-in presets'**
  String get agentChoiceBuiltinTitle;

  /// No description provided for @agentChoiceBuiltinNote.
  ///
  /// In en, this message translates to:
  /// **'Default launch settings. The agent runs from its installation on this device.'**
  String get agentChoiceBuiltinNote;

  /// No description provided for @agentChoiceSavedTitle.
  ///
  /// In en, this message translates to:
  /// **'Saved configurations'**
  String get agentChoiceSavedTitle;

  /// No description provided for @agentChoiceSavedNote.
  ///
  /// In en, this message translates to:
  /// **'Reuse launch settings you saved, or create a new configuration.'**
  String get agentChoiceSavedNote;

  /// No description provided for @agentChoiceCustomTitle.
  ///
  /// In en, this message translates to:
  /// **'Custom configuration'**
  String get agentChoiceCustomTitle;

  /// No description provided for @agentChoiceCustomNote.
  ///
  /// In en, this message translates to:
  /// **'Set the program and arguments yourself, and save them for next time.'**
  String get agentChoiceCustomNote;

  /// No description provided for @hostNameHelper.
  ///
  /// In en, this message translates to:
  /// **'Unique on this device; it names the host on your relay.'**
  String get hostNameHelper;

  /// No description provided for @hostAdvanced.
  ///
  /// In en, this message translates to:
  /// **'Advanced'**
  String get hostAdvanced;

  /// No description provided for @localHostPortInvalid.
  ///
  /// In en, this message translates to:
  /// **'Enter a port from 0 to 65535 (0 picks a free port).'**
  String get localHostPortInvalid;

  /// No description provided for @localHostPortHelper.
  ///
  /// In en, this message translates to:
  /// **'0 picks a free port automatically.'**
  String get localHostPortHelper;

  /// No description provided for @hostPortConflict.
  ///
  /// In en, this message translates to:
  /// **'Another host on this device already uses this port. Use 0 for a free port, or pick another.'**
  String get hostPortConflict;

  /// No description provided for @hostSwitchingTo.
  ///
  /// In en, this message translates to:
  /// **'Switching to {host}…'**
  String hostSwitchingTo(String host);

  /// No description provided for @hostSwitchProbing.
  ///
  /// In en, this message translates to:
  /// **'Checking that the host is answering…'**
  String get hostSwitchProbing;

  /// No description provided for @hostSwitchConnecting.
  ///
  /// In en, this message translates to:
  /// **'Opening the connection…'**
  String get hostSwitchConnecting;

  /// No description provided for @hostSwitchLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading its conversations…'**
  String get hostSwitchLoading;

  /// No description provided for @hostSwitchFailedTitle.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t switch to {host}'**
  String hostSwitchFailedTitle(String host);

  /// No description provided for @hostSwitchStayed.
  ///
  /// In en, this message translates to:
  /// **'You\'re still on {host}.'**
  String hostSwitchStayed(String host);

  /// No description provided for @hostSwitched.
  ///
  /// In en, this message translates to:
  /// **'Now on {host}'**
  String hostSwitched(String host);

  /// No description provided for @hostSwitchLastFailed.
  ///
  /// In en, this message translates to:
  /// **'Last attempt failed'**
  String get hostSwitchLastFailed;

  /// No description provided for @hostInUse.
  ///
  /// In en, this message translates to:
  /// **'{name} · in use'**
  String hostInUse(String name);

  /// No description provided for @hostCurrent.
  ///
  /// In en, this message translates to:
  /// **'Host: {host}'**
  String hostCurrent(String host);

  /// No description provided for @hostSwitchHint.
  ///
  /// In en, this message translates to:
  /// **'Host: {host}. Switch host'**
  String hostSwitchHint(String host);

  /// No description provided for @pickFolderTitle.
  ///
  /// In en, this message translates to:
  /// **'Choose a project folder'**
  String get pickFolderTitle;

  /// No description provided for @useThisFolder.
  ///
  /// In en, this message translates to:
  /// **'Use this folder'**
  String get useThisFolder;

  /// No description provided for @folderUp.
  ///
  /// In en, this message translates to:
  /// **'Up one level'**
  String get folderUp;

  /// No description provided for @folderPickerNoRoots.
  ///
  /// In en, this message translates to:
  /// **'No project folders are configured on this host yet. Add them on the computer under hosting settings.'**
  String get folderPickerNoRoots;

  /// No description provided for @gitRepoLabel.
  ///
  /// In en, this message translates to:
  /// **'Git repository'**
  String get gitRepoLabel;

  /// No description provided for @hostFiles.
  ///
  /// In en, this message translates to:
  /// **'Host files'**
  String get hostFiles;

  /// No description provided for @fileBrowserEmpty.
  ///
  /// In en, this message translates to:
  /// **'This folder is empty'**
  String get fileBrowserEmpty;

  /// No description provided for @fileDownload.
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get fileDownload;

  /// No description provided for @fileUpload.
  ///
  /// In en, this message translates to:
  /// **'Upload'**
  String get fileUpload;

  /// No description provided for @fileDownloaded.
  ///
  /// In en, this message translates to:
  /// **'Downloaded to {path}'**
  String fileDownloaded(Object path);

  /// No description provided for @fileDownloadFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t download: {error}'**
  String fileDownloadFailed(Object error);

  /// No description provided for @fileUploaded.
  ///
  /// In en, this message translates to:
  /// **'Uploaded {name}'**
  String fileUploaded(Object name);

  /// No description provided for @hostFileUploadFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t upload: {error}'**
  String hostFileUploadFailed(Object error);

  /// No description provided for @browseProjectFolder.
  ///
  /// In en, this message translates to:
  /// **'Browse project folders'**
  String get browseProjectFolder;

  /// No description provided for @orEnterPathManually.
  ///
  /// In en, this message translates to:
  /// **'Or enter a path manually:'**
  String get orEnterPathManually;

  /// No description provided for @projectFolders.
  ///
  /// In en, this message translates to:
  /// **'Project folders'**
  String get projectFolders;

  /// No description provided for @addProjectFolder.
  ///
  /// In en, this message translates to:
  /// **'Add folder'**
  String get addProjectFolder;

  /// No description provided for @removeProjectFolder.
  ///
  /// In en, this message translates to:
  /// **'Remove'**
  String get removeProjectFolder;

  /// No description provided for @welcomeTitle.
  ///
  /// In en, this message translates to:
  /// **'Welcome to PocketCodex'**
  String get welcomeTitle;

  /// No description provided for @welcomeSubtitleDesktop.
  ///
  /// In en, this message translates to:
  /// **'One step left: start hosting on this computer, and your phone can drive its Codex from anywhere.'**
  String get welcomeSubtitleDesktop;

  /// No description provided for @welcomeSubtitleMobile.
  ///
  /// In en, this message translates to:
  /// **'PocketCodex remote-controls the Codex running on your computer. Set up hosting there once and you\'re ready.'**
  String get welcomeSubtitleMobile;

  /// No description provided for @welcomeStepHost.
  ///
  /// In en, this message translates to:
  /// **'Start hosting on this computer'**
  String get welcomeStepHost;

  /// No description provided for @welcomeStepHostDesc.
  ///
  /// In en, this message translates to:
  /// **'One click brings up a codex app-server and publishes it to your account — the defaults just work.'**
  String get welcomeStepHostDesc;

  /// No description provided for @welcomeHostRunning.
  ///
  /// In en, this message translates to:
  /// **'Hosting is up'**
  String get welcomeHostRunning;

  /// No description provided for @welcomeStepFolders.
  ///
  /// In en, this message translates to:
  /// **'Configure project folders (optional)'**
  String get welcomeStepFolders;

  /// No description provided for @welcomeStepFoldersDesc.
  ///
  /// In en, this message translates to:
  /// **'Your phone browses these folders visually to start a session in the right project.'**
  String get welcomeStepFoldersDesc;

  /// No description provided for @welcomeFoldersLocked.
  ///
  /// In en, this message translates to:
  /// **'Available once hosting is up.'**
  String get welcomeFoldersLocked;

  /// No description provided for @welcomeMobileStep1.
  ///
  /// In en, this message translates to:
  /// **'Install and open PocketCodex on your computer'**
  String get welcomeMobileStep1;

  /// No description provided for @welcomeMobileStep2.
  ///
  /// In en, this message translates to:
  /// **'Sign in with the same GitHub account'**
  String get welcomeMobileStep2;

  /// No description provided for @welcomeMobileStep3.
  ///
  /// In en, this message translates to:
  /// **'Tap \"Start hosting\" — one click completes the setup'**
  String get welcomeMobileStep3;

  /// No description provided for @welcomeWaitingHost.
  ///
  /// In en, this message translates to:
  /// **'Watching for your host to come online…'**
  String get welcomeWaitingHost;

  /// No description provided for @welcomeHostFound.
  ///
  /// In en, this message translates to:
  /// **'Host found: {label}'**
  String welcomeHostFound(String label);

  /// No description provided for @welcomeDownloadDesktop.
  ///
  /// In en, this message translates to:
  /// **'Get the desktop app'**
  String get welcomeDownloadDesktop;

  /// No description provided for @welcomeEnterChat.
  ///
  /// In en, this message translates to:
  /// **'Enter the chat'**
  String get welcomeEnterChat;

  /// No description provided for @welcomeSkip.
  ///
  /// In en, this message translates to:
  /// **'Skip the guide'**
  String get welcomeSkip;

  /// No description provided for @codexSetup.
  ///
  /// In en, this message translates to:
  /// **'Configure Codex'**
  String get codexSetup;

  /// No description provided for @codexChatNeedsSetup.
  ///
  /// In en, this message translates to:
  /// **'Model access isn\'t set up yet — set this host\'s model service before chatting.'**
  String get codexChatNeedsSetup;

  /// No description provided for @codexSetupStepTitle.
  ///
  /// In en, this message translates to:
  /// **'Configure Codex model access'**
  String get codexSetupStepTitle;

  /// No description provided for @codexSetupStepDesc.
  ///
  /// In en, this message translates to:
  /// **'Sign in with ChatGPT, or give a host its own provider when you host it, and optionally switch on the non-degraded system prompt.'**
  String get codexSetupStepDesc;

  /// No description provided for @codexSetupProviderSection.
  ///
  /// In en, this message translates to:
  /// **'Custom provider'**
  String get codexSetupProviderSection;

  /// No description provided for @codexSetupLoginSection.
  ///
  /// In en, this message translates to:
  /// **'ChatGPT sign-in'**
  String get codexSetupLoginSection;

  /// No description provided for @codexSetupLoginDesc.
  ///
  /// In en, this message translates to:
  /// **'Run the model on your ChatGPT subscription; Codex creates the credential itself.'**
  String get codexSetupLoginDesc;

  /// No description provided for @codexSetupLoginButton.
  ///
  /// In en, this message translates to:
  /// **'Sign in with ChatGPT'**
  String get codexSetupLoginButton;

  /// No description provided for @codexSetupLoginWaiting.
  ///
  /// In en, this message translates to:
  /// **'Waiting for the browser login…'**
  String get codexSetupLoginWaiting;

  /// No description provided for @codexSetupMethodsTitle.
  ///
  /// In en, this message translates to:
  /// **'Model access'**
  String get codexSetupMethodsTitle;

  /// No description provided for @codexSetupMethodsHint.
  ///
  /// In en, this message translates to:
  /// **'If Codex\'s own configuration selects a custom provider, that provider is used instead of the ChatGPT sign-in.'**
  String get codexSetupMethodsHint;

  /// No description provided for @codexSetupInUse.
  ///
  /// In en, this message translates to:
  /// **'In use'**
  String get codexSetupInUse;

  /// No description provided for @codexSetupProxyNote.
  ///
  /// In en, this message translates to:
  /// **'On a restricted network, sign-in needs a proxy: set one in a host\'s editor and sign in there.'**
  String get codexSetupProxyNote;

  /// No description provided for @codexSetupAdvanced.
  ///
  /// In en, this message translates to:
  /// **'Advanced'**
  String get codexSetupAdvanced;

  /// No description provided for @codexSetupNonDegradedTitle.
  ///
  /// In en, this message translates to:
  /// **'Non-degraded system prompt'**
  String get codexSetupNonDegradedTitle;

  /// No description provided for @codexSetupNonDegradedDesc.
  ///
  /// In en, this message translates to:
  /// **'Removes the commentary / progress-report instructions that interrupt the reasoning chain, so the model spends its budget on reasoning.'**
  String get codexSetupNonDegradedDesc;

  /// No description provided for @codexSetupNonDegradedScope.
  ///
  /// In en, this message translates to:
  /// **'Applies to new conversations'**
  String get codexSetupNonDegradedScope;

  /// No description provided for @codexSetupNonDegradedIssue.
  ///
  /// In en, this message translates to:
  /// **'openai/codex#30364'**
  String get codexSetupNonDegradedIssue;

  /// No description provided for @codexSetupLoginOpened.
  ///
  /// In en, this message translates to:
  /// **'Opened the login page in your browser; completion is detected automatically…'**
  String get codexSetupLoginOpened;

  /// No description provided for @codexSetupDeviceOpened.
  ///
  /// In en, this message translates to:
  /// **'Opened the verification page in your browser. Enter the code below there; completion is detected automatically…'**
  String get codexSetupDeviceOpened;

  /// No description provided for @codexSetupDeviceCodeLabel.
  ///
  /// In en, this message translates to:
  /// **'Verification code'**
  String get codexSetupDeviceCodeLabel;

  /// No description provided for @codexSetupDeviceReopen.
  ///
  /// In en, this message translates to:
  /// **'Reopen verification page'**
  String get codexSetupDeviceReopen;

  /// No description provided for @codexSetupLoginSuccess.
  ///
  /// In en, this message translates to:
  /// **'Signed in.'**
  String get codexSetupLoginSuccess;

  /// No description provided for @codexSetupLoginTimeout.
  ///
  /// In en, this message translates to:
  /// **'Login timed out, please retry.'**
  String get codexSetupLoginTimeout;

  /// No description provided for @codexSetupStatusProvider.
  ///
  /// In en, this message translates to:
  /// **'Custom provider configured'**
  String get codexSetupStatusProvider;

  /// No description provided for @codexSetupStatusNeedSetup.
  ///
  /// In en, this message translates to:
  /// **'Not configured yet'**
  String get codexSetupStatusNeedSetup;

  /// No description provided for @codexSetupStatusNeedSetupHint.
  ///
  /// In en, this message translates to:
  /// **'Codex cannot make a model call yet. Set up either method below.'**
  String get codexSetupStatusNeedSetupHint;

  /// No description provided for @codexSetupStatusReadyHint.
  ///
  /// In en, this message translates to:
  /// **'Codex can make model calls.'**
  String get codexSetupStatusReadyHint;

  /// No description provided for @codexSetupCodexHomeLabel.
  ///
  /// In en, this message translates to:
  /// **'Config directory'**
  String get codexSetupCodexHomeLabel;

  /// No description provided for @codexSignedInChatgpt.
  ///
  /// In en, this message translates to:
  /// **'Signed in with ChatGPT'**
  String get codexSignedInChatgpt;

  /// No description provided for @codexSignedInApiKey.
  ///
  /// In en, this message translates to:
  /// **'Using an API key'**
  String get codexSignedInApiKey;

  /// No description provided for @codexSignedIn.
  ///
  /// In en, this message translates to:
  /// **'Signed in'**
  String get codexSignedIn;

  /// No description provided for @codexSetupStatusInvalid.
  ///
  /// In en, this message translates to:
  /// **'Codex\'s config.toml could not be read'**
  String get codexSetupStatusInvalid;

  /// No description provided for @codexSetupStatusInvalidHint.
  ///
  /// In en, this message translates to:
  /// **'Fix the file before hosts on \"Existing configuration\" can use it.'**
  String get codexSetupStatusInvalidHint;

  /// No description provided for @codexSetupProviderPerHost.
  ///
  /// In en, this message translates to:
  /// **'Each host can use its own custom provider, chosen in that host\'s editor.'**
  String get codexSetupProviderPerHost;

  /// No description provided for @codexSetupProviderScope.
  ///
  /// In en, this message translates to:
  /// **'This page sets up Codex itself (the config directory above): the ChatGPT sign-in and the prompt option. Hosts set to \"Existing configuration\" use them; a host\'s own custom provider or official account is chosen in that host\'s editor.'**
  String get codexSetupProviderScope;

  /// No description provided for @codexSetupGlobalProvider.
  ///
  /// In en, this message translates to:
  /// **'Codex\'s own configuration already selects a custom provider; hosts set to \"Existing configuration\" keep using it.'**
  String get codexSetupGlobalProvider;

  /// No description provided for @codexSetupConfigureHost.
  ///
  /// In en, this message translates to:
  /// **'Configure a host'**
  String get codexSetupConfigureHost;

  /// No description provided for @codexSetupHostingDesktopOnly.
  ///
  /// In en, this message translates to:
  /// **'Hosting runs on a desktop; set a host\'s provider there.'**
  String get codexSetupHostingDesktopOnly;

  /// No description provided for @codexSetupLoginDesktopOnly.
  ///
  /// In en, this message translates to:
  /// **'Sign in on the computer where Codex is installed.'**
  String get codexSetupLoginDesktopOnly;

  /// No description provided for @settingsHosting.
  ///
  /// In en, this message translates to:
  /// **'Hosting and model service'**
  String get settingsHosting;

  /// No description provided for @settingsHostingDesc.
  ///
  /// In en, this message translates to:
  /// **'A host\'s agent, model service and sign-in'**
  String get settingsHostingDesc;

  /// No description provided for @hostPickTitle.
  ///
  /// In en, this message translates to:
  /// **'Configure which host?'**
  String get hostPickTitle;

  /// No description provided for @hostPickHint.
  ///
  /// In en, this message translates to:
  /// **'Each host keeps its own model service. Choosing one only opens its editor. Save keeps a running host as it is; its saved configuration takes effect only after Apply and restart.'**
  String get hostPickHint;

  /// No description provided for @hostPickNew.
  ///
  /// In en, this message translates to:
  /// **'New host'**
  String get hostPickNew;

  /// No description provided for @hostPickLoadFailed.
  ///
  /// In en, this message translates to:
  /// **'The running hosts could not be read. You can still set up a new host.'**
  String get hostPickLoadFailed;

  /// No description provided for @resizeComposer.
  ///
  /// In en, this message translates to:
  /// **'Drag to resize; double-click to reset'**
  String get resizeComposer;

  /// No description provided for @historyGap.
  ///
  /// In en, this message translates to:
  /// **'Load messages in this gap'**
  String get historyGap;

  /// No description provided for @reviewRequest.
  ///
  /// In en, this message translates to:
  /// **'Automatic review request'**
  String get reviewRequest;

  /// No description provided for @reviewResult.
  ///
  /// In en, this message translates to:
  /// **'Model review result'**
  String get reviewResult;

  /// No description provided for @reviewAllowed.
  ///
  /// In en, this message translates to:
  /// **'Allowed'**
  String get reviewAllowed;

  /// No description provided for @reviewDenied.
  ///
  /// In en, this message translates to:
  /// **'Denied'**
  String get reviewDenied;

  /// No description provided for @reviewLow.
  ///
  /// In en, this message translates to:
  /// **'Low'**
  String get reviewLow;

  /// No description provided for @reviewMedium.
  ///
  /// In en, this message translates to:
  /// **'Medium'**
  String get reviewMedium;

  /// No description provided for @reviewHigh.
  ///
  /// In en, this message translates to:
  /// **'High'**
  String get reviewHigh;

  /// No description provided for @reviewCritical.
  ///
  /// In en, this message translates to:
  /// **'Critical'**
  String get reviewCritical;

  /// No description provided for @reviewUnknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get reviewUnknown;

  /// No description provided for @reviewRisk.
  ///
  /// In en, this message translates to:
  /// **'Risk: {level}'**
  String reviewRisk(String level);

  /// No description provided for @reviewAuthorization.
  ///
  /// In en, this message translates to:
  /// **'Authorization: {level}'**
  String reviewAuthorization(String level);

  /// No description provided for @reviewRaw.
  ///
  /// In en, this message translates to:
  /// **'View original content'**
  String get reviewRaw;

  /// No description provided for @conversationOutline.
  ///
  /// In en, this message translates to:
  /// **'Conversation outline'**
  String get conversationOutline;

  /// No description provided for @searchTurns.
  ///
  /// In en, this message translates to:
  /// **'Search messages or enter a turn number'**
  String get searchTurns;

  /// No description provided for @turnPosition.
  ///
  /// In en, this message translates to:
  /// **'Turn {current} of {total}'**
  String turnPosition(int current, int total);

  /// No description provided for @stepRange.
  ///
  /// In en, this message translates to:
  /// **'Steps {first}–{last} of {total}'**
  String stepRange(int first, int last, int total);

  /// No description provided for @previousSteps.
  ///
  /// In en, this message translates to:
  /// **'Previous steps'**
  String get previousSteps;

  /// No description provided for @nextSteps.
  ///
  /// In en, this message translates to:
  /// **'Next steps'**
  String get nextSteps;

  /// No description provided for @latestSteps.
  ///
  /// In en, this message translates to:
  /// **'Latest steps'**
  String get latestSteps;

  /// No description provided for @jumpToStep.
  ///
  /// In en, this message translates to:
  /// **'Jump to step'**
  String get jumpToStep;

  /// No description provided for @stepNumber.
  ///
  /// In en, this message translates to:
  /// **'Step number'**
  String get stepNumber;

  /// No description provided for @noMatchingTurns.
  ///
  /// In en, this message translates to:
  /// **'No matching turns'**
  String get noMatchingTurns;

  /// No description provided for @loadingTurn.
  ///
  /// In en, this message translates to:
  /// **'Opening turn {number}…'**
  String loadingTurn(int number);

  /// No description provided for @historySyncing.
  ///
  /// In en, this message translates to:
  /// **'Syncing history…'**
  String get historySyncing;

  /// No description provided for @historyCachedOffline.
  ///
  /// In en, this message translates to:
  /// **'Cached history · waiting to sync'**
  String get historyCachedOffline;

  /// No description provided for @historyCacheTitle.
  ///
  /// In en, this message translates to:
  /// **'Conversation cache'**
  String get historyCacheTitle;

  /// No description provided for @historyCacheDescription.
  ///
  /// In en, this message translates to:
  /// **'Shared disk limit in MB. 0 disables caching.'**
  String get historyCacheDescription;

  /// No description provided for @historyCacheUsage.
  ///
  /// In en, this message translates to:
  /// **'{used} MB used · {limit} MB limit'**
  String historyCacheUsage(String used, int limit);

  /// No description provided for @historyCacheInvalid.
  ///
  /// In en, this message translates to:
  /// **'Enter a whole number from 0 to 64000.'**
  String get historyCacheInvalid;

  /// No description provided for @modeAsk.
  ///
  /// In en, this message translates to:
  /// **'Ask for approval'**
  String get modeAsk;

  /// No description provided for @modeAskDesc.
  ///
  /// In en, this message translates to:
  /// **'Work in the workspace; ask you before actions that require approval.'**
  String get modeAskDesc;

  /// No description provided for @fastMode.
  ///
  /// In en, this message translates to:
  /// **'Fast'**
  String get fastMode;

  /// No description provided for @fastModeHint.
  ///
  /// In en, this message translates to:
  /// **'Use the model’s Fast service tier for subsequent turns.'**
  String get fastModeHint;

  /// No description provided for @steerMessage.
  ///
  /// In en, this message translates to:
  /// **'Supplement'**
  String get steerMessage;

  /// No description provided for @steerMessageHint.
  ///
  /// In en, this message translates to:
  /// **'Add to the current turn at its next step. Otherwise, queue for the next turn.'**
  String get steerMessageHint;

  /// No description provided for @queueNextTurn.
  ///
  /// In en, this message translates to:
  /// **'Queue for next turn'**
  String get queueNextTurn;

  /// No description provided for @steerTurnEnded.
  ///
  /// In en, this message translates to:
  /// **'The turn changed or ended. Your draft is kept; send it as a new turn.'**
  String get steerTurnEnded;

  /// No description provided for @steerUnavailableHint.
  ///
  /// In en, this message translates to:
  /// **'This connection does not offer running-turn supplements. Select for details.'**
  String get steerUnavailableHint;

  /// No description provided for @steerQueueOnly.
  ///
  /// In en, this message translates to:
  /// **'Queue only'**
  String get steerQueueOnly;

  /// No description provided for @steerUnavailableTitle.
  ///
  /// In en, this message translates to:
  /// **'Supplement unavailable'**
  String get steerUnavailableTitle;

  /// No description provided for @steerUnavailableDetails.
  ///
  /// In en, this message translates to:
  /// **'{agent} has not advertised support for adding messages to a running turn over this ACP connection. Messages you send now are queued for the next turn.\n\nSupplement is available when the connected agent advertises this capability; choosing a different model does not enable it.'**
  String steerUnavailableDetails(String agent);

  /// No description provided for @steerQueuedAfterEnd.
  ///
  /// In en, this message translates to:
  /// **'The turn had already ended, so your supplement will be sent as the next turn.'**
  String get steerQueuedAfterEnd;

  /// No description provided for @retryUnverified.
  ///
  /// In en, this message translates to:
  /// **'Could not check whether the host received this message, so it was not sent again. Retry when the host is reachable.'**
  String get retryUnverified;

  /// No description provided for @queuedUncertain.
  ///
  /// In en, this message translates to:
  /// **'May already have been sent. It is sent again only if you retry it.'**
  String get queuedUncertain;

  /// No description provided for @queuedUncertainRetry.
  ///
  /// In en, this message translates to:
  /// **'It may already have reached the host, so it stays queued and is not sent by itself. Check the conversation: Retry sends it again, tapping it lets you edit it.'**
  String get queuedUncertainRetry;

  /// No description provided for @steerAnswerStale.
  ///
  /// In en, this message translates to:
  /// **'The answer arrived after this conversation\'s host or view changed, so your draft is kept as it was. Check the conversation before sending it again.'**
  String get steerAnswerStale;

  /// No description provided for @sendHostChanged.
  ///
  /// In en, this message translates to:
  /// **'The host connection changed before this message was sent, so nothing was sent.'**
  String get sendHostChanged;

  /// No description provided for @sendAnswerStale.
  ///
  /// In en, this message translates to:
  /// **'The host answered after its connection changed, so it is unknown whether this message arrived.'**
  String get sendAnswerStale;

  /// No description provided for @sendReturnedToDraft.
  ///
  /// In en, this message translates to:
  /// **'It is back in the composer; check the conversation before sending it again.'**
  String get sendReturnedToDraft;

  /// No description provided for @advancedSettings.
  ///
  /// In en, this message translates to:
  /// **'Advanced settings'**
  String get advancedSettings;

  /// No description provided for @planModeDescription.
  ///
  /// In en, this message translates to:
  /// **'Plan first without implementing. Normal mode is recommended for most tasks.'**
  String get planModeDescription;

  /// No description provided for @imageGenerating.
  ///
  /// In en, this message translates to:
  /// **'Generating image…'**
  String get imageGenerating;

  /// No description provided for @imageLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading image…'**
  String get imageLoading;

  /// No description provided for @imageGenerationFailed.
  ///
  /// In en, this message translates to:
  /// **'Image generation failed'**
  String get imageGenerationFailed;

  /// No description provided for @imageGenerationIncomplete.
  ///
  /// In en, this message translates to:
  /// **'Image generation incomplete'**
  String get imageGenerationIncomplete;

  /// No description provided for @imageGenerationUnavailable.
  ///
  /// In en, this message translates to:
  /// **'No image was returned'**
  String get imageGenerationUnavailable;

  /// No description provided for @imageGenerationLimit.
  ///
  /// In en, this message translates to:
  /// **'Image generation usage limit reached'**
  String get imageGenerationLimit;

  /// No description provided for @fileLinkPreview.
  ///
  /// In en, this message translates to:
  /// **'Preview'**
  String get fileLinkPreview;

  /// No description provided for @fileLinkDownload.
  ///
  /// In en, this message translates to:
  /// **'Download'**
  String get fileLinkDownload;

  /// No description provided for @fileLinkUnsupported.
  ///
  /// In en, this message translates to:
  /// **'This file cannot be previewed. Download it to open with an app on your device.'**
  String get fileLinkUnsupported;

  /// No description provided for @fileLinkTruncated.
  ///
  /// In en, this message translates to:
  /// **'Showing a limited preview. Download for the complete file.'**
  String get fileLinkTruncated;

  /// No description provided for @fileLinkLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading file…'**
  String get fileLinkLoading;

  /// No description provided for @fileLinkRemoteWebTitle.
  ///
  /// In en, this message translates to:
  /// **'Website on the host'**
  String get fileLinkRemoteWebTitle;

  /// No description provided for @fileLinkRemoteWeb.
  ///
  /// In en, this message translates to:
  /// **'This address points to the controlled device, not this device. Use pb-mapper to register the web service port on the host and subscribe to it on this device, then open the mapped local address. Keep the URL path and query unchanged.'**
  String get fileLinkRemoteWeb;

  /// No description provided for @fileLinkCheckingHost.
  ///
  /// In en, this message translates to:
  /// **'Checking host location…'**
  String get fileLinkCheckingHost;

  /// No description provided for @fileLinkFailed.
  ///
  /// In en, this message translates to:
  /// **'Unable to read the file. Check that it still exists and the host is running a version that supports file links.'**
  String get fileLinkFailed;

  /// No description provided for @fileLinkClose.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get fileLinkClose;

  /// No description provided for @providerLabel.
  ///
  /// In en, this message translates to:
  /// **'Provider'**
  String get providerLabel;

  /// No description provided for @providerCodex.
  ///
  /// In en, this message translates to:
  /// **'Codex'**
  String get providerCodex;

  /// No description provided for @hostMetaLabel.
  ///
  /// In en, this message translates to:
  /// **'Meta service'**
  String get hostMetaLabel;

  /// No description provided for @draft.
  ///
  /// In en, this message translates to:
  /// **'Draft'**
  String get draft;

  /// No description provided for @editMessage.
  ///
  /// In en, this message translates to:
  /// **'Edit message'**
  String get editMessage;

  /// No description provided for @done.
  ///
  /// In en, this message translates to:
  /// **'Done'**
  String get done;

  /// No description provided for @expandComposer.
  ///
  /// In en, this message translates to:
  /// **'Expand editor'**
  String get expandComposer;

  /// No description provided for @resetComposerHeight.
  ///
  /// In en, this message translates to:
  /// **'Reset height'**
  String get resetComposerHeight;

  /// No description provided for @selectText.
  ///
  /// In en, this message translates to:
  /// **'Select text'**
  String get selectText;

  /// No description provided for @sharePrompt.
  ///
  /// In en, this message translates to:
  /// **'Share prompt'**
  String get sharePrompt;

  /// No description provided for @shareMessage.
  ///
  /// In en, this message translates to:
  /// **'Share message'**
  String get shareMessage;

  /// No description provided for @messageShareFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not open sharing. You can copy the text instead.'**
  String get messageShareFailed;

  /// No description provided for @messageCopyFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not copy the text. Please try again.'**
  String get messageCopyFailed;

  /// No description provided for @addToDraft.
  ///
  /// In en, this message translates to:
  /// **'Add to draft'**
  String get addToDraft;

  /// No description provided for @editMessageHint.
  ///
  /// In en, this message translates to:
  /// **'Reuse this text as a new message. History stays unchanged; original attachments are not added.'**
  String get editMessageHint;

  /// No description provided for @editMessageDraftHint.
  ///
  /// In en, this message translates to:
  /// **'Your existing draft and attachments will be kept. This text will be added below.'**
  String get editMessageDraftHint;

  /// No description provided for @voiceStart.
  ///
  /// In en, this message translates to:
  /// **'Start voice'**
  String get voiceStart;

  /// No description provided for @voiceStartHint.
  ///
  /// In en, this message translates to:
  /// **'Talk about the current project. Voice can ask Codex to perform tasks using the same permissions as text conversations.'**
  String get voiceStartHint;

  /// No description provided for @voiceUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Voice unavailable'**
  String get voiceUnavailable;

  /// No description provided for @voiceVoice.
  ///
  /// In en, this message translates to:
  /// **'Voice'**
  String get voiceVoice;

  /// No description provided for @voiceModel.
  ///
  /// In en, this message translates to:
  /// **'Voice model'**
  String get voiceModel;

  /// No description provided for @voiceModelHint.
  ///
  /// In en, this message translates to:
  /// **'Leave empty to use the host default'**
  String get voiceModelHint;

  /// No description provided for @voiceConnecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting voice…'**
  String get voiceConnecting;

  /// No description provided for @voiceListening.
  ///
  /// In en, this message translates to:
  /// **'Connected · Listening'**
  String get voiceListening;

  /// No description provided for @voiceMuted.
  ///
  /// In en, this message translates to:
  /// **'Microphone muted'**
  String get voiceMuted;

  /// No description provided for @voiceStopping.
  ///
  /// In en, this message translates to:
  /// **'Ending voice…'**
  String get voiceStopping;

  /// No description provided for @voiceStop.
  ///
  /// In en, this message translates to:
  /// **'End voice'**
  String get voiceStop;

  /// No description provided for @voiceMute.
  ///
  /// In en, this message translates to:
  /// **'Mute'**
  String get voiceMute;

  /// No description provided for @voiceUnmute.
  ///
  /// In en, this message translates to:
  /// **'Unmute'**
  String get voiceUnmute;

  /// No description provided for @voiceHistory.
  ///
  /// In en, this message translates to:
  /// **'Voice transcript'**
  String get voiceHistory;

  /// No description provided for @voiceHistoryEmpty.
  ///
  /// In en, this message translates to:
  /// **'No voice transcript on this page'**
  String get voiceHistoryEmpty;

  /// No description provided for @voiceHistoryMore.
  ///
  /// In en, this message translates to:
  /// **'Load more'**
  String get voiceHistoryMore;

  /// No description provided for @voiceErrorDetails.
  ///
  /// In en, this message translates to:
  /// **'Error details'**
  String get voiceErrorDetails;

  /// No description provided for @voiceNewSession.
  ///
  /// In en, this message translates to:
  /// **'New voice conversation'**
  String get voiceNewSession;

  /// No description provided for @restoringSessionSettings.
  ///
  /// In en, this message translates to:
  /// **'Restoring session settings…'**
  String get restoringSessionSettings;

  /// No description provided for @reviewInProgress.
  ///
  /// In en, this message translates to:
  /// **'Model review in progress'**
  String get reviewInProgress;

  /// No description provided for @reviewTimedOut.
  ///
  /// In en, this message translates to:
  /// **'Review timed out'**
  String get reviewTimedOut;

  /// No description provided for @reviewAborted.
  ///
  /// In en, this message translates to:
  /// **'Review aborted'**
  String get reviewAborted;

  /// No description provided for @guardianSession.
  ///
  /// In en, this message translates to:
  /// **'Model approval'**
  String get guardianSession;

  /// No description provided for @childSession.
  ///
  /// In en, this message translates to:
  /// **'Child session'**
  String get childSession;

  /// No description provided for @childSessions.
  ///
  /// In en, this message translates to:
  /// **'Child sessions ({count})'**
  String childSessions(int count);

  /// No description provided for @guardianReadOnly.
  ///
  /// In en, this message translates to:
  /// **'Model approval session · Read only'**
  String get guardianReadOnly;

  /// No description provided for @parentSession.
  ///
  /// In en, this message translates to:
  /// **'Open parent session'**
  String get parentSession;

  /// No description provided for @hideSidebar.
  ///
  /// In en, this message translates to:
  /// **'Hide sidebar'**
  String get hideSidebar;

  /// No description provided for @showSidebar.
  ///
  /// In en, this message translates to:
  /// **'Show sidebar'**
  String get showSidebar;

  /// No description provided for @locateConversation.
  ///
  /// In en, this message translates to:
  /// **'Show current conversation'**
  String get locateConversation;

  /// No description provided for @voiceReconnecting.
  ///
  /// In en, this message translates to:
  /// **'Voice reconnecting…'**
  String get voiceReconnecting;

  /// No description provided for @voiceSpeaking.
  ///
  /// In en, this message translates to:
  /// **'Assistant speaking'**
  String get voiceSpeaking;

  /// No description provided for @voiceUserSpeaking.
  ///
  /// In en, this message translates to:
  /// **'You\'re speaking'**
  String get voiceUserSpeaking;

  /// No description provided for @voiceEndedByUser.
  ///
  /// In en, this message translates to:
  /// **'Voice call ended'**
  String get voiceEndedByUser;

  /// No description provided for @voiceEndedReason.
  ///
  /// In en, this message translates to:
  /// **'Voice call ended: {reason}'**
  String voiceEndedReason(String reason);

  /// No description provided for @voiceFailedShort.
  ///
  /// In en, this message translates to:
  /// **'Voice call dropped'**
  String get voiceFailedShort;

  /// No description provided for @voiceHangUp.
  ///
  /// In en, this message translates to:
  /// **'Hang up'**
  String get voiceHangUp;

  /// No description provided for @voiceModelDefault.
  ///
  /// In en, this message translates to:
  /// **'Host default'**
  String get voiceModelDefault;

  /// No description provided for @voiceModelFrameless.
  ///
  /// In en, this message translates to:
  /// **'gpt-live-1-codex (low latency)'**
  String get voiceModelFrameless;

  /// No description provided for @voiceModelCustom.
  ///
  /// In en, this message translates to:
  /// **'Custom…'**
  String get voiceModelCustom;

  /// No description provided for @voiceModelCustomLabel.
  ///
  /// In en, this message translates to:
  /// **'Model id'**
  String get voiceModelCustomLabel;

  /// No description provided for @voiceTaskContinues.
  ///
  /// In en, this message translates to:
  /// **'Tasks the voice handed to Codex keep running after you hang up.'**
  String get voiceTaskContinues;

  /// No description provided for @stepOutput.
  ///
  /// In en, this message translates to:
  /// **'output'**
  String get stepOutput;

  /// No description provided for @previewRendered.
  ///
  /// In en, this message translates to:
  /// **'Preview'**
  String get previewRendered;

  /// No description provided for @previewSource.
  ///
  /// In en, this message translates to:
  /// **'Source'**
  String get previewSource;

  /// No description provided for @dictate.
  ///
  /// In en, this message translates to:
  /// **'Dictate'**
  String get dictate;

  /// No description provided for @dictationReady.
  ///
  /// In en, this message translates to:
  /// **'ready'**
  String get dictationReady;

  /// No description provided for @dictationConnecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting dictation…'**
  String get dictationConnecting;

  /// No description provided for @dictationListening.
  ///
  /// In en, this message translates to:
  /// **'Listening'**
  String get dictationListening;

  /// No description provided for @dictationFinishing.
  ///
  /// In en, this message translates to:
  /// **'Finishing…'**
  String get dictationFinishing;

  /// No description provided for @dictationStop.
  ///
  /// In en, this message translates to:
  /// **'Done (Enter)'**
  String get dictationStop;

  /// No description provided for @dictationCancel.
  ///
  /// In en, this message translates to:
  /// **'Discard (Esc)'**
  String get dictationCancel;

  /// No description provided for @dictationKeys.
  ///
  /// In en, this message translates to:
  /// **'Enter done · Esc discard'**
  String get dictationKeys;

  /// No description provided for @dictationPermission.
  ///
  /// In en, this message translates to:
  /// **'Microphone access was denied'**
  String get dictationPermission;

  /// No description provided for @dictationUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Dictation is unavailable on this host'**
  String get dictationUnavailable;

  /// No description provided for @dictationInterrupted.
  ///
  /// In en, this message translates to:
  /// **'Dictation dropped; what was heard is kept'**
  String get dictationInterrupted;

  /// No description provided for @dictationVoiceBusy.
  ///
  /// In en, this message translates to:
  /// **'The live voice call is using the microphone'**
  String get dictationVoiceBusy;

  /// No description provided for @providerAcpAgent.
  ///
  /// In en, this message translates to:
  /// **'ACP agent'**
  String get providerAcpAgent;

  /// No description provided for @protocolAcpTag.
  ///
  /// In en, this message translates to:
  /// **'ACP'**
  String get protocolAcpTag;

  /// No description provided for @acpHostHint.
  ///
  /// In en, this message translates to:
  /// **'Runs an ACP-compatible agent on this device from a program and its arguments, and makes it available to your devices. Stopping hosting stops the agent.'**
  String get acpHostHint;

  /// No description provided for @acpAgentCustom.
  ///
  /// In en, this message translates to:
  /// **'Custom agent'**
  String get acpAgentCustom;

  /// No description provided for @acpAgentName.
  ///
  /// In en, this message translates to:
  /// **'Agent name'**
  String get acpAgentName;

  /// No description provided for @acpProgram.
  ///
  /// In en, this message translates to:
  /// **'Program'**
  String get acpProgram;

  /// No description provided for @acpProgramBrowse.
  ///
  /// In en, this message translates to:
  /// **'Choose…'**
  String get acpProgramBrowse;

  /// No description provided for @acpProgramHint.
  ///
  /// In en, this message translates to:
  /// **'A full path, or a program name found on this device\'s PATH. Paths may contain spaces.'**
  String get acpProgramHint;

  /// No description provided for @acpArguments.
  ///
  /// In en, this message translates to:
  /// **'Arguments'**
  String get acpArguments;

  /// No description provided for @acpArgumentsHint.
  ///
  /// In en, this message translates to:
  /// **'One argument per row, passed exactly as typed: no shell, no splitting, no variables.'**
  String get acpArgumentsHint;

  /// No description provided for @acpArgumentAdd.
  ///
  /// In en, this message translates to:
  /// **'Add argument'**
  String get acpArgumentAdd;

  /// No description provided for @acpArgumentRemove.
  ///
  /// In en, this message translates to:
  /// **'Remove argument'**
  String get acpArgumentRemove;

  /// No description provided for @acpArgumentMoveUp.
  ///
  /// In en, this message translates to:
  /// **'Move up'**
  String get acpArgumentMoveUp;

  /// No description provided for @acpArgumentMoveDown.
  ///
  /// In en, this message translates to:
  /// **'Move down'**
  String get acpArgumentMoveDown;

  /// No description provided for @acpArgumentLabel.
  ///
  /// In en, this message translates to:
  /// **'Argument {index}'**
  String acpArgumentLabel(int index);

  /// No description provided for @acpSecretsNote.
  ///
  /// In en, this message translates to:
  /// **'Saved in plain text on this computer. Keep secrets out of the arguments.'**
  String get acpSecretsNote;

  /// No description provided for @acpProgramNotFound.
  ///
  /// In en, this message translates to:
  /// **'This program was not found on this device.'**
  String get acpProgramNotFound;

  /// No description provided for @claudeEnvTitle.
  ///
  /// In en, this message translates to:
  /// **'Claude environment'**
  String get claudeEnvTitle;

  /// No description provided for @claudeRuntimeTitle.
  ///
  /// In en, this message translates to:
  /// **'Runtime components'**
  String get claudeRuntimeTitle;

  /// No description provided for @claudeAdapterInstalled.
  ///
  /// In en, this message translates to:
  /// **'ACP adapter: {path}'**
  String claudeAdapterInstalled(String path);

  /// No description provided for @claudeAdapterPrepared.
  ///
  /// In en, this message translates to:
  /// **'ACP adapter (prepared by this app): {path}'**
  String claudeAdapterPrepared(String path);

  /// No description provided for @claudeAdapterWillPrepare.
  ///
  /// In en, this message translates to:
  /// **'The ACP adapter is not installed. Prepare it now, or let Start hosting prepare it first. This downloads {package} from npm into this app\'s cache; the global environment stays unchanged.'**
  String claudeAdapterWillPrepare(String package);

  /// No description provided for @claudeAdapterMissing.
  ///
  /// In en, this message translates to:
  /// **'The ACP adapter ({package}) is not installed, and it cannot be prepared here.'**
  String claudeAdapterMissing(String package);

  /// No description provided for @claudeNodeFound.
  ///
  /// In en, this message translates to:
  /// **'Node.js {version}: {path}'**
  String claudeNodeFound(String version, String path);

  /// No description provided for @claudeNodeTooOld.
  ///
  /// In en, this message translates to:
  /// **'Node.js {version} is too old; the adapter needs Node.js 22 or newer.'**
  String claudeNodeTooOld(String version);

  /// No description provided for @claudeNodeMissing.
  ///
  /// In en, this message translates to:
  /// **'Node.js 22 or newer was not found.'**
  String get claudeNodeMissing;

  /// No description provided for @claudeCliFound.
  ///
  /// In en, this message translates to:
  /// **'Claude Code: {path}'**
  String claudeCliFound(String path);

  /// No description provided for @claudeCliInherited.
  ///
  /// In en, this message translates to:
  /// **'Claude Code: as set in CLAUDE_CODE_EXECUTABLE'**
  String get claudeCliInherited;

  /// No description provided for @claudeCliBundled.
  ///
  /// In en, this message translates to:
  /// **'Claude Code was not found; the adapter uses its bundled copy.'**
  String get claudeCliBundled;

  /// No description provided for @claudePrepareEnvironment.
  ///
  /// In en, this message translates to:
  /// **'Prepare ACP environment'**
  String get claudePrepareEnvironment;

  /// No description provided for @claudeEnvironmentReady.
  ///
  /// In en, this message translates to:
  /// **'ACP environment is ready. No host was started or restarted.'**
  String get claudeEnvironmentReady;

  /// No description provided for @claudeEnvRecheck.
  ///
  /// In en, this message translates to:
  /// **'Check again'**
  String get claudeEnvRecheck;

  /// No description provided for @claudePrepareCancel.
  ///
  /// In en, this message translates to:
  /// **'Stop preparing'**
  String get claudePrepareCancel;

  /// No description provided for @claudePrepareStopWaiting.
  ///
  /// In en, this message translates to:
  /// **'Stop waiting'**
  String get claudePrepareStopWaiting;

  /// No description provided for @claudePrepareJoined.
  ///
  /// In en, this message translates to:
  /// **'Another window started this preparation; this page only follows it.'**
  String get claudePrepareJoined;

  /// No description provided for @claudePrepareLeftRunning.
  ///
  /// In en, this message translates to:
  /// **'Stopped waiting. The preparation started elsewhere keeps running.'**
  String get claudePrepareLeftRunning;

  /// No description provided for @claudePrepareReplaced.
  ///
  /// In en, this message translates to:
  /// **'Another preparation replaced the one this page followed, so its result is not shown here. Prepare ACP environment or Start hosting to retry.'**
  String get claudePrepareReplaced;

  /// No description provided for @quitStopping.
  ///
  /// In en, this message translates to:
  /// **'Stopping hosted agents before quitting…'**
  String get quitStopping;

  /// No description provided for @quitBlocked.
  ///
  /// In en, this message translates to:
  /// **'The app stays open because hosting could not be stopped safely: {error}'**
  String quitBlocked(String error);

  /// No description provided for @quitRetry.
  ///
  /// In en, this message translates to:
  /// **'Quit again'**
  String get quitRetry;

  /// No description provided for @claudePrepareFailed.
  ///
  /// In en, this message translates to:
  /// **'Preparing the Claude ACP adapter failed: {error}'**
  String claudePrepareFailed(String error);

  /// No description provided for @claudePrepareCancelled.
  ///
  /// In en, this message translates to:
  /// **'Preparing the Claude ACP adapter was stopped. Choose Prepare ACP environment or Start hosting to retry.'**
  String get claudePrepareCancelled;

  /// No description provided for @acpProgramFound.
  ///
  /// In en, this message translates to:
  /// **'Found: {path}'**
  String acpProgramFound(String path);

  /// No description provided for @acpSaveAgent.
  ///
  /// In en, this message translates to:
  /// **'Save as a configuration'**
  String get acpSaveAgent;

  /// No description provided for @acpHostingUnsupported.
  ///
  /// In en, this message translates to:
  /// **'Hosting ACP agents is not available on this platform yet. You can still use agents hosted on another device.'**
  String get acpHostingUnsupported;

  /// No description provided for @acpPhaseStarting.
  ///
  /// In en, this message translates to:
  /// **'Starting'**
  String get acpPhaseStarting;

  /// No description provided for @acpPhaseReady.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get acpPhaseReady;

  /// No description provided for @acpPhaseFailed.
  ///
  /// In en, this message translates to:
  /// **'Not running'**
  String get acpPhaseFailed;

  /// No description provided for @acpPhaseStopped.
  ///
  /// In en, this message translates to:
  /// **'Stopped'**
  String get acpPhaseStopped;

  /// No description provided for @acpAuthRequired.
  ///
  /// In en, this message translates to:
  /// **'The agent says you need to sign in. Sign in with the agent itself on the host, then restart it.'**
  String get acpAuthRequired;

  /// No description provided for @acpRestart.
  ///
  /// In en, this message translates to:
  /// **'Restart agent'**
  String get acpRestart;

  /// No description provided for @acpGatewayLabel.
  ///
  /// In en, this message translates to:
  /// **'Agent gateway'**
  String get acpGatewayLabel;

  /// No description provided for @acpStopNote.
  ///
  /// In en, this message translates to:
  /// **'Stopping hosting stops this agent and the processes it started.'**
  String get acpStopNote;

  /// No description provided for @acpFeatureReopen.
  ///
  /// In en, this message translates to:
  /// **'Reopen earlier sessions'**
  String get acpFeatureReopen;

  /// No description provided for @acpFeatureReopenLoad.
  ///
  /// In en, this message translates to:
  /// **'With history'**
  String get acpFeatureReopenLoad;

  /// No description provided for @acpFeatureReopenResume.
  ///
  /// In en, this message translates to:
  /// **'Without earlier history'**
  String get acpFeatureReopenResume;

  /// No description provided for @acpFeatureList.
  ///
  /// In en, this message translates to:
  /// **'Agent\'s own session list'**
  String get acpFeatureList;

  /// No description provided for @acpFeatureImages.
  ///
  /// In en, this message translates to:
  /// **'Image attachments'**
  String get acpFeatureImages;

  /// No description provided for @acpFeatureSupported.
  ///
  /// In en, this message translates to:
  /// **'Supported'**
  String get acpFeatureSupported;

  /// No description provided for @acpFeatureNotSupported.
  ///
  /// In en, this message translates to:
  /// **'Not supported'**
  String get acpFeatureNotSupported;

  /// No description provided for @acpFeatureUnknown.
  ///
  /// In en, this message translates to:
  /// **'Shown after connecting to the running agent'**
  String get acpFeatureUnknown;

  /// No description provided for @permissionFromAgent.
  ///
  /// In en, this message translates to:
  /// **'{agent} asks for permission'**
  String permissionFromAgent(String agent);

  /// No description provided for @permissionRememberTitle.
  ///
  /// In en, this message translates to:
  /// **'Remember this choice?'**
  String get permissionRememberTitle;

  /// No description provided for @permissionRememberBody.
  ///
  /// In en, this message translates to:
  /// **'\"{option}\" asks the agent to remember this decision. How long and how widely it applies is decided by the agent.'**
  String permissionRememberBody(String option);

  /// No description provided for @permissionAnswerFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not send your answer. Try again.'**
  String get permissionAnswerFailed;

  /// No description provided for @agentOptions.
  ///
  /// In en, this message translates to:
  /// **'Agent options'**
  String get agentOptions;

  /// No description provided for @agentMode.
  ///
  /// In en, this message translates to:
  /// **'Mode'**
  String get agentMode;

  /// No description provided for @agentOptionSearch.
  ///
  /// In en, this message translates to:
  /// **'Search {option}'**
  String agentOptionSearch(String option);

  /// No description provided for @agentOptionNoMatch.
  ///
  /// In en, this message translates to:
  /// **'No matching values.'**
  String get agentOptionNoMatch;

  /// No description provided for @historyGapNotice.
  ///
  /// In en, this message translates to:
  /// **'Earlier history is not available from this agent host.'**
  String get historyGapNotice;

  /// No description provided for @contentOmittedNotice.
  ///
  /// In en, this message translates to:
  /// **'Not kept because it was too large: {parts}.'**
  String contentOmittedNotice(String parts);

  /// No description provided for @contentUnavailableNotice.
  ///
  /// In en, this message translates to:
  /// **'Part of this reply could not be read from the agent host.'**
  String get contentUnavailableNotice;

  /// No description provided for @omittedImages.
  ///
  /// In en, this message translates to:
  /// **'images'**
  String get omittedImages;

  /// No description provided for @omittedDiffs.
  ///
  /// In en, this message translates to:
  /// **'file changes'**
  String get omittedDiffs;

  /// No description provided for @omittedInput.
  ///
  /// In en, this message translates to:
  /// **'tool input'**
  String get omittedInput;

  /// No description provided for @omittedLocations.
  ///
  /// In en, this message translates to:
  /// **'file locations'**
  String get omittedLocations;

  /// No description provided for @omittedPlan.
  ///
  /// In en, this message translates to:
  /// **'plan steps'**
  String get omittedPlan;

  /// No description provided for @omittedContent.
  ///
  /// In en, this message translates to:
  /// **'some output'**
  String get omittedContent;

  /// No description provided for @omittedSeparator.
  ///
  /// In en, this message translates to:
  /// **', '**
  String get omittedSeparator;

  /// No description provided for @hostEditorTitle.
  ///
  /// In en, this message translates to:
  /// **'Host {name}'**
  String hostEditorTitle(String name);

  /// No description provided for @hostSectionAgent.
  ///
  /// In en, this message translates to:
  /// **'Agent'**
  String get hostSectionAgent;

  /// No description provided for @hostSectionStatus.
  ///
  /// In en, this message translates to:
  /// **'Status'**
  String get hostSectionStatus;

  /// No description provided for @hostSectionModel.
  ///
  /// In en, this message translates to:
  /// **'Model service'**
  String get hostSectionModel;

  /// No description provided for @hostSectionModelHint.
  ///
  /// In en, this message translates to:
  /// **'Where this host\'s agent sends model requests. Saved per host on this device.'**
  String get hostSectionModelHint;

  /// No description provided for @hostSourceInherited.
  ///
  /// In en, this message translates to:
  /// **'Existing configuration'**
  String get hostSourceInherited;

  /// No description provided for @hostSourceOfficial.
  ///
  /// In en, this message translates to:
  /// **'Official account'**
  String get hostSourceOfficial;

  /// No description provided for @hostSourceCustom.
  ///
  /// In en, this message translates to:
  /// **'Custom provider'**
  String get hostSourceCustom;

  /// No description provided for @hostInheritedAcp.
  ///
  /// In en, this message translates to:
  /// **'The agent uses its own configuration files and environment, unchanged.'**
  String get hostInheritedAcp;

  /// No description provided for @hostInheritedCodexCustom.
  ///
  /// In en, this message translates to:
  /// **'Codex\'s config.toml already selects provider {provider}. Nothing more is needed.'**
  String hostInheritedCodexCustom(String provider);

  /// No description provided for @hostInheritedCodexAccount.
  ///
  /// In en, this message translates to:
  /// **'Codex is already signed in. Nothing more is needed.'**
  String get hostInheritedCodexAccount;

  /// No description provided for @hostInheritedCodexInvalid.
  ///
  /// In en, this message translates to:
  /// **'Codex\'s config.toml could not be read. Fix it, or choose an option below.'**
  String get hostInheritedCodexInvalid;

  /// No description provided for @hostInheritedCodexNone.
  ///
  /// In en, this message translates to:
  /// **'Codex has no sign-in or provider yet. Sign in or add a custom provider.'**
  String get hostInheritedCodexNone;

  /// No description provided for @hostInheritedCodexHome.
  ///
  /// In en, this message translates to:
  /// **'Configuration directory: {path}'**
  String hostInheritedCodexHome(String path);

  /// No description provided for @hostProtocolLabel.
  ///
  /// In en, this message translates to:
  /// **'API type'**
  String get hostProtocolLabel;

  /// No description provided for @hostProtocolResponses.
  ///
  /// In en, this message translates to:
  /// **'OpenAI Responses'**
  String get hostProtocolResponses;

  /// No description provided for @hostProtocolAnthropic.
  ///
  /// In en, this message translates to:
  /// **'Anthropic Messages'**
  String get hostProtocolAnthropic;

  /// No description provided for @hostProtocolChat.
  ///
  /// In en, this message translates to:
  /// **'OpenAI Chat Completions'**
  String get hostProtocolChat;

  /// No description provided for @hostBaseUrl.
  ///
  /// In en, this message translates to:
  /// **'Base URL'**
  String get hostBaseUrl;

  /// No description provided for @hostApiKey.
  ///
  /// In en, this message translates to:
  /// **'API key'**
  String get hostApiKey;

  /// No description provided for @hostApiKeyStored.
  ///
  /// In en, this message translates to:
  /// **'A key is stored. Leave empty to keep it.'**
  String get hostApiKeyStored;

  /// No description provided for @hostKeyEnvNote.
  ///
  /// In en, this message translates to:
  /// **'Stored only on this device and handed to the agent when it starts, never on its command line.'**
  String get hostKeyEnvNote;

  /// No description provided for @hostApiKeyRemove.
  ///
  /// In en, this message translates to:
  /// **'Remove stored key'**
  String get hostApiKeyRemove;

  /// No description provided for @hostModelOptional.
  ///
  /// In en, this message translates to:
  /// **'Model (optional)'**
  String get hostModelOptional;

  /// No description provided for @hostBaseUrlRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter the provider\'s base URL.'**
  String get hostBaseUrlRequired;

  /// No description provided for @hostSaveConfig.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get hostSaveConfig;

  /// No description provided for @hostConfigSaved.
  ///
  /// In en, this message translates to:
  /// **'Saved. The running host keeps its current configuration until it restarts.'**
  String get hostConfigSaved;

  /// No description provided for @hostConfigPending.
  ///
  /// In en, this message translates to:
  /// **'Saved changes are not active yet. Restart the host to apply them.'**
  String get hostConfigPending;

  /// No description provided for @hostApplyRestart.
  ///
  /// In en, this message translates to:
  /// **'Apply and restart'**
  String get hostApplyRestart;

  /// No description provided for @hostSaveAndRestart.
  ///
  /// In en, this message translates to:
  /// **'Save and restart'**
  String get hostSaveAndRestart;

  /// No description provided for @hostApplyNote.
  ///
  /// In en, this message translates to:
  /// **'Restarting stops the agent briefly and disconnects open sessions. If the new configuration fails to start, the previous one is restored.'**
  String get hostApplyNote;

  /// No description provided for @hostApplied.
  ///
  /// In en, this message translates to:
  /// **'Restarted with the new configuration.'**
  String get hostApplied;

  /// No description provided for @hostApplyRestored.
  ///
  /// In en, this message translates to:
  /// **'The new configuration failed, so the previous one was restored: {error}'**
  String hostApplyRestored(String error);

  /// No description provided for @hostApplyFailed.
  ///
  /// In en, this message translates to:
  /// **'Restart failed and the host is stopped: {error}'**
  String hostApplyFailed(String error);

  /// No description provided for @hostOfficialNote.
  ///
  /// In en, this message translates to:
  /// **'Sign-in runs on a private local Codex server, so no host needs to be running.'**
  String get hostOfficialNote;

  /// No description provided for @hostAgentEditNote.
  ///
  /// In en, this message translates to:
  /// **'Changing the launch command takes effect on restart.'**
  String get hostAgentEditNote;

  /// No description provided for @hostCodexRuntimeFixed.
  ///
  /// In en, this message translates to:
  /// **'What the host runs now. Edit the runtime below, save, then apply to restart with it.'**
  String get hostCodexRuntimeFixed;

  /// No description provided for @hostLoginRefreshFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not check the sign-in; retrying: {error}'**
  String hostLoginRefreshFailed(String error);

  /// No description provided for @hostLoginNotOfficial.
  ///
  /// In en, this message translates to:
  /// **'Codex finished the login, but not with a ChatGPT account. Sign in again.'**
  String get hostLoginNotOfficial;

  /// No description provided for @hostLoginFailed.
  ///
  /// In en, this message translates to:
  /// **'Sign-in failed: {error}'**
  String hostLoginFailed(String error);

  /// No description provided for @hostSourceUnsupported.
  ///
  /// In en, this message translates to:
  /// **'The saved model service cannot be used with this agent. Choose another one.'**
  String get hostSourceUnsupported;

  /// No description provided for @hostModelRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter the default model this provider should use.'**
  String get hostModelRequired;

  /// No description provided for @hostKeyRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter the provider\'s API key.'**
  String get hostKeyRequired;

  /// No description provided for @hostModelDefault.
  ///
  /// In en, this message translates to:
  /// **'Default model'**
  String get hostModelDefault;

  /// No description provided for @hostModelDefaultNote.
  ///
  /// In en, this message translates to:
  /// **'The provider\'s model for new sessions. A conversation\'s own model choice is made in the conversation and is not stored here.'**
  String get hostModelDefaultNote;

  /// No description provided for @hostLeaveTitle.
  ///
  /// In en, this message translates to:
  /// **'Discard unsaved changes?'**
  String get hostLeaveTitle;

  /// No description provided for @hostLeaveBody.
  ///
  /// In en, this message translates to:
  /// **'Your changes to this host have not been saved.'**
  String get hostLeaveBody;

  /// No description provided for @hostLeaveStay.
  ///
  /// In en, this message translates to:
  /// **'Keep editing'**
  String get hostLeaveStay;

  /// No description provided for @hostLeaveDiscard.
  ///
  /// In en, this message translates to:
  /// **'Discard'**
  String get hostLeaveDiscard;

  /// No description provided for @hostApplyConfirmTitle.
  ///
  /// In en, this message translates to:
  /// **'Restart {name}?'**
  String hostApplyConfirmTitle(String name);

  /// No description provided for @hostApplyConfirmBody.
  ///
  /// In en, this message translates to:
  /// **'The host stops and starts again with its saved configuration. A running turn is interrupted and open conversations reconnect. Drafts, attachments and queued messages on this device are kept. Other hosts are not affected.'**
  String get hostApplyConfirmBody;

  /// No description provided for @hostReplaced.
  ///
  /// In en, this message translates to:
  /// **'This host was restarted elsewhere since this page read it. Reload before applying.'**
  String get hostReplaced;

  /// No description provided for @hostApplyStopped.
  ///
  /// In en, this message translates to:
  /// **'The new configuration failed ({error}) and restoring the previous one failed too ({restoreError}). The host is stopped.'**
  String hostApplyStopped(String error, String restoreError);

  /// No description provided for @hostGone.
  ///
  /// In en, this message translates to:
  /// **'This host is no longer running.'**
  String get hostGone;

  /// No description provided for @hostConfigReload.
  ///
  /// In en, this message translates to:
  /// **'Reload'**
  String get hostConfigReload;

  /// No description provided for @hostConfigNewer.
  ///
  /// In en, this message translates to:
  /// **'A newer saved configuration exists. Reload to see it (your edits are discarded).'**
  String get hostConfigNewer;

  /// No description provided for @hostConfigLoadFailedTitle.
  ///
  /// In en, this message translates to:
  /// **'Configuration unavailable'**
  String get hostConfigLoadFailedTitle;

  /// No description provided for @hostConfigLoadFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not read this host\'s saved configuration: {error}'**
  String hostConfigLoadFailed(String error);

  /// No description provided for @hostRuntimeEditNote.
  ///
  /// In en, this message translates to:
  /// **'Saved changes take effect when you apply them; the name stays.'**
  String get hostRuntimeEditNote;

  /// No description provided for @hostAgentUnknown.
  ///
  /// In en, this message translates to:
  /// **'This host does not report its agent, so it cannot be edited here.'**
  String get hostAgentUnknown;

  /// No description provided for @hostApiProxyNote.
  ///
  /// In en, this message translates to:
  /// **'This host also publishes a separate Responses API service (api:<name>). It always uses this device\'s official Codex sign-in, not the provider chosen here.'**
  String get hostApiProxyNote;

  /// No description provided for @claudePrepareStageWorking.
  ///
  /// In en, this message translates to:
  /// **'Preparing the Claude ACP adapter…'**
  String get claudePrepareStageWorking;

  /// No description provided for @claudePrepareStageInstalling.
  ///
  /// In en, this message translates to:
  /// **'Resolving and installing packages…'**
  String get claudePrepareStageInstalling;

  /// No description provided for @claudePrepareStageDownloading.
  ///
  /// In en, this message translates to:
  /// **'Downloading packages…'**
  String get claudePrepareStageDownloading;

  /// No description provided for @claudePrepareStageVerifying.
  ///
  /// In en, this message translates to:
  /// **'Checking the downloaded adapter…'**
  String get claudePrepareStageVerifying;

  /// No description provided for @claudePrepareStageDone.
  ///
  /// In en, this message translates to:
  /// **'The Claude ACP adapter is ready'**
  String get claudePrepareStageDone;

  /// No description provided for @claudePrepareStageFailed.
  ///
  /// In en, this message translates to:
  /// **'Preparation failed'**
  String get claudePrepareStageFailed;

  /// No description provided for @claudePrepareStageCancelled.
  ///
  /// In en, this message translates to:
  /// **'Preparation stopped'**
  String get claudePrepareStageCancelled;

  /// No description provided for @claudePrepareElapsed.
  ///
  /// In en, this message translates to:
  /// **'{time} elapsed'**
  String claudePrepareElapsed(String time);

  /// No description provided for @claudePrepareDownloaded.
  ///
  /// In en, this message translates to:
  /// **'{size} downloaded'**
  String claudePrepareDownloaded(String size);

  /// No description provided for @claudePrepareSpeed.
  ///
  /// In en, this message translates to:
  /// **'Download speed {speed}'**
  String claudePrepareSpeed(String speed);

  /// No description provided for @claudePrepareSpeedMeasuring.
  ///
  /// In en, this message translates to:
  /// **'Measuring speed…'**
  String get claudePrepareSpeedMeasuring;

  /// No description provided for @claudePrepareStepFetch.
  ///
  /// In en, this message translates to:
  /// **'Download and install'**
  String get claudePrepareStepFetch;

  /// No description provided for @claudePrepareStepVerify.
  ///
  /// In en, this message translates to:
  /// **'Verify'**
  String get claudePrepareStepVerify;

  /// No description provided for @claudePrepareStep.
  ///
  /// In en, this message translates to:
  /// **'Step {step} of {total}: {name}'**
  String claudePrepareStep(int step, int total, String name);

  /// No description provided for @claudeEnvHint.
  ///
  /// In en, this message translates to:
  /// **'Defaults inherit the environment available to this host (including the login shell on macOS). Turn on a field to override it for this host only. Save keeps the running host unchanged; restart to apply. Model support depends on your Claude version and provider.'**
  String get claudeEnvHint;

  /// No description provided for @claudeEnvProviderWins.
  ///
  /// In en, this message translates to:
  /// **'The custom provider fields above control the URL, credential and default model. Environment connection overrides are used only with inherited configuration.'**
  String get claudeEnvProviderWins;

  /// No description provided for @claudeEnvInherited.
  ///
  /// In en, this message translates to:
  /// **'Inherit environment'**
  String get claudeEnvInherited;

  /// No description provided for @claudeEnvCustom.
  ///
  /// In en, this message translates to:
  /// **'Override for this host'**
  String get claudeEnvCustom;

  /// No description provided for @claudeEnvStored.
  ///
  /// In en, this message translates to:
  /// **'Stored securely · leave blank to keep'**
  String get claudeEnvStored;

  /// No description provided for @claudeEnvValue.
  ///
  /// In en, this message translates to:
  /// **'Enter a value'**
  String get claudeEnvValue;

  /// No description provided for @claudeEnvRequired.
  ///
  /// In en, this message translates to:
  /// **'Fill each enabled override, or switch it back to inherit.'**
  String get claudeEnvRequired;

  /// No description provided for @claudeEnvConnection.
  ///
  /// In en, this message translates to:
  /// **'Connection & credentials'**
  String get claudeEnvConnection;

  /// No description provided for @claudeEnvModels.
  ///
  /// In en, this message translates to:
  /// **'Models & aliases'**
  String get claudeEnvModels;

  /// No description provided for @claudeEnvReasoning.
  ///
  /// In en, this message translates to:
  /// **'Reasoning & output'**
  String get claudeEnvReasoning;

  /// No description provided for @claudeEnvNetwork.
  ///
  /// In en, this message translates to:
  /// **'Network & compatibility'**
  String get claudeEnvNetwork;

  /// No description provided for @claudeEnvBaseUrl.
  ///
  /// In en, this message translates to:
  /// **'API base URL'**
  String get claudeEnvBaseUrl;

  /// No description provided for @claudeEnvAuthToken.
  ///
  /// In en, this message translates to:
  /// **'Bearer token'**
  String get claudeEnvAuthToken;

  /// No description provided for @claudeEnvApiKey.
  ///
  /// In en, this message translates to:
  /// **'Anthropic API key'**
  String get claudeEnvApiKey;

  /// No description provided for @claudeEnvOAuthToken.
  ///
  /// In en, this message translates to:
  /// **'Existing OAuth token'**
  String get claudeEnvOAuthToken;

  /// No description provided for @claudeEnvModel.
  ///
  /// In en, this message translates to:
  /// **'Default model'**
  String get claudeEnvModel;

  /// No description provided for @claudeEnvOpus.
  ///
  /// In en, this message translates to:
  /// **'Opus alias model'**
  String get claudeEnvOpus;

  /// No description provided for @claudeEnvSonnet.
  ///
  /// In en, this message translates to:
  /// **'Sonnet alias model'**
  String get claudeEnvSonnet;

  /// No description provided for @claudeEnvHaiku.
  ///
  /// In en, this message translates to:
  /// **'Haiku alias model'**
  String get claudeEnvHaiku;

  /// No description provided for @claudeEnvCustomModel.
  ///
  /// In en, this message translates to:
  /// **'Additional model picker option'**
  String get claudeEnvCustomModel;

  /// No description provided for @claudeEnvSubagentModel.
  ///
  /// In en, this message translates to:
  /// **'Subagent model'**
  String get claudeEnvSubagentModel;

  /// No description provided for @claudeEnvEffort.
  ///
  /// In en, this message translates to:
  /// **'Reasoning effort'**
  String get claudeEnvEffort;

  /// No description provided for @claudeEnvThinkingTokens.
  ///
  /// In en, this message translates to:
  /// **'Thinking token budget'**
  String get claudeEnvThinkingTokens;

  /// No description provided for @claudeEnvOutputTokens.
  ///
  /// In en, this message translates to:
  /// **'Maximum output tokens'**
  String get claudeEnvOutputTokens;

  /// No description provided for @claudeEnvDisableThinking.
  ///
  /// In en, this message translates to:
  /// **'Omit the thinking parameter'**
  String get claudeEnvDisableThinking;

  /// No description provided for @claudeEnvDisableAdaptive.
  ///
  /// In en, this message translates to:
  /// **'Disable adaptive thinking (supported models)'**
  String get claudeEnvDisableAdaptive;

  /// No description provided for @claudeEnvTimeout.
  ///
  /// In en, this message translates to:
  /// **'API timeout (milliseconds)'**
  String get claudeEnvTimeout;

  /// No description provided for @claudeEnvHttpProxy.
  ///
  /// In en, this message translates to:
  /// **'HTTP proxy'**
  String get claudeEnvHttpProxy;

  /// No description provided for @claudeEnvHttpsProxy.
  ///
  /// In en, this message translates to:
  /// **'HTTPS proxy'**
  String get claudeEnvHttpsProxy;

  /// No description provided for @claudeEnvNoProxy.
  ///
  /// In en, this message translates to:
  /// **'Proxy bypass hosts'**
  String get claudeEnvNoProxy;

  /// No description provided for @claudeEnvDisableCaching.
  ///
  /// In en, this message translates to:
  /// **'Disable prompt caching'**
  String get claudeEnvDisableCaching;

  /// No description provided for @claudeEnvDisableTraffic.
  ///
  /// In en, this message translates to:
  /// **'Disable nonessential traffic'**
  String get claudeEnvDisableTraffic;

  /// No description provided for @claudeEnvNeedsValue.
  ///
  /// In en, this message translates to:
  /// **'Needs a value, or switch back to inherit.'**
  String get claudeEnvNeedsValue;

  /// No description provided for @claudeEnvFlagOn.
  ///
  /// In en, this message translates to:
  /// **'On'**
  String get claudeEnvFlagOn;

  /// No description provided for @claudeEnvFlagOff.
  ///
  /// In en, this message translates to:
  /// **'Off'**
  String get claudeEnvFlagOff;

  /// No description provided for @claudeEnvCredential.
  ///
  /// In en, this message translates to:
  /// **'Credential'**
  String get claudeEnvCredential;

  /// No description provided for @claudeEnvCredentialNote.
  ///
  /// In en, this message translates to:
  /// **'Claude uses one credential. Choosing one replaces the others for this host.'**
  String get claudeEnvCredentialNote;

  /// No description provided for @claudeEnvGroupInherited.
  ///
  /// In en, this message translates to:
  /// **'All inherited'**
  String get claudeEnvGroupInherited;

  /// No description provided for @claudeEnvGroupMissing.
  ///
  /// In en, this message translates to:
  /// **'A value is still needed'**
  String get claudeEnvGroupMissing;

  /// No description provided for @claudeEnvAllInherited.
  ///
  /// In en, this message translates to:
  /// **'Inherits everything'**
  String get claudeEnvAllInherited;

  /// No description provided for @claudeEnvOverrides.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 override} other{{count} overrides}}'**
  String claudeEnvOverrides(int count);

  /// No description provided for @claudeEnvMissing.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 value needed} other{{count} values needed}}'**
  String claudeEnvMissing(int count);

  /// No description provided for @claudeEnvDormant.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 saved connection override is kept and applies again with inherited configuration.} other{{count} saved connection overrides are kept and apply again with inherited configuration.}}'**
  String claudeEnvDormant(int count);

  /// No description provided for @claudeEnvEditDormant.
  ///
  /// In en, this message translates to:
  /// **'Edit inactive overrides'**
  String get claudeEnvEditDormant;

  /// No description provided for @nativeHistoryReadOnly.
  ///
  /// In en, this message translates to:
  /// **'Local history · Read-only'**
  String get nativeHistoryReadOnly;

  /// No description provided for @agentWelcomeTitle.
  ///
  /// In en, this message translates to:
  /// **'Start with {agent}'**
  String agentWelcomeTitle(String agent);

  /// No description provided for @agentWelcomeOpening.
  ///
  /// In en, this message translates to:
  /// **'Opening a {agent} session in this folder…'**
  String agentWelcomeOpening(String agent);

  /// No description provided for @agentWelcomeReady.
  ///
  /// In en, this message translates to:
  /// **'Ready. Type your first message.'**
  String get agentWelcomeReady;

  /// No description provided for @agentWelcomeReadyModel.
  ///
  /// In en, this message translates to:
  /// **'Ready · {model}'**
  String agentWelcomeReadyModel(String model);

  /// No description provided for @agentWelcomeFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t open a session: {reason}'**
  String agentWelcomeFailed(String reason);

  /// No description provided for @agentWelcomeNeedsProject.
  ///
  /// In en, this message translates to:
  /// **'Choose a folder for {agent} to work in.'**
  String agentWelcomeNeedsProject(String agent);

  /// No description provided for @agentWelcomeLater.
  ///
  /// In en, this message translates to:
  /// **'The session opens with your first message.'**
  String get agentWelcomeLater;

  /// No description provided for @agentWelcomeConnecting.
  ///
  /// In en, this message translates to:
  /// **'Waiting for the host connection…'**
  String get agentWelcomeConnecting;

  /// No description provided for @chooseFolder.
  ///
  /// In en, this message translates to:
  /// **'Choose a folder…'**
  String get chooseFolder;

  /// No description provided for @recentProjects.
  ///
  /// In en, this message translates to:
  /// **'Recent projects'**
  String get recentProjects;

  /// No description provided for @agentHostSettings.
  ///
  /// In en, this message translates to:
  /// **'Host settings'**
  String get agentHostSettings;

  /// No description provided for @agentOpening.
  ///
  /// In en, this message translates to:
  /// **'Opening {agent}…'**
  String agentOpening(String agent);

  /// No description provided for @agentOpeningHint.
  ///
  /// In en, this message translates to:
  /// **'The agent\'s model and options appear once its session is open. Anything you type stays here.'**
  String get agentOpeningHint;

  /// No description provided for @agentOptionsUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Options unavailable'**
  String get agentOptionsUnavailable;

  /// No description provided for @agentOptionsRetry.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t load the agent\'s options. Select to try again.'**
  String get agentOptionsRetry;

  /// No description provided for @agentErrorHint.
  ///
  /// In en, this message translates to:
  /// **'Check this host\'s model service and environment, then retry.'**
  String get agentErrorHint;

  /// No description provided for @agentAuthHint.
  ///
  /// In en, this message translates to:
  /// **'The agent reports that it needs credentials. Provide them through this host\'s environment, then retry.'**
  String get agentAuthHint;

  /// No description provided for @folderOnThisComputer.
  ///
  /// In en, this message translates to:
  /// **'Choose on this computer…'**
  String get folderOnThisComputer;

  /// No description provided for @folderTypePath.
  ///
  /// In en, this message translates to:
  /// **'Enter a path'**
  String get folderTypePath;

  /// No description provided for @folderPathFieldLabel.
  ///
  /// In en, this message translates to:
  /// **'Folder path on the host'**
  String get folderPathFieldLabel;

  /// No description provided for @folderOutsideRootsNote.
  ///
  /// In en, this message translates to:
  /// **'This path is outside the project folders, so it can\'t be browsed here. The host checks it when the conversation starts.'**
  String get folderOutsideRootsNote;

  /// No description provided for @folderPickerNoRootsHint.
  ///
  /// In en, this message translates to:
  /// **'You can still enter any folder path on the host.'**
  String get folderPickerNoRootsHint;

  /// No description provided for @folderNoSubfolders.
  ///
  /// In en, this message translates to:
  /// **'No subfolders here. You can use this folder as it is.'**
  String get folderNoSubfolders;

  /// No description provided for @folderWillUse.
  ///
  /// In en, this message translates to:
  /// **'Will use'**
  String get folderWillUse;

  /// No description provided for @folderPathAbsolute.
  ///
  /// In en, this message translates to:
  /// **'Enter a full path, starting at the root of the host\'s file system.'**
  String get folderPathAbsolute;

  /// No description provided for @folderBreadcrumbTop.
  ///
  /// In en, this message translates to:
  /// **'Start'**
  String get folderBreadcrumbTop;

  /// No description provided for @noProjectsYet.
  ///
  /// In en, this message translates to:
  /// **'No projects yet. Choose a folder to start one.'**
  String get noProjectsYet;

  /// No description provided for @nativeLocalHistoryHint.
  ///
  /// In en, this message translates to:
  /// **'Browse saved sessions on this computer without connecting to an agent. Native history stays read-only.'**
  String get nativeLocalHistoryHint;

  /// No description provided for @nativeHistoryLoadMore.
  ///
  /// In en, this message translates to:
  /// **'Load more sessions'**
  String get nativeHistoryLoadMore;

  /// No description provided for @nativeHistoryLoadOlder.
  ///
  /// In en, this message translates to:
  /// **'Load older messages'**
  String get nativeHistoryLoadOlder;

  /// No description provided for @nativeHistoryWindowHint.
  ///
  /// In en, this message translates to:
  /// **'A bounded history window is shown. Some content is omitted or outside this window.'**
  String get nativeHistoryWindowHint;

  /// No description provided for @providerOpenCode.
  ///
  /// In en, this message translates to:
  /// **'OpenCode'**
  String get providerOpenCode;

  /// No description provided for @agentChoiceNativeServer.
  ///
  /// In en, this message translates to:
  /// **'Native server'**
  String get agentChoiceNativeServer;

  /// No description provided for @hostInheritedOpenCode.
  ///
  /// In en, this message translates to:
  /// **'Uses OpenCode\'s own providers and sign-ins on this computer.'**
  String get hostInheritedOpenCode;

  /// No description provided for @openCodeRuntimeHint.
  ///
  /// In en, this message translates to:
  /// **'Pocket starts and owns one opencode serve on this computer, reachable only through this host.'**
  String get openCodeRuntimeHint;

  /// No description provided for @openCodeDetecting.
  ///
  /// In en, this message translates to:
  /// **'Looking for OpenCode…'**
  String get openCodeDetecting;

  /// No description provided for @openCodeFoundAt.
  ///
  /// In en, this message translates to:
  /// **'OpenCode {version} · {path}'**
  String openCodeFoundAt(String version, String path);

  /// No description provided for @openCodeNotFound.
  ///
  /// In en, this message translates to:
  /// **'OpenCode was not found. Install it or choose its executable.'**
  String get openCodeNotFound;

  /// No description provided for @openCodeUnsupportedVersion.
  ///
  /// In en, this message translates to:
  /// **'This OpenCode version is not supported. Update OpenCode.'**
  String get openCodeUnsupportedVersion;

  /// No description provided for @openCodeNoProvider.
  ///
  /// In en, this message translates to:
  /// **'No provider is signed in. Run `opencode auth login` on this computer, then refresh.'**
  String get openCodeNoProvider;

  /// No description provided for @openCodeAlreadyHosted.
  ///
  /// In en, this message translates to:
  /// **'Another host already uses this OpenCode history. Choose “This host only” for this host, or stop that host first.'**
  String get openCodeAlreadyHosted;

  /// No description provided for @hostNameInUse.
  ///
  /// In en, this message translates to:
  /// **'A host on this computer already uses the name “{name}”. Choose another name.'**
  String hostNameInUse(String name);

  /// No description provided for @managedNameInUse.
  ///
  /// In en, this message translates to:
  /// **'A service on that computer already uses the name “{name}”. Choose another name.'**
  String managedNameInUse(String name);

  /// No description provided for @openCodeHistoryTitle.
  ///
  /// In en, this message translates to:
  /// **'History'**
  String get openCodeHistoryTitle;

  /// No description provided for @openCodeHistoryShared.
  ///
  /// In en, this message translates to:
  /// **'Shared with OpenCode'**
  String get openCodeHistoryShared;

  /// No description provided for @openCodeHistoryOwn.
  ///
  /// In en, this message translates to:
  /// **'This host only'**
  String get openCodeHistoryOwn;

  /// No description provided for @openCodeHistoryInfo.
  ///
  /// In en, this message translates to:
  /// **'Shared: this computer\'s own OpenCode sessions, the same ones the OpenCode terminal app shows. One host can use them at a time.\n\nThis host only: sessions are kept in this host\'s own database, apart from other hosts and the OpenCode terminal app.'**
  String get openCodeHistoryInfo;

  /// No description provided for @openCodeHistoryTaken.
  ///
  /// In en, this message translates to:
  /// **'“{name}” is using the shared history, and only one host can at a time. Choose “This host only”.'**
  String openCodeHistoryTaken(String name);

  /// No description provided for @openCodePath.
  ///
  /// In en, this message translates to:
  /// **'OpenCode executable'**
  String get openCodePath;

  /// No description provided for @openCodePathRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter the OpenCode executable.'**
  String get openCodePathRequired;

  /// No description provided for @openCodeRunning.
  ///
  /// In en, this message translates to:
  /// **'OpenCode {version} is running'**
  String openCodeRunning(String version);

  /// No description provided for @openCodeProviders.
  ///
  /// In en, this message translates to:
  /// **'Providers ready'**
  String get openCodeProviders;

  /// No description provided for @openCodeModels.
  ///
  /// In en, this message translates to:
  /// **'Models'**
  String get openCodeModels;

  /// No description provided for @openCodeDefaultModel.
  ///
  /// In en, this message translates to:
  /// **'Default model'**
  String get openCodeDefaultModel;

  /// No description provided for @openCodeAgents.
  ///
  /// In en, this message translates to:
  /// **'Agents'**
  String get openCodeAgents;

  /// No description provided for @openCodeNone.
  ///
  /// In en, this message translates to:
  /// **'None'**
  String get openCodeNone;

  /// No description provided for @openCodeConfigNote.
  ///
  /// In en, this message translates to:
  /// **'Providers, models and agents come from OpenCode\'s own configuration on this computer, plus the custom provider this host\'s model service may add. Change OpenCode\'s configuration there, then Refresh; restart this host if a change does not show.'**
  String get openCodeConfigNote;

  /// No description provided for @openCodeScopeNote.
  ///
  /// In en, this message translates to:
  /// **'Every Pocket device writes through this one server, so its turns never overlap. OpenCode windows and other OpenCode processes on this computer are not blocked.'**
  String get openCodeScopeNote;

  /// No description provided for @refresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get refresh;

  /// A draft option left to the agent's own default (for example no variant for a newly chosen model).
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get draftOptionDefault;

  /// No description provided for @openCodeRefreshFailed.
  ///
  /// In en, this message translates to:
  /// **'Couldn\'t refresh: {error}'**
  String openCodeRefreshFailed(String error);

  /// No description provided for @sessionHandBack.
  ///
  /// In en, this message translates to:
  /// **'Hand back to other clients'**
  String get sessionHandBack;

  /// No description provided for @accessQueueHeld.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 queued message is waiting while this session was followed. Retry sends it.} other{{count} queued messages are waiting while this session was followed. Retry sends them.}}'**
  String accessQueueHeld(int count);

  /// No description provided for @accessScopeEngine.
  ///
  /// In en, this message translates to:
  /// **'Writing through this host. Other {agent} clients on this computer are not blocked.'**
  String accessScopeEngine(String agent);

  /// No description provided for @accessScopePocket.
  ///
  /// In en, this message translates to:
  /// **'Held by this host. Other Pocket hosts cannot write; native clients are not blocked.'**
  String get accessScopePocket;

  /// No description provided for @accessScopeNone.
  ///
  /// In en, this message translates to:
  /// **'Writing through this host. Other clients are not blocked.'**
  String get accessScopeNone;

  /// No description provided for @accessClientClaude.
  ///
  /// In en, this message translates to:
  /// **'Claude Code'**
  String get accessClientClaude;

  /// No description provided for @accessClientOpenCode.
  ///
  /// In en, this message translates to:
  /// **'Another OpenCode process'**
  String get accessClientOpenCode;

  /// No description provided for @accessClientPocketHost.
  ///
  /// In en, this message translates to:
  /// **'Another Pocket host'**
  String get accessClientPocketHost;

  /// No description provided for @accessClientOther.
  ///
  /// In en, this message translates to:
  /// **'Another client'**
  String get accessClientOther;

  /// No description provided for @accessStatusOpening.
  ///
  /// In en, this message translates to:
  /// **'Opening'**
  String get accessStatusOpening;

  /// No description provided for @accessStatusHandedBack.
  ///
  /// In en, this message translates to:
  /// **'Handed back'**
  String get accessStatusHandedBack;

  /// No description provided for @accessStatusNotOpen.
  ///
  /// In en, this message translates to:
  /// **'Not open here'**
  String get accessStatusNotOpen;

  /// No description provided for @accessStatusChanging.
  ///
  /// In en, this message translates to:
  /// **'Changing hands'**
  String get accessStatusChanging;

  /// No description provided for @accessStatusUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Unavailable'**
  String get accessStatusUnavailable;

  /// No description provided for @accessStatusUnknown.
  ///
  /// In en, this message translates to:
  /// **'Writer unknown'**
  String get accessStatusUnknown;

  /// No description provided for @accessOpening.
  ///
  /// In en, this message translates to:
  /// **'Opening the original session…'**
  String get accessOpening;

  /// No description provided for @accessHandedBackTitle.
  ///
  /// In en, this message translates to:
  /// **'Handed back to other clients'**
  String get accessHandedBackTitle;

  /// No description provided for @accessHandedBackDetail.
  ///
  /// In en, this message translates to:
  /// **'This host closed its writer, so another app can continue the original session. Nothing is taken back by itself.'**
  String get accessHandedBackDetail;

  /// No description provided for @accessNotOpenedTitle.
  ///
  /// In en, this message translates to:
  /// **'This host is not keeping this session open'**
  String get accessNotOpenedTitle;

  /// No description provided for @accessNotOpenedDetail.
  ///
  /// In en, this message translates to:
  /// **'The host holds no admission for it: it may never have been opened there, or the host restarted or let it go to make room. Nothing opens it by itself; Check now or Continue here opens it when nobody else writes it.'**
  String get accessNotOpenedDetail;

  /// No description provided for @accessDeletedDetail.
  ///
  /// In en, this message translates to:
  /// **'It was deleted on the host. Your unsent draft and queued messages stay here.'**
  String get accessDeletedDetail;

  /// No description provided for @accessTransitionTitle.
  ///
  /// In en, this message translates to:
  /// **'This session is changing hands'**
  String get accessTransitionTitle;

  /// No description provided for @accessTransitionDetail.
  ///
  /// In en, this message translates to:
  /// **'The host is loading or closing it.'**
  String get accessTransitionDetail;

  /// No description provided for @accessHostStoppedTitle.
  ///
  /// In en, this message translates to:
  /// **'The {agent} host is not running'**
  String accessHostStoppedTitle(String agent);

  /// No description provided for @accessHostStoppedDetail.
  ///
  /// In en, this message translates to:
  /// **'Start it to continue this session.'**
  String get accessHostStoppedDetail;

  /// No description provided for @accessExternalRunningTitle.
  ///
  /// In en, this message translates to:
  /// **'{client} is running a turn in this session'**
  String accessExternalRunningTitle(String client);

  /// No description provided for @accessExternalRunningDetail.
  ///
  /// In en, this message translates to:
  /// **'Following it live. Nothing can be sent from here meanwhile.'**
  String get accessExternalRunningDetail;

  /// No description provided for @accessExternalIdleTitle.
  ///
  /// In en, this message translates to:
  /// **'{client} still has this session open'**
  String accessExternalIdleTitle(String client);

  /// No description provided for @accessExternalIdleDetail.
  ///
  /// In en, this message translates to:
  /// **'No turn is running. Close the session there, then check again.'**
  String get accessExternalIdleDetail;

  /// No description provided for @accessSourceOpenTitle.
  ///
  /// In en, this message translates to:
  /// **'Another OpenCode process has this history source open'**
  String get accessSourceOpenTitle;

  /// No description provided for @accessSourceOpenDetail.
  ///
  /// In en, this message translates to:
  /// **'Pocket cannot tell which session or turn it writes. Close or release it there, then check now.'**
  String get accessSourceOpenDetail;

  /// No description provided for @accessPocketHostTitle.
  ///
  /// In en, this message translates to:
  /// **'{client} is writing this session'**
  String accessPocketHostTitle(String client);

  /// No description provided for @accessPocketHostRunning.
  ///
  /// In en, this message translates to:
  /// **'It is running a turn now.'**
  String get accessPocketHostRunning;

  /// No description provided for @accessPocketHostIdle.
  ///
  /// In en, this message translates to:
  /// **'No turn is running there, but it still holds the session.'**
  String get accessPocketHostIdle;

  /// No description provided for @accessUnsupportedTitle.
  ///
  /// In en, this message translates to:
  /// **'{agent} cannot reopen this session'**
  String accessUnsupportedTitle(String agent);

  /// No description provided for @accessNotFoundTitle.
  ///
  /// In en, this message translates to:
  /// **'This session is no longer on the host'**
  String get accessNotFoundTitle;

  /// No description provided for @accessUnknownTitle.
  ///
  /// In en, this message translates to:
  /// **'Cannot tell who else writes this session'**
  String get accessUnknownTitle;

  /// No description provided for @accessUnknownDetail.
  ///
  /// In en, this message translates to:
  /// **'The host could not inspect the session\'s other clients.'**
  String get accessUnknownDetail;

  /// No description provided for @accessNextAutomatic.
  ///
  /// In en, this message translates to:
  /// **'When that changes, this view continues by itself if the host still keeps it open here; otherwise Check now opens it.'**
  String get accessNextAutomatic;

  /// No description provided for @accessNextWhenClosed.
  ///
  /// In en, this message translates to:
  /// **'When nobody else holds it, this view continues by itself if the host still keeps it open here; otherwise Check now opens it.'**
  String get accessNextWhenClosed;

  /// No description provided for @accessHolderPid.
  ///
  /// In en, this message translates to:
  /// **'pid {pid}'**
  String accessHolderPid(int pid);

  /// No description provided for @accessHolderSince.
  ///
  /// In en, this message translates to:
  /// **'since {time}'**
  String accessHolderSince(String time);

  /// No description provided for @accessOpenIn.
  ///
  /// In en, this message translates to:
  /// **'Open in {host}'**
  String accessOpenIn(String host);

  /// No description provided for @accessStartHost.
  ///
  /// In en, this message translates to:
  /// **'Open host'**
  String get accessStartHost;

  /// No description provided for @accessCheckNow.
  ///
  /// In en, this message translates to:
  /// **'Check now'**
  String get accessCheckNow;

  /// No description provided for @accessContinueHere.
  ///
  /// In en, this message translates to:
  /// **'Continue here'**
  String get accessContinueHere;

  /// No description provided for @sessionAccessChanged.
  ///
  /// In en, this message translates to:
  /// **'The session changed while it was being opened. Try again.'**
  String get sessionAccessChanged;

  /// No description provided for @permissionAllowOnce.
  ///
  /// In en, this message translates to:
  /// **'Allow once'**
  String get permissionAllowOnce;

  /// No description provided for @permissionAllowAlways.
  ///
  /// In en, this message translates to:
  /// **'Always allow'**
  String get permissionAllowAlways;

  /// No description provided for @permissionRejectOnce.
  ///
  /// In en, this message translates to:
  /// **'Reject'**
  String get permissionRejectOnce;

  /// No description provided for @permissionRejectAlways.
  ///
  /// In en, this message translates to:
  /// **'Always reject'**
  String get permissionRejectAlways;

  /// No description provided for @permissionAlwaysAllows.
  ///
  /// In en, this message translates to:
  /// **'\"Always allow\" covers: {patterns}'**
  String permissionAlwaysAllows(String patterns);

  /// No description provided for @userInputChooseAny.
  ///
  /// In en, this message translates to:
  /// **'Choose any that apply.'**
  String get userInputChooseAny;

  /// No description provided for @commandPaletteLabel.
  ///
  /// In en, this message translates to:
  /// **'{count, plural, =1{1 command} other{{count} commands}}'**
  String commandPaletteLabel(int count);

  /// No description provided for @commandSourceMcp.
  ///
  /// In en, this message translates to:
  /// **'MCP'**
  String get commandSourceMcp;

  /// No description provided for @commandSourceSkill.
  ///
  /// In en, this message translates to:
  /// **'Skill'**
  String get commandSourceSkill;

  /// No description provided for @nativeSessionFree.
  ///
  /// In en, this message translates to:
  /// **'Not open elsewhere'**
  String get nativeSessionFree;

  /// No description provided for @nativeSessionUnknown.
  ///
  /// In en, this message translates to:
  /// **'Holder unknown'**
  String get nativeSessionUnknown;

  /// No description provided for @nativeContinueOn.
  ///
  /// In en, this message translates to:
  /// **'Continue in {host}'**
  String nativeContinueOn(String host);

  /// No description provided for @nativeHostToContinue.
  ///
  /// In en, this message translates to:
  /// **'Host {agent} to continue'**
  String nativeHostToContinue(String agent);

  /// No description provided for @nativeContinueElsewhere.
  ///
  /// In en, this message translates to:
  /// **'Continue on the computer that hosts {agent}.'**
  String nativeContinueElsewhere(String agent);

  /// No description provided for @defaultProjectTitle.
  ///
  /// In en, this message translates to:
  /// **'Default project folder'**
  String get defaultProjectTitle;

  /// No description provided for @defaultProjectAutomatic.
  ///
  /// In en, this message translates to:
  /// **'Automatic, chosen by the host computer'**
  String get defaultProjectAutomatic;

  /// No description provided for @defaultProjectCreatedLater.
  ///
  /// In en, this message translates to:
  /// **'Not created yet. It is created when the first conversation starts here.'**
  String get defaultProjectCreatedLater;

  /// No description provided for @defaultProjectMissing.
  ///
  /// In en, this message translates to:
  /// **'This folder no longer exists on the host. Choose another one.'**
  String get defaultProjectMissing;

  /// No description provided for @defaultProjectNone.
  ///
  /// In en, this message translates to:
  /// **'No default folder. New conversations ask for one.'**
  String get defaultProjectNone;

  /// No description provided for @defaultProjectChange.
  ///
  /// In en, this message translates to:
  /// **'Change folder'**
  String get defaultProjectChange;

  /// No description provided for @defaultProjectUseAutomatic.
  ///
  /// In en, this message translates to:
  /// **'Use automatic folder'**
  String get defaultProjectUseAutomatic;

  /// No description provided for @defaultProjectSaved.
  ///
  /// In en, this message translates to:
  /// **'Default project folder saved.'**
  String get defaultProjectSaved;

  /// No description provided for @defaultProjectConflict.
  ///
  /// In en, this message translates to:
  /// **'Another device changed the project settings. Reload to see them.'**
  String get defaultProjectConflict;

  /// No description provided for @defaultProjectUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Project settings unavailable. Service management is not affected. {error}'**
  String defaultProjectUnavailable(String error);

  /// No description provided for @defaultProjectLabel.
  ///
  /// In en, this message translates to:
  /// **'{name} (default)'**
  String defaultProjectLabel(String name);

  /// No description provided for @projectRootsAdvanced.
  ///
  /// In en, this message translates to:
  /// **'Browsable folders ({count})'**
  String projectRootsAdvanced(int count);

  /// No description provided for @projectRootsHint.
  ///
  /// In en, this message translates to:
  /// **'Other devices can browse only inside these folders. Leave empty to use the host\'s defaults.'**
  String get projectRootsHint;

  /// No description provided for @projectRootPathHint.
  ///
  /// In en, this message translates to:
  /// **'Absolute path on the host, e.g. /Users/me/code'**
  String get projectRootPathHint;

  /// No description provided for @managedComputerTitle.
  ///
  /// In en, this message translates to:
  /// **'Computer management'**
  String get managedComputerTitle;

  /// No description provided for @managedComputerSummary.
  ///
  /// In en, this message translates to:
  /// **'{os} · Pocket {version}'**
  String managedComputerSummary(String os, String version);

  /// No description provided for @managedNoServices.
  ///
  /// In en, this message translates to:
  /// **'No services on this computer yet. Add one below.'**
  String get managedNoServices;

  /// No description provided for @managedAddService.
  ///
  /// In en, this message translates to:
  /// **'Add service'**
  String get managedAddService;

  /// No description provided for @managedAddHint.
  ///
  /// In en, this message translates to:
  /// **'Installed on that computer. Nothing is installed or signed in here.'**
  String get managedAddHint;

  /// No description provided for @managedAddTitle.
  ///
  /// In en, this message translates to:
  /// **'New service on {device}'**
  String managedAddTitle(String device);

  /// No description provided for @managedEditTitle.
  ///
  /// In en, this message translates to:
  /// **'{name} on {device}'**
  String managedEditTitle(String name, String device);

  /// No description provided for @managedNoRuntimes.
  ///
  /// In en, this message translates to:
  /// **'No supported runtime was found on that computer.'**
  String get managedNoRuntimes;

  /// No description provided for @managedRuntimeUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Not available on that computer.'**
  String get managedRuntimeUnavailable;

  /// No description provided for @managedNeedsHostLogin.
  ///
  /// In en, this message translates to:
  /// **'Needs sign-in on that computer to use its own account; a custom provider works without it.'**
  String get managedNeedsHostLogin;

  /// No description provided for @managedKindUnknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown agent'**
  String get managedKindUnknown;

  /// No description provided for @managedStateRunning.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get managedStateRunning;

  /// No description provided for @managedStateStopped.
  ///
  /// In en, this message translates to:
  /// **'Stopped'**
  String get managedStateStopped;

  /// No description provided for @managedStateNew.
  ///
  /// In en, this message translates to:
  /// **'New'**
  String get managedStateNew;

  /// No description provided for @managedStateNeedsAttention.
  ///
  /// In en, this message translates to:
  /// **'Needs attention'**
  String get managedStateNeedsAttention;

  /// No description provided for @managedStart.
  ///
  /// In en, this message translates to:
  /// **'Start'**
  String get managedStart;

  /// No description provided for @managedEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get managedEdit;

  /// No description provided for @managedStarted.
  ///
  /// In en, this message translates to:
  /// **'{name} is running.'**
  String managedStarted(String name);

  /// No description provided for @managedStopped.
  ///
  /// In en, this message translates to:
  /// **'{name} stopped. It stays listed and can be started again.'**
  String managedStopped(String name);

  /// No description provided for @managedSaved.
  ///
  /// In en, this message translates to:
  /// **'Saved.'**
  String get managedSaved;

  /// No description provided for @managedStopConfirmTitle.
  ///
  /// In en, this message translates to:
  /// **'Stop {name}?'**
  String managedStopConfirmTitle(String name);

  /// No description provided for @managedStopConfirmBody.
  ///
  /// In en, this message translates to:
  /// **'Its conversations disconnect and a running turn is interrupted. The computer stays manageable.'**
  String get managedStopConfirmBody;

  /// No description provided for @managedRestoreProblem.
  ///
  /// In en, this message translates to:
  /// **'Automatic restore needs attention: {problem}'**
  String managedRestoreProblem(String problem);

  /// No description provided for @managedConfigUnreadable.
  ///
  /// In en, this message translates to:
  /// **'The saved configuration cannot be read ({problem}). It can still be stopped; fix it on that computer.'**
  String managedConfigUnreadable(String problem);

  /// No description provided for @managedConflict.
  ///
  /// In en, this message translates to:
  /// **'Another device saved this service since you opened it. Your edits are kept; reload to see the new version (your edits are then discarded).'**
  String get managedConflict;

  /// No description provided for @managedOutcomeUnknown.
  ///
  /// In en, this message translates to:
  /// **'The result is unknown (the answer was lost). The current state was refreshed; check it before trying again.'**
  String get managedOutcomeUnknown;

  /// No description provided for @managedHostBusy.
  ///
  /// In en, this message translates to:
  /// **'That computer is busy. Try again shortly.'**
  String get managedHostBusy;

  /// No description provided for @managedAlreadyRunning.
  ///
  /// In en, this message translates to:
  /// **'This service is already running. Refresh, or choose another name.'**
  String get managedAlreadyRunning;

  /// No description provided for @managedHostStopped.
  ///
  /// In en, this message translates to:
  /// **'This service has stopped. Refresh its status.'**
  String get managedHostStopped;

  /// No description provided for @managedHostChanged.
  ///
  /// In en, this message translates to:
  /// **'This service was restarted or replaced elsewhere. Its state was refreshed.'**
  String get managedHostChanged;

  /// No description provided for @managedLoginRequired.
  ///
  /// In en, this message translates to:
  /// **'The service\'s own account is not signed in on that computer. Sign in there, or choose a custom provider.'**
  String get managedLoginRequired;

  /// No description provided for @managedPrepareRequired.
  ///
  /// In en, this message translates to:
  /// **'Prepare the Claude adapter on that computer first (Add service → Prepare ACP environment).'**
  String get managedPrepareRequired;

  /// No description provided for @managedInvalidConfiguration.
  ///
  /// In en, this message translates to:
  /// **'Review and save this service\'s configuration before starting.'**
  String get managedInvalidConfiguration;

  /// No description provided for @managedConfigurationChanged.
  ///
  /// In en, this message translates to:
  /// **'Another device saved this service. Reload before saving.'**
  String get managedConfigurationChanged;

  /// No description provided for @managedOutsideRoots.
  ///
  /// In en, this message translates to:
  /// **'Choose a folder within the host\'s browsable folders.'**
  String get managedOutsideRoots;

  /// No description provided for @managedNameRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter a name for the service.'**
  String get managedNameRequired;

  /// No description provided for @managedNameHelper.
  ///
  /// In en, this message translates to:
  /// **'Unique on that computer; it names the service on your relay.'**
  String get managedNameHelper;

  /// No description provided for @managedPortLabel.
  ///
  /// In en, this message translates to:
  /// **'Port'**
  String get managedPortLabel;

  /// No description provided for @managedPortHelper.
  ///
  /// In en, this message translates to:
  /// **'0 picks a free port.'**
  String get managedPortHelper;

  /// No description provided for @managedPortInvalid.
  ///
  /// In en, this message translates to:
  /// **'Enter a port from 0 to 65535.'**
  String get managedPortInvalid;

  /// No description provided for @managedRuntimeNote.
  ///
  /// In en, this message translates to:
  /// **'Runs the program installed on that computer. Paths and arguments are managed there.'**
  String get managedRuntimeNote;

  /// No description provided for @managedInheritedNote.
  ///
  /// In en, this message translates to:
  /// **'Uses the configuration already on that computer.'**
  String get managedInheritedNote;

  /// No description provided for @managedOfficialNote.
  ///
  /// In en, this message translates to:
  /// **'Uses the official account signed in on that computer. Sign in there if it is not yet.'**
  String get managedOfficialNote;

  /// No description provided for @managedDiagnostics.
  ///
  /// In en, this message translates to:
  /// **'Diagnostics'**
  String get managedDiagnostics;

  /// No description provided for @managedKey.
  ///
  /// In en, this message translates to:
  /// **'Management key'**
  String get managedKey;

  /// No description provided for @managedDevice.
  ///
  /// In en, this message translates to:
  /// **'Device'**
  String get managedDevice;

  /// No description provided for @managedSwitchTitle.
  ///
  /// In en, this message translates to:
  /// **'Allow management from my other devices'**
  String get managedSwitchTitle;

  /// No description provided for @managedSwitchOn.
  ///
  /// In en, this message translates to:
  /// **'On'**
  String get managedSwitchOn;

  /// No description provided for @managedSwitchOff.
  ///
  /// In en, this message translates to:
  /// **'Off'**
  String get managedSwitchOff;

  /// No description provided for @managedSwitchInfo.
  ///
  /// In en, this message translates to:
  /// **'When on, your other signed-in devices can add, configure, start and stop services here while this computer is online. When off, only this computer can change them.'**
  String get managedSwitchInfo;

  /// No description provided for @managedSwitchConnecting.
  ///
  /// In en, this message translates to:
  /// **'On, not reachable yet.'**
  String get managedSwitchConnecting;

  /// No description provided for @managedSwitchUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Management status could not be read.'**
  String get managedSwitchUnavailable;

  /// No description provided for @hostRestoreBlocked.
  ///
  /// In en, this message translates to:
  /// **'Automatic restore was skipped because its saved choices cannot be read: {problem}'**
  String hostRestoreBlocked(String problem);

  /// No description provided for @managedRuntimeReady.
  ///
  /// In en, this message translates to:
  /// **'Ready'**
  String get managedRuntimeReady;

  /// No description provided for @managedRuntimeNeedsSetup.
  ///
  /// In en, this message translates to:
  /// **'Needs setup'**
  String get managedRuntimeNeedsSetup;

  /// No description provided for @managedRuntimeSignInThere.
  ///
  /// In en, this message translates to:
  /// **'Ready · sign in on that computer'**
  String get managedRuntimeSignInThere;

  /// No description provided for @managedRuntimeDetails.
  ///
  /// In en, this message translates to:
  /// **'Details'**
  String get managedRuntimeDetails;

  /// No description provided for @managedRuntimePath.
  ///
  /// In en, this message translates to:
  /// **'Executable'**
  String get managedRuntimePath;

  /// No description provided for @managedRuntimeVersion.
  ///
  /// In en, this message translates to:
  /// **'Version'**
  String get managedRuntimeVersion;

  /// No description provided for @refreshServices.
  ///
  /// In en, this message translates to:
  /// **'Refresh services'**
  String get refreshServices;

  /// No description provided for @refreshServicesFailed.
  ///
  /// In en, this message translates to:
  /// **'Services could not be refreshed. Tap to try again.'**
  String get refreshServicesFailed;

  /// No description provided for @managedRefreshComputer.
  ///
  /// In en, this message translates to:
  /// **'Refresh this computer'**
  String get managedRefreshComputer;

  /// No description provided for @composerWrite.
  ///
  /// In en, this message translates to:
  /// **'Write'**
  String get composerWrite;

  /// No description provided for @composerSplit.
  ///
  /// In en, this message translates to:
  /// **'Split'**
  String get composerSplit;

  /// No description provided for @composerPreview.
  ///
  /// In en, this message translates to:
  /// **'Preview'**
  String get composerPreview;

  /// No description provided for @composerPreviewEmpty.
  ///
  /// In en, this message translates to:
  /// **'Nothing to preview yet. Write Markdown on the left and it renders here.'**
  String get composerPreviewEmpty;

  /// No description provided for @composerPreviewEmptyTab.
  ///
  /// In en, this message translates to:
  /// **'Nothing to preview yet.'**
  String get composerPreviewEmptyTab;

  /// No description provided for @reviewSessionSource.
  ///
  /// In en, this message translates to:
  /// **'This conversation'**
  String get reviewSessionSource;

  /// No description provided for @reviewWorkspaceSource.
  ///
  /// In en, this message translates to:
  /// **'Workspace'**
  String get reviewWorkspaceSource;

  /// No description provided for @reviewNoSessionChanges.
  ///
  /// In en, this message translates to:
  /// **'This conversation hasn\'t changed any files yet.'**
  String get reviewNoSessionChanges;

  /// No description provided for @reviewEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit {index}'**
  String reviewEdit(int index);

  /// No description provided for @reviewAllEdits.
  ///
  /// In en, this message translates to:
  /// **'Show every edit'**
  String get reviewAllEdits;

  /// No description provided for @reviewEditCount.
  ///
  /// In en, this message translates to:
  /// **'{count} edits'**
  String reviewEditCount(int count);

  /// No description provided for @reviewFilterFiles.
  ///
  /// In en, this message translates to:
  /// **'Filter files'**
  String get reviewFilterFiles;

  /// No description provided for @reviewOpenFile.
  ///
  /// In en, this message translates to:
  /// **'Open the whole file'**
  String get reviewOpenFile;

  /// No description provided for @reviewShowChanges.
  ///
  /// In en, this message translates to:
  /// **'Show the changes'**
  String get reviewShowChanges;

  /// No description provided for @reviewFileLoading.
  ///
  /// In en, this message translates to:
  /// **'Reading the file…'**
  String get reviewFileLoading;

  /// No description provided for @reviewFileUnavailable.
  ///
  /// In en, this message translates to:
  /// **'This file can\'t be read from the host.'**
  String get reviewFileUnavailable;

  /// No description provided for @copyPath.
  ///
  /// In en, this message translates to:
  /// **'Copy path'**
  String get copyPath;

  /// No description provided for @openInReview.
  ///
  /// In en, this message translates to:
  /// **'Open in review'**
  String get openInReview;

  /// No description provided for @showTurnNavigation.
  ///
  /// In en, this message translates to:
  /// **'Show conversation controls'**
  String get showTurnNavigation;

  /// No description provided for @automationRun.
  ///
  /// In en, this message translates to:
  /// **'Automation'**
  String get automationRun;

  /// No description provided for @reviewWorkspaceLoading.
  ///
  /// In en, this message translates to:
  /// **'Reading the working tree\'s changes…'**
  String get reviewWorkspaceLoading;

  /// No description provided for @reviewWorkspaceUnavailable.
  ///
  /// In en, this message translates to:
  /// **'The working tree\'s changes couldn\'t be read.'**
  String get reviewWorkspaceUnavailable;

  /// No description provided for @relayVirtualAddress.
  ///
  /// In en, this message translates to:
  /// **'The system DNS resolved {host} to {address}, a proxy\'s or VPN\'s virtual address that nothing routes right now. Reconnect that proxy or VPN, then retry.'**
  String relayVirtualAddress(String host, String address);

  /// No description provided for @relayNoAnswer.
  ///
  /// In en, this message translates to:
  /// **'{relay} didn\'t answer.'**
  String relayNoAnswer(String relay);

  /// No description provided for @homeRelayLocalOnly.
  ///
  /// In en, this message translates to:
  /// **'The relay can\'t be reached, so this computer\'s own service opened directly.'**
  String get homeRelayLocalOnly;

  /// No description provided for @hostStopping.
  ///
  /// In en, this message translates to:
  /// **'Stopping…'**
  String get hostStopping;

  /// No description provided for @hostSaving.
  ///
  /// In en, this message translates to:
  /// **'Saving…'**
  String get hostSaving;

  /// No description provided for @hostRestarting.
  ///
  /// In en, this message translates to:
  /// **'Restarting…'**
  String get hostRestarting;

  /// No description provided for @hostStoppedToast.
  ///
  /// In en, this message translates to:
  /// **'{name} stopped.'**
  String hostStoppedToast(String name);

  /// No description provided for @conversationsLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading conversations…'**
  String get conversationsLoading;
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
