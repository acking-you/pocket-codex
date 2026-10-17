// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get appTitle => 'PocketCodex';

  @override
  String get webUnsupported =>
      'PocketCodex needs local network and file access; Web is not supported.\nUse Android / iOS / desktop.';

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
  String get accountWelcomeHeadline => 'Welcome to PocketCodex';

  @override
  String get accountWelcomeSubtitle =>
      'Sign in to pick up your conversations on any device.';

  @override
  String get accountSignInTitle => 'Sign in';

  @override
  String get accountSignInButton => 'Sign in with GitHub';

  @override
  String get accountUseDeviceCode => 'Use a device code instead';

  @override
  String get accountSessionExpiredTitle => 'Your session expired';

  @override
  String get accountSessionExpiredMessage =>
      'Your saved sign-in can no longer be renewed. Sign in again to reconnect to your services.';

  @override
  String get accountSignInAgain => 'Sign in again';

  @override
  String get accountWebFailed => 'Sign-in didn\'t complete. Please try again.';

  @override
  String get accountWebTrouble =>
      'Browser sign-in didn\'t finish — either the GitHub page wouldn\'t load, or the browser couldn\'t hand the result back to this app. The device code below avoids the hand-off entirely.';

  @override
  String get accountSignedIn => 'Signed in';

  @override
  String accountSignedInAs(String login) {
    return 'Signed in as @$login';
  }

  @override
  String get accountEnterCode =>
      'Enter this code on GitHub to finish signing in:';

  @override
  String get accountCopyCode => 'Copy code';

  @override
  String get accountOpenGitHub => 'Open GitHub';

  @override
  String get accountWaiting => 'Waiting for you to authorize on GitHub…';

  @override
  String get accountCodeExpired => 'The code expired. Please try again.';

  @override
  String get accountDenied => 'Sign-in was denied on GitHub.';

  @override
  String get accountAdvancedSelfHost => 'Use a self-hosted relay instead';

  @override
  String get accountAdvanced => 'Advanced / self-hosted';

  @override
  String get accountBackendHint => 'Backend URL (blank = default)';

  @override
  String get accountSection => 'Account';

  @override
  String get accountSignOut => 'Sign out';

  @override
  String get connecting => 'Connecting…';

  @override
  String get conversationsSection => 'Conversations';

  @override
  String get newConversation => 'New conversation';

  @override
  String newConversationInProject(String project) {
    return 'New conversation in $project';
  }

  @override
  String get copyProjectPath => 'Copy project path';

  @override
  String get noThreads => 'No conversations yet';

  @override
  String get untitledThread => '(untitled)';

  @override
  String get renameConversation => 'Rename';

  @override
  String get renameConversationHint => 'Conversation name';

  @override
  String get renameHint => 'Click to rename';

  @override
  String retryingAttempt(int attempt, int max) {
    return 'Retrying… $attempt/$max';
  }

  @override
  String get renameFailed => 'Couldn\'t rename the conversation';

  @override
  String showMoreCount(int count) {
    return 'Show $count more';
  }

  @override
  String get showLess => 'Show less';

  @override
  String get messageHint => 'Message…';

  @override
  String get send => 'Send';

  @override
  String get voiceLive => 'Live voice';

  @override
  String get voiceSessionEnded => 'Live voice · ended';

  @override
  String get voiceYou => 'You';

  @override
  String get voiceAgent => 'Agent';

  @override
  String get addAttachment => 'Add';

  @override
  String get turnSettings => 'Turn settings';

  @override
  String get prevTurn => 'Previous turn';

  @override
  String get nextTurn => 'Next turn';

  @override
  String get jumpToLatest => 'Jump to latest';

  @override
  String get attachImage => 'Attach image';

  @override
  String get removeImage => 'Remove image';

  @override
  String get imagePickFailed => 'Couldn\'t read the selected image';

  @override
  String get imagesNotAccepted => 'This agent doesn\'t accept images';

  @override
  String get attachmentRevoked =>
      'Not attached: the conversation changed while the file was read. Attach it again.';

  @override
  String imageTooMany(int count) {
    return 'At most $count images per message';
  }

  @override
  String get imageSave => 'Save image';

  @override
  String get previewImage => 'Click to preview';

  @override
  String get imageLoadFailed => 'Couldn\'t load this image';

  @override
  String imageSaved(String path) {
    return 'Saved to $path';
  }

  @override
  String imageSaveFailed(String error) {
    return 'Couldn\'t save the image: $error';
  }

  @override
  String imageOnHost(String path) {
    return 'Image file on the host machine: $path';
  }

  @override
  String get imageOnlyMessage => '[Image]';

  @override
  String get attachFile => 'Attach file';

  @override
  String get dropToAttach => 'Drop to attach';

  @override
  String queuedCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count messages queued',
      one: '1 message queued',
    );
    return '$_temp0';
  }

  @override
  String get queuedHint =>
      'Sends after the current reply. Esc puts the last one back.';

  @override
  String get removeQueued => 'Remove from queue';

  @override
  String get removeFile => 'Remove file';

  @override
  String get filePickFailed => 'Couldn\'t read the selected file';

  @override
  String fileTooMany(int count) {
    return 'At most $count files per message';
  }

  @override
  String fileTooLarge(int mb) {
    return 'File exceeds the $mb MB limit';
  }

  @override
  String get fileOnlyMessage => '[File]';

  @override
  String get emptyConversation => 'Send a message to start the conversation';

  @override
  String get olderHistoryHint => 'Load earlier messages';

  @override
  String get historyStart => 'Start of conversation';

  @override
  String get historyLoading => 'Loading history…';

  @override
  String get historyRetry => 'Couldn’t load history. Retry';

  @override
  String get turnFailed =>
      'Turn didn\'t finish — the connection dropped or the remote codex failed. Retry, or check codex on the host machine (it may need to be logged in again).';

  @override
  String get sandboxHelperUnavailable =>
      'This built-in (自带) session can\'t start its command sandbox, so the agent can\'t run commands or read files here. Switch to \"Full access\" mode to run without a sandbox, or connect to an external/remote codex host.';

  @override
  String get connectionLost => 'Connection lost';

  @override
  String get reconnect => 'Reconnect';

  @override
  String get projectsSection => 'Projects';

  @override
  String get newProject => 'New project';

  @override
  String get effortFaster => 'Faster';

  @override
  String get effortSmarter => 'Smarter';

  @override
  String get searchProjects => 'Search projects';

  @override
  String get noMatchingProjects => 'No matching projects';

  @override
  String get workOutsideProject => 'Work outside a project';

  @override
  String get remotePathLabel => 'Remote folder path (on the host)';

  @override
  String get remotePathHint =>
      'e.g. /home/ubuntu/myproject — blank uses the host default';

  @override
  String get model => 'Model';

  @override
  String get modelDefault => 'Default model';

  @override
  String get defaultFolder => 'Default folder';

  @override
  String get permissionMode => 'Permission';

  @override
  String get modeReadOnly => 'Read-only';

  @override
  String get modeReadOnlyDesc => 'Ask before running; no file writes';

  @override
  String get modeAuto => 'Auto';

  @override
  String get modeAutoDesc =>
      'Let Codex review approval requests for you, within the workspace sandbox.';

  @override
  String get modeFull => 'Full access';

  @override
  String get modeFullDesc => 'No sandbox, never ask (use with care)';

  @override
  String get approvalPrompt => 'The agent wants to run a command';

  @override
  String get approvalFilePrompt => 'The agent wants to edit files';

  @override
  String get approvalPermissionPrompt =>
      'The agent requests additional permission';

  @override
  String get approve => 'Approve';

  @override
  String get approveForSession => 'Approve for session';

  @override
  String get deny => 'Deny';

  @override
  String get userInputTitle => 'The agent needs your input';

  @override
  String get userInputSubmit => 'Submit';

  @override
  String get userInputOther => 'Other…';

  @override
  String get planMode => 'Plan';

  @override
  String get planReadyTitle => 'Plan ready';

  @override
  String get implementPlan => 'Implement plan';

  @override
  String get keepPlanning => 'Keep planning';

  @override
  String get implementPlanPrompt => 'Go ahead and implement the plan above.';

  @override
  String get noModelForMode => 'Can\'t switch mode: no model is available';

  @override
  String get modelWithdrawn =>
      'This host no longer offers the selected model. Choose a model and send again.';

  @override
  String get effort => 'Effort';

  @override
  String get effortMinimal => 'Minimal';

  @override
  String get effortMinimalDesc => 'Least thinking, fastest';

  @override
  String get effortLow => 'Low';

  @override
  String get effortLowDesc => 'A little thinking';

  @override
  String get effortMedium => 'Medium';

  @override
  String get effortMediumDesc => 'Balanced (usual default)';

  @override
  String get effortHigh => 'High';

  @override
  String get effortHighDesc => 'Thorough';

  @override
  String get effortXhigh => 'Extra high';

  @override
  String get effortXhighDesc => 'Most thorough, slowest';

  @override
  String get linkOpenFailed => 'Couldn\'t open the link';

  @override
  String get contextLabel => 'Context';

  @override
  String get contextUsageTitle => 'Context & usage';

  @override
  String get quota5h => '5-hour limit';

  @override
  String get quotaWeekly => 'Weekly limit';

  @override
  String get quotaUnavailable => 'Quota information is unavailable.';

  @override
  String get quotaRemaining => 'Usage left';

  @override
  String resetsIn(String span) {
    return 'resets in $span';
  }

  @override
  String get moreActions => 'More';

  @override
  String get backToSessions => 'Back to sessions';

  @override
  String get stateReady => 'Ready';

  @override
  String get stateWorking => 'Working…';

  @override
  String get statePlanning => 'Planning…';

  @override
  String get statePlanMode => 'Plan mode';

  @override
  String get stateDisconnected => 'Disconnected';

  @override
  String get stateReconnecting => 'Reconnecting…';

  @override
  String get compactingContext => 'Compacting context…';

  @override
  String get compacted => 'Conversation compacted';

  @override
  String get turnStopped => 'Stopped';

  @override
  String get modelErrorTitle => 'The model couldn\'t respond';

  @override
  String modelErrorModel(String model) {
    return 'Model: $model';
  }

  @override
  String modelErrorStatus(String status) {
    return 'HTTP $status';
  }

  @override
  String get modelErrorRegion =>
      'This model isn\'t available in your region. Choose another model, then retry.';

  @override
  String get modelErrorAuthentication =>
      'The provider couldn\'t sign you in. Check its API key or sign-in, then retry.';

  @override
  String get modelErrorPermission =>
      'The provider denied access to this model. Choose another model or check your account.';

  @override
  String get modelErrorUnavailable =>
      'The model or its endpoint is unavailable. Choose another model or check the provider configuration.';

  @override
  String get modelErrorRateLimited =>
      'The provider is receiving too many requests. Wait a moment, then retry.';

  @override
  String get modelErrorQuota =>
      'The provider\'s usage allowance is used up. Check your account or choose another model.';

  @override
  String get modelErrorContext =>
      'The conversation is longer than this model accepts. Choose a model with a larger context or start a new conversation.';

  @override
  String get modelErrorOutput =>
      'The model reached its output limit before finishing. Ask it to continue.';

  @override
  String get modelErrorProvider =>
      'The provider is temporarily unavailable. Retry later or choose another model.';

  @override
  String get modelErrorInvalid =>
      'The provider rejected this request. Check the model and the message, then retry.';

  @override
  String get modelErrorUnknown =>
      'The model couldn\'t complete the response. Retry, or choose another model.';

  @override
  String get modelErrorRetry => 'Retry';

  @override
  String get modelErrorChangeModel => 'Change model';

  @override
  String get modelErrorIncomplete =>
      'The failed message isn\'t fully available here, so it can\'t be sent again as recorded. Edit it or send a new message.';

  @override
  String modelChangeFailed(String reason) {
    return 'Couldn\'t switch the model: $reason. The previous model stays selected and your draft is unchanged.';
  }

  @override
  String get modelErrorRetryHint =>
      'Sends this message again with the model selected now.';

  @override
  String get reasoningOutputNote =>
      'The model returned this as reasoning, without a separate answer.';

  @override
  String get queuedPausedHint => 'Paused. Retry or send a message to continue.';

  @override
  String turnElapsed(String duration) {
    return 'Took $duration';
  }

  @override
  String turnProcessed(String duration) {
    return 'Worked for $duration';
  }

  @override
  String turnProcessing(int count) {
    return 'Working, $count steps so far';
  }

  @override
  String turnActivityCount(int count) {
    return '$count steps';
  }

  @override
  String completedAt(String time) {
    return 'Completed at $time';
  }

  @override
  String get activeModelTooltip => 'Active model & settings — tap for details';

  @override
  String get runtimeSheetTitle => 'Runtime configuration';

  @override
  String get runtimeEffortModelDefault => 'Model default';

  @override
  String get runtimeCollabDefault => 'Default';

  @override
  String runtimeConfirmedAt(String time) {
    return 'Confirmed by the server · $time';
  }

  @override
  String runtimeFromSnapshot(String time) {
    return 'From the server\'s session snapshot · $time';
  }

  @override
  String get runtimeUnavailable =>
      'This server doesn\'t report runtime settings; showing what the app sends.';

  @override
  String get runtimeConfirmed => 'Confirmed by the server';

  @override
  String get runtimeUnconfirmed =>
      'As sent by the app — the server didn\'t report back';

  @override
  String turnHandledBy(String model) {
    return 'Handled by $model';
  }

  @override
  String get modelReroutedNote => 'The server switched to this model mid-turn';

  @override
  String get refreshStatus => 'Refresh status';

  @override
  String get statusOnline => 'Online';

  @override
  String get statusConnected => 'Connected';

  @override
  String get statusChecking => 'Checking…';

  @override
  String get statusUnreachable => 'Unreachable';

  @override
  String get unreachableReason =>
      'Still registered on the relay, but the remote app-server isn\'t responding — it may not be running, or has crashed.';

  @override
  String get unreachableAuthRejected =>
      'The relay refused the connection: the authentication code is missing or no longer valid. The tunnel itself works and the remote host is not down — sign in again, or re-enter the relay key, under Manage services.';

  @override
  String get unreachableSilent =>
      'The tunnel is up, but the remote app-server didn\'t answer before the timeout — it may still be starting, wedged, or gone.';

  @override
  String get apiUnreachableReason =>
      'Still registered on the relay, but the remote API service isn\'t responding — it may not be running, or has crashed.';

  @override
  String get subscribedAlive => 'Subscribed';

  @override
  String get subscribedDead => 'Dropped';

  @override
  String get compact => 'Compact conversation';

  @override
  String get compactConfirm =>
      'Summarise and shrink this conversation to free up context? This can\'t be undone.';

  @override
  String get viewDiff => 'View changes';

  @override
  String get cancelDiffLoad => 'Cancel loading changes';

  @override
  String get changesTitle => 'Changes';

  @override
  String get noChanges => 'No changes vs the main branch.';

  @override
  String get envTitle => 'Environment';

  @override
  String get envLocal => 'Local';

  @override
  String get envSource => 'Source';

  @override
  String get envRefresh => 'Refresh';

  @override
  String diffTruncated(int count) {
    return '$count more lines not shown — copy the path to view the full diff';
  }

  @override
  String diffUnmodified(int count) {
    return '$count unmodified lines';
  }

  @override
  String diffExpandFailed(int count) {
    return '$count unmodified lines (couldn\'t load)';
  }

  @override
  String get reviewTitle => 'Review';

  @override
  String get reviewNoFiles => 'No changed files.';

  @override
  String get reviewPickFile => 'Pick a file to review its changes.';

  @override
  String envFilesChanged(int count) {
    return '$count files changed';
  }

  @override
  String turnProgressStep(int current, int total) {
    return 'Step $current of $total';
  }

  @override
  String get start => 'Start';

  @override
  String get create => 'Create';

  @override
  String get copy => 'Copy';

  @override
  String get copied => 'Copied';

  @override
  String get toolSearched => 'Searched the web';

  @override
  String get toolRan => 'Ran command';

  @override
  String get toolEdited => 'Edited files';

  @override
  String get toolCalled => 'Used a tool';

  @override
  String get toolCollaborated => 'Coordinated agents';

  @override
  String get toolSubAgent => 'Subagent activity';

  @override
  String get toolViewedImage => 'Viewed image';

  @override
  String get toolGeneratedImage => 'Generated image';

  @override
  String get toolWaited => 'Waited';

  @override
  String get toolHook => 'Hook prompt';

  @override
  String get toolEnteredReview => 'Entered review';

  @override
  String get toolExitedReview => 'Exited review';

  @override
  String get toolThinking => 'Thinking';

  @override
  String get toolPlan => 'Plan';

  @override
  String get toolActivity => 'Activity';

  @override
  String get relayNotConfigured => '(no relay configured)';

  @override
  String get retry => 'Retry';

  @override
  String get discoverFailed => 'Couldn\'t reach the relay';

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
  String get utilityChat => 'Conversation';

  @override
  String get utilityPages => 'Pages';

  @override
  String get utilitySwitchPage => 'Switch page';

  @override
  String get settingsGeneral => 'General';

  @override
  String get settingsAccountConnection => 'Account & connection';

  @override
  String get settingsServicesSubscriptions => 'Services & subscriptions';

  @override
  String get settingsAdvanced => 'Advanced';

  @override
  String get settingsSelfHostedRelay => 'Self-hosted relay fallback';

  @override
  String get settingsSelfHostedKey => 'Self-hosted relay key';

  @override
  String get settingsExportUnavailable =>
      'Configure a self-hosted relay and 32-byte key first.';

  @override
  String settingsOperationFailed(String error) {
    return 'Operation failed: $error';
  }

  @override
  String get settingsDiagnostics => 'Diagnostics';

  @override
  String get settingsConfigure => 'Configure';

  @override
  String get settingsEdit => 'Edit';

  @override
  String get trayShow => 'Show window';

  @override
  String get trayQuit => 'Quit';

  @override
  String get windowMinimize => 'Minimize';

  @override
  String get windowMaximize => 'Maximize';

  @override
  String get windowRestore => 'Restore';

  @override
  String get windowClose => 'Close';

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

  @override
  String get appearance => 'Appearance';

  @override
  String get appearanceSystem => 'Follow system';

  @override
  String get appearanceLight => 'Light';

  @override
  String get appearanceDark => 'Dark';

  @override
  String get searchConversations => 'Search conversations';

  @override
  String get searchLocalSessions => 'Search content / path / source';

  @override
  String get noMatchingThreads => 'No matching conversations';

  @override
  String get groupActive => 'Active';

  @override
  String get groupToday => 'Today';

  @override
  String get groupYesterday => 'Yesterday';

  @override
  String get groupEarlier => 'Earlier';

  @override
  String get activityView => 'Activity';

  @override
  String get running => 'Running…';

  @override
  String get timeJustNow => 'just now';

  @override
  String timeMinutesAgo(int n) {
    return '${n}m ago';
  }

  @override
  String timeHoursAgo(int n) {
    return '${n}h ago';
  }

  @override
  String get timeYesterday => 'yesterday';

  @override
  String timeDaysAgo(int n) {
    return '${n}d ago';
  }

  @override
  String get modelLabel => 'Model';

  @override
  String get permissionLabel => 'Permission';

  @override
  String get localSessions => 'Local sessions';

  @override
  String get localSessionsTitle => 'Local sessions';

  @override
  String get noLocalSessions => 'No local sessions';

  @override
  String get localSessionsUnavailable =>
      'Local sessions aren’t available on this device';

  @override
  String get localSessionsMobileHint =>
      'This page reads Codex sessions saved on this device. To browse your computer’s sessions, connect to it in Services. Cached conversations remain in Chats.';

  @override
  String get noLocalSessionsHint =>
      'This computer has no readable Codex sessions yet. Create a session with Codex, then refresh, or connect to another device.';

  @override
  String get noHostSessions => 'No sessions on this host';

  @override
  String get noHostSessionsHint =>
      'This host has no sessions to display yet. Create a Codex session on it, then refresh.';

  @override
  String get sessionsReadFailed => 'Couldn’t read sessions';

  @override
  String get localSessionsReadHint =>
      'Check that Codex has saved sessions for this user and that its session folder is readable, then retry.';

  @override
  String get remoteSessionsReadHint =>
      'The host’s sessions are temporarily unavailable. Check its connection, then retry.';

  @override
  String get errorDetails => 'Error details';

  @override
  String get moveTurnNavigation => 'Drag to move conversation controls';

  @override
  String historyLoaded(int count) {
    return 'Loaded $count earlier items';
  }

  @override
  String get historyAlreadyVisible => 'This history is already displayed';

  @override
  String get sessionResumable => 'Resumable';

  @override
  String get sessionUnfinished => 'Last turn interrupted';

  @override
  String get sessionRunningElsewhere => 'Running elsewhere';

  @override
  String get sessionInUseElsewhere => 'In use elsewhere';

  @override
  String get sessionReadOnly => 'Read-only';

  @override
  String get readOnlyViewing =>
      'Read-only — another client is using this session';

  @override
  String get sessionTranscriptEmpty => 'Nothing to show yet';

  @override
  String get resumeSession => 'Resume';

  @override
  String get forceTakeover => 'Force takeover';

  @override
  String get takeoverTitle => 'Force takeover?';

  @override
  String takeoverBody(int n) {
    return 'This session is held open by $n other process(es). PocketCodex will try to terminate them, then resume it here. Any unsaved work in those processes will be lost.';
  }

  @override
  String get takeoverWillTerminate => 'Will terminate';

  @override
  String get takeoverConfirm => 'Terminate & resume';

  @override
  String get takeoverResumed => 'Session resumed';

  @override
  String takeoverKilled(int n) {
    return 'Terminated $n process(es)';
  }

  @override
  String get takeoverStillHeld => 'Still held open — resumed anyway';

  @override
  String takeoverResumeFailed(String error) {
    return 'Resume failed: $error';
  }

  @override
  String get takeoverNoTarget =>
      'Connect to an app-server service first to resume.';

  @override
  String holderRow(String name, int pid) {
    return '$name · PID $pid';
  }

  @override
  String get localHostingSection => 'Local hosting';

  @override
  String get localHostRunning => 'Hosting';

  @override
  String get localHostStarting => 'Starting…';

  @override
  String get localHostUnresponsive => 'Not responding';

  @override
  String get startHosting => 'Start hosting';

  @override
  String get stopHosting => 'Stop hosting';

  @override
  String get localHostStopFailed => 'Did not stop: stop again';

  @override
  String get localHostPort => 'Port';

  @override
  String get localHostName => 'Instance name';

  @override
  String get codexBinaryPath => 'codex binary path';

  @override
  String get codexNotFound =>
      'codex wasn\'t found on PATH — enter its full path below, or install it and tap Re-detect.';

  @override
  String get codexRedetect => 'Re-detect';

  @override
  String get codexSourceExternal => 'External Codex';

  @override
  String get codexSourceBuiltin => 'Built-in engine';

  @override
  String get codexBuiltinNote => 'Not implemented yet. Use external Codex.';

  @override
  String get codexBuiltinUnavailable => 'Not implemented yet';

  @override
  String get hostRuntimeInfo => 'Runtime';

  @override
  String get hostCodexPath => 'Path';

  @override
  String get hostProxyLabel => 'Proxy';

  @override
  String get hostProxyInherit => 'inherits the app\'s environment';

  @override
  String get localHostDialogTitle => 'Host on this device';

  @override
  String get localHostHint =>
      'Runs codex on this machine and publishes it on your relay, so your other devices can drive it.';

  @override
  String codexFoundAt(String path) {
    return 'codex found: $path';
  }

  @override
  String get chooseCodexPath => 'Choose codex binary…';

  @override
  String get codexPathRequired => 'Choose the codex binary to continue.';

  @override
  String get useProxy => 'Use a proxy';

  @override
  String get proxyLabel => 'Proxy';

  @override
  String get proxyRequired => 'Enter a proxy, or turn off “Use a proxy”.';

  @override
  String get noProxyWarning =>
      'Without a proxy, codex on this machine may fail to reach chatgpt.com.';

  @override
  String get addLocalHost => 'Host another…';

  @override
  String get customizeCodexPath => 'Change path';

  @override
  String get deregister => 'Deregister';

  @override
  String get reregister => 'Re-register';

  @override
  String get deregisterTitle => 'Deregister this service?';

  @override
  String deregisterWarning(String name) {
    return 'Remove “$name” from your account\'s relay listing. If a host is still running it, it will re-register within seconds — stop that host to remove it for good.';
  }

  @override
  String deregisterLocalWarning(String name) {
    return 'Take “$name” off the relay. codex and the API proxy keep running — re-register it from the Local hosting card anytime.';
  }

  @override
  String get deregisterOrphanTitle => 'Remove this unreachable service?';

  @override
  String deregisterOrphanWarning(String name) {
    return '“$name” isn\'t responding — its registration is lingering on the relay with no reachable host. This removes it from your list (and keeps it hidden even if the stale registration lingers), and asks the backend to drop it.';
  }

  @override
  String get remove => 'Remove';

  @override
  String get deregisterFailed => 'Couldn\'t deregister the service';

  @override
  String get hostNameConflict =>
      'This server name is already online from another instance. Stop that instance first, or host under a different name. (A just-stopped server frees its name within ~15 seconds.)';

  @override
  String get batchRemoveEnter => 'Clean up unreachable';

  @override
  String get batchRemoveTitle => 'Remove these unreachable services?';

  @override
  String batchRemoveWarning(int count) {
    return 'Remove $count unreachable services from this device\'s list. Any that recover will re-appear.';
  }

  @override
  String batchRemovedSnack(int count) {
    return 'Removed $count services';
  }

  @override
  String get tunnelAppLabel => 'App-server';

  @override
  String get tunnelApiLabel => 'API';

  @override
  String get tunnelOffline => 'Offline';

  @override
  String get tunnelNotConnected => 'Not connected';

  @override
  String get logsTitle => 'Runtime logs';

  @override
  String get logsLive => 'Live capture';

  @override
  String get logsLevelAll => 'All';

  @override
  String get logsKeywordHint => 'error, host, tunnel…';

  @override
  String get logsCopy => 'Copy visible logs';

  @override
  String get logsClear => 'Clear logs';

  @override
  String get logsScrollBottom => 'Scroll to bottom';

  @override
  String get logsEmpty => 'No logs yet';

  @override
  String logsVisible(int visible, int total) {
    return '$visible/$total shown';
  }

  @override
  String logsCopied(int count) {
    return 'Copied $count log lines';
  }

  @override
  String get manageServices => 'Manage services';

  @override
  String get servicesHostThisDevice => 'Host this device';

  @override
  String get servicesDevices => 'Devices';

  @override
  String servicesDeviceCapabilityCount(int count) {
    return '$count capabilities';
  }

  @override
  String get servicesLocalDevice => 'This device';

  @override
  String get servicesPlatformUnknown => 'Platform unknown';

  @override
  String get servicesPublisherInstance => 'Publisher instance';

  @override
  String get servicesHostInstance => 'Hosting instance';

  @override
  String get servicesCapabilities => 'Available capabilities';

  @override
  String get servicesChatCapability => 'Conversation service';

  @override
  String get servicesApiCapability => 'Responses API';

  @override
  String get servicesSessionsCapability => 'Session sharing';

  @override
  String get servicesDefault => 'Default';

  @override
  String get servicesSetDefault => 'Set as default';

  @override
  String get servicesOpen => 'Open';

  @override
  String get servicesManage => 'Manage';

  @override
  String get servicesBrowse => 'Browse';

  @override
  String get servicesNoCapabilities =>
      'No capabilities are currently available on this device';

  @override
  String get hostSessions => 'Host session history';

  @override
  String get homeConnecting => 'Connecting to your Codex host…';

  @override
  String get homeRestoringHost => 'Restoring hosting…';

  @override
  String get homeNoServiceTitle => 'No Codex host available';

  @override
  String get homeNoServiceDesktopHint =>
      'Start hosting on this computer and the chat opens right here — your phone can connect to it too.';

  @override
  String get homeNoServiceMobileHint =>
      'Open PocketCodex on your computer and start hosting (app-server); this screen will enter the chat automatically.';

  @override
  String get homeAutoRetryNote => 'Keeps checking for hosts automatically.';

  @override
  String get switchServiceFailed =>
      'Couldn\'t reach that host — staying on the current one.';

  @override
  String get hostSwitchTitle => 'Switch host';

  @override
  String acpSavedAgentsFull(int count) {
    return 'This configuration was not saved: $count configurations are already saved.';
  }

  @override
  String get hostSwitchManage => 'Manage hosts…';

  @override
  String get agentChoiceNative => 'Native app-server';

  @override
  String get agentChoiceSaved => 'Saved · ACP';

  @override
  String get agentChoicePreset => 'Preset · ACP';

  @override
  String get agentChoiceNew => 'New configuration';

  @override
  String get agentChoiceBuiltinTitle => 'Built-in presets';

  @override
  String get agentChoiceBuiltinNote =>
      'Default launch settings. The agent runs from its installation on this device.';

  @override
  String get agentChoiceSavedTitle => 'Saved configurations';

  @override
  String get agentChoiceSavedNote =>
      'Reuse launch settings you saved, or create a new configuration.';

  @override
  String get agentChoiceCustomTitle => 'Custom configuration';

  @override
  String get agentChoiceCustomNote =>
      'Set the program and arguments yourself, and save them for next time.';

  @override
  String get hostNameHelper =>
      'Unique on this device; it names the host on your relay.';

  @override
  String get hostAdvanced => 'Advanced';

  @override
  String get localHostPortInvalid =>
      'Enter a port from 0 to 65535 (0 picks a free port).';

  @override
  String get localHostPortHelper => '0 picks a free port automatically.';

  @override
  String get hostPortConflict =>
      'Another host on this device already uses this port. Use 0 for a free port, or pick another.';

  @override
  String hostSwitchingTo(String host) {
    return 'Switching to $host…';
  }

  @override
  String get hostSwitchProbing => 'Checking that the host is answering…';

  @override
  String get hostSwitchConnecting => 'Opening the connection…';

  @override
  String get hostSwitchLoading => 'Loading its conversations…';

  @override
  String hostSwitchFailedTitle(String host) {
    return 'Couldn\'t switch to $host';
  }

  @override
  String hostSwitchStayed(String host) {
    return 'You\'re still on $host.';
  }

  @override
  String hostSwitched(String host) {
    return 'Now on $host';
  }

  @override
  String get hostSwitchLastFailed => 'Last attempt failed';

  @override
  String hostInUse(String name) {
    return '$name · in use';
  }

  @override
  String hostCurrent(String host) {
    return 'Host: $host';
  }

  @override
  String hostSwitchHint(String host) {
    return 'Host: $host. Switch host';
  }

  @override
  String get pickFolderTitle => 'Choose a project folder';

  @override
  String get useThisFolder => 'Use this folder';

  @override
  String get folderUp => 'Up one level';

  @override
  String get folderPickerNoRoots =>
      'No project folders are configured on this host yet. Add them on the computer under hosting settings.';

  @override
  String get gitRepoLabel => 'Git repository';

  @override
  String get hostFiles => 'Host files';

  @override
  String get fileBrowserEmpty => 'This folder is empty';

  @override
  String get fileDownload => 'Download';

  @override
  String get fileUpload => 'Upload';

  @override
  String fileDownloaded(Object path) {
    return 'Downloaded to $path';
  }

  @override
  String fileDownloadFailed(Object error) {
    return 'Couldn\'t download: $error';
  }

  @override
  String fileUploaded(Object name) {
    return 'Uploaded $name';
  }

  @override
  String hostFileUploadFailed(Object error) {
    return 'Couldn\'t upload: $error';
  }

  @override
  String get browseProjectFolder => 'Browse project folders';

  @override
  String get orEnterPathManually => 'Or enter a path manually:';

  @override
  String get projectFolders => 'Project folders';

  @override
  String get addProjectFolder => 'Add folder';

  @override
  String get removeProjectFolder => 'Remove';

  @override
  String get welcomeTitle => 'Welcome to PocketCodex';

  @override
  String get welcomeSubtitleDesktop =>
      'One step left: start hosting on this computer, and your phone can drive its Codex from anywhere.';

  @override
  String get welcomeSubtitleMobile =>
      'PocketCodex remote-controls the Codex running on your computer. Set up hosting there once and you\'re ready.';

  @override
  String get welcomeStepHost => 'Start hosting on this computer';

  @override
  String get welcomeStepHostDesc =>
      'One click brings up a codex app-server and publishes it to your account — the defaults just work.';

  @override
  String get welcomeHostRunning => 'Hosting is up';

  @override
  String get welcomeStepFolders => 'Configure project folders (optional)';

  @override
  String get welcomeStepFoldersDesc =>
      'Your phone browses these folders visually to start a session in the right project.';

  @override
  String get welcomeFoldersLocked => 'Available once hosting is up.';

  @override
  String get welcomeMobileStep1 =>
      'Install and open PocketCodex on your computer';

  @override
  String get welcomeMobileStep2 => 'Sign in with the same GitHub account';

  @override
  String get welcomeMobileStep3 =>
      'Tap \"Start hosting\" — one click completes the setup';

  @override
  String get welcomeWaitingHost => 'Watching for your host to come online…';

  @override
  String welcomeHostFound(String label) {
    return 'Host found: $label';
  }

  @override
  String get welcomeDownloadDesktop => 'Get the desktop app';

  @override
  String get welcomeEnterChat => 'Enter the chat';

  @override
  String get welcomeSkip => 'Skip the guide';

  @override
  String get codexSetup => 'Configure Codex';

  @override
  String get codexChatNeedsSetup =>
      'Model access isn\'t set up yet — set this host\'s model service before chatting.';

  @override
  String get codexSetupStepTitle => 'Configure Codex model access';

  @override
  String get codexSetupStepDesc =>
      'Sign in with ChatGPT, or give a host its own provider when you host it, and optionally switch on the non-degraded system prompt.';

  @override
  String get codexSetupProviderSection => 'Custom provider';

  @override
  String get codexSetupLoginSection => 'ChatGPT sign-in';

  @override
  String get codexSetupLoginDesc =>
      'Run the model on your ChatGPT subscription; Codex creates the credential itself.';

  @override
  String get codexSetupLoginButton => 'Sign in with ChatGPT';

  @override
  String get codexSetupLoginWaiting => 'Waiting for the browser login…';

  @override
  String get codexSetupMethodsTitle => 'Model access';

  @override
  String get codexSetupMethodsHint =>
      'If Codex\'s own configuration selects a custom provider, that provider is used instead of the ChatGPT sign-in.';

  @override
  String get codexSetupInUse => 'In use';

  @override
  String get codexSetupProxyNote =>
      'On a restricted network, sign-in needs a proxy: set one in a host\'s editor and sign in there.';

  @override
  String get codexSetupAdvanced => 'Advanced';

  @override
  String get codexSetupNonDegradedTitle => 'Non-degraded system prompt';

  @override
  String get codexSetupNonDegradedDesc =>
      'Removes the commentary / progress-report instructions that interrupt the reasoning chain, so the model spends its budget on reasoning.';

  @override
  String get codexSetupNonDegradedScope => 'Applies to new conversations';

  @override
  String get codexSetupNonDegradedIssue => 'openai/codex#30364';

  @override
  String get codexSetupLoginOpened =>
      'Opened the login page in your browser; completion is detected automatically…';

  @override
  String get codexSetupDeviceOpened =>
      'Opened the verification page in your browser. Enter the code below there; completion is detected automatically…';

  @override
  String get codexSetupDeviceCodeLabel => 'Verification code';

  @override
  String get codexSetupDeviceReopen => 'Reopen verification page';

  @override
  String get codexSetupLoginSuccess => 'Signed in.';

  @override
  String get codexSetupLoginTimeout => 'Login timed out, please retry.';

  @override
  String get codexSetupStatusProvider => 'Custom provider configured';

  @override
  String get codexSetupStatusNeedSetup => 'Not configured yet';

  @override
  String get codexSetupStatusNeedSetupHint =>
      'Codex cannot make a model call yet. Set up either method below.';

  @override
  String get codexSetupStatusReadyHint => 'Codex can make model calls.';

  @override
  String get codexSetupCodexHomeLabel => 'Config directory';

  @override
  String get codexSignedInChatgpt => 'Signed in with ChatGPT';

  @override
  String get codexSignedInApiKey => 'Using an API key';

  @override
  String get codexSignedIn => 'Signed in';

  @override
  String get codexSetupStatusInvalid =>
      'Codex\'s config.toml could not be read';

  @override
  String get codexSetupStatusInvalidHint =>
      'Fix the file before hosts on \"Existing configuration\" can use it.';

  @override
  String get codexSetupProviderPerHost =>
      'Each host can use its own custom provider, chosen in that host\'s editor.';

  @override
  String get codexSetupProviderScope =>
      'This page sets up Codex itself (the config directory above): the ChatGPT sign-in and the prompt option. Hosts set to \"Existing configuration\" use them; a host\'s own custom provider or official account is chosen in that host\'s editor.';

  @override
  String get codexSetupGlobalProvider =>
      'Codex\'s own configuration already selects a custom provider; hosts set to \"Existing configuration\" keep using it.';

  @override
  String get codexSetupConfigureHost => 'Configure a host';

  @override
  String get codexSetupHostingDesktopOnly =>
      'Hosting runs on a desktop; set a host\'s provider there.';

  @override
  String get codexSetupLoginDesktopOnly =>
      'Sign in on the computer where Codex is installed.';

  @override
  String get settingsHosting => 'Hosting and model service';

  @override
  String get settingsHostingDesc =>
      'A host\'s agent, model service and sign-in';

  @override
  String get hostPickTitle => 'Configure which host?';

  @override
  String get hostPickHint =>
      'Each host keeps its own model service. Choosing one only opens its editor. Save keeps a running host as it is; its saved configuration takes effect only after Apply and restart.';

  @override
  String get hostPickNew => 'New host';

  @override
  String get hostPickLoadFailed =>
      'The running hosts could not be read. You can still set up a new host.';

  @override
  String get resizeComposer => 'Drag to resize; double-click to reset';

  @override
  String get historyGap => 'Load messages in this gap';

  @override
  String get reviewRequest => 'Automatic review request';

  @override
  String get reviewResult => 'Model review result';

  @override
  String get reviewAllowed => 'Allowed';

  @override
  String get reviewDenied => 'Denied';

  @override
  String get reviewLow => 'Low';

  @override
  String get reviewMedium => 'Medium';

  @override
  String get reviewHigh => 'High';

  @override
  String get reviewCritical => 'Critical';

  @override
  String get reviewUnknown => 'Unknown';

  @override
  String reviewRisk(String level) {
    return 'Risk: $level';
  }

  @override
  String reviewAuthorization(String level) {
    return 'Authorization: $level';
  }

  @override
  String get reviewRaw => 'View original content';

  @override
  String get conversationOutline => 'Conversation outline';

  @override
  String get searchTurns => 'Search messages or enter a turn number';

  @override
  String turnPosition(int current, int total) {
    return 'Turn $current of $total';
  }

  @override
  String stepRange(int first, int last, int total) {
    return 'Steps $first–$last of $total';
  }

  @override
  String get previousSteps => 'Previous steps';

  @override
  String get nextSteps => 'Next steps';

  @override
  String get latestSteps => 'Latest steps';

  @override
  String get jumpToStep => 'Jump to step';

  @override
  String get stepNumber => 'Step number';

  @override
  String get noMatchingTurns => 'No matching turns';

  @override
  String loadingTurn(int number) {
    return 'Opening turn $number…';
  }

  @override
  String get historySyncing => 'Syncing history…';

  @override
  String get historyCachedOffline => 'Cached history · waiting to sync';

  @override
  String get historyCacheTitle => 'Conversation cache';

  @override
  String get historyCacheDescription =>
      'Shared disk limit in MB. 0 disables caching.';

  @override
  String historyCacheUsage(String used, int limit) {
    return '$used MB used · $limit MB limit';
  }

  @override
  String get historyCacheInvalid => 'Enter a whole number from 0 to 64000.';

  @override
  String get modeAsk => 'Ask for approval';

  @override
  String get modeAskDesc =>
      'Work in the workspace; ask you before actions that require approval.';

  @override
  String get fastMode => 'Fast';

  @override
  String get fastModeHint =>
      'Use the model’s Fast service tier for subsequent turns.';

  @override
  String get steerMessage => 'Supplement';

  @override
  String get steerMessageHint =>
      'Add to the current turn at its next step. Otherwise, queue for the next turn.';

  @override
  String get queueNextTurn => 'Queue for next turn';

  @override
  String get steerTurnEnded =>
      'The turn changed or ended. Your draft is kept; send it as a new turn.';

  @override
  String get steerUnavailableHint =>
      'This connection does not offer running-turn supplements. Select for details.';

  @override
  String get steerQueueOnly => 'Queue only';

  @override
  String get steerUnavailableTitle => 'Supplement unavailable';

  @override
  String steerUnavailableDetails(String agent) {
    return '$agent has not advertised support for adding messages to a running turn over this ACP connection. Messages you send now are queued for the next turn.\n\nSupplement is available when the connected agent advertises this capability; choosing a different model does not enable it.';
  }

  @override
  String get steerQueuedAfterEnd =>
      'The turn had already ended, so your supplement will be sent as the next turn.';

  @override
  String get retryUnverified =>
      'Could not check whether the host received this message, so it was not sent again. Retry when the host is reachable.';

  @override
  String get queuedUncertain =>
      'May already have been sent. It is sent again only if you retry it.';

  @override
  String get queuedUncertainRetry =>
      'It may already have reached the host, so it stays queued and is not sent by itself. Check the conversation: Retry sends it again, tapping it lets you edit it.';

  @override
  String get steerAnswerStale =>
      'The answer arrived after this conversation\'s host or view changed, so your draft is kept as it was. Check the conversation before sending it again.';

  @override
  String get sendHostChanged =>
      'The host connection changed before this message was sent, so nothing was sent.';

  @override
  String get sendAnswerStale =>
      'The host answered after its connection changed, so it is unknown whether this message arrived.';

  @override
  String get sendReturnedToDraft =>
      'It is back in the composer; check the conversation before sending it again.';

  @override
  String get advancedSettings => 'Advanced settings';

  @override
  String get planModeDescription =>
      'Plan first without implementing. Normal mode is recommended for most tasks.';

  @override
  String get imageGenerating => 'Generating image…';

  @override
  String get imageLoading => 'Loading image…';

  @override
  String get imageGenerationFailed => 'Image generation failed';

  @override
  String get imageGenerationIncomplete => 'Image generation incomplete';

  @override
  String get imageGenerationUnavailable => 'No image was returned';

  @override
  String get imageGenerationLimit => 'Image generation usage limit reached';

  @override
  String get fileLinkPreview => 'Preview';

  @override
  String get fileLinkDownload => 'Download';

  @override
  String get fileLinkUnsupported =>
      'This file cannot be previewed. Download it to open with an app on your device.';

  @override
  String get fileLinkTruncated =>
      'Showing a limited preview. Download for the complete file.';

  @override
  String get fileLinkLoading => 'Loading file…';

  @override
  String get fileLinkRemoteWebTitle => 'Website on the host';

  @override
  String get fileLinkRemoteWeb =>
      'This address points to the controlled device, not this device. Use pb-mapper to register the web service port on the host and subscribe to it on this device, then open the mapped local address. Keep the URL path and query unchanged.';

  @override
  String get fileLinkCheckingHost => 'Checking host location…';

  @override
  String get fileLinkFailed =>
      'Unable to read the file. Check that it still exists and the host is running a version that supports file links.';

  @override
  String get fileLinkClose => 'Close';

  @override
  String get providerLabel => 'Provider';

  @override
  String get providerCodex => 'Codex';

  @override
  String get hostMetaLabel => 'Meta service';

  @override
  String get draft => 'Draft';

  @override
  String get editMessage => 'Edit message';

  @override
  String get done => 'Done';

  @override
  String get expandComposer => 'Expand editor';

  @override
  String get resetComposerHeight => 'Reset height';

  @override
  String get selectText => 'Select text';

  @override
  String get sharePrompt => 'Share prompt';

  @override
  String get shareMessage => 'Share message';

  @override
  String get messageShareFailed =>
      'Could not open sharing. You can copy the text instead.';

  @override
  String get messageCopyFailed => 'Could not copy the text. Please try again.';

  @override
  String get addToDraft => 'Add to draft';

  @override
  String get editMessageHint =>
      'Reuse this text as a new message. History stays unchanged; original attachments are not added.';

  @override
  String get editMessageDraftHint =>
      'Your existing draft and attachments will be kept. This text will be added below.';

  @override
  String get voiceStart => 'Start voice';

  @override
  String get voiceStartHint =>
      'Talk about the current project. Voice can ask Codex to perform tasks using the same permissions as text conversations.';

  @override
  String get voiceUnavailable => 'Voice unavailable';

  @override
  String get voiceVoice => 'Voice';

  @override
  String get voiceModel => 'Voice model';

  @override
  String get voiceModelHint => 'Leave empty to use the host default';

  @override
  String get voiceConnecting => 'Connecting voice…';

  @override
  String get voiceListening => 'Connected · Listening';

  @override
  String get voiceMuted => 'Microphone muted';

  @override
  String get voiceStopping => 'Ending voice…';

  @override
  String get voiceStop => 'End voice';

  @override
  String get voiceMute => 'Mute';

  @override
  String get voiceUnmute => 'Unmute';

  @override
  String get voiceHistory => 'Voice transcript';

  @override
  String get voiceHistoryEmpty => 'No voice transcript on this page';

  @override
  String get voiceHistoryMore => 'Load more';

  @override
  String get voiceErrorDetails => 'Error details';

  @override
  String get voiceNewSession => 'New voice conversation';

  @override
  String get restoringSessionSettings => 'Restoring session settings…';

  @override
  String get reviewInProgress => 'Model review in progress';

  @override
  String get reviewTimedOut => 'Review timed out';

  @override
  String get reviewAborted => 'Review aborted';

  @override
  String get guardianSession => 'Model approval';

  @override
  String get childSession => 'Child session';

  @override
  String childSessions(int count) {
    return 'Child sessions ($count)';
  }

  @override
  String get guardianReadOnly => 'Model approval session · Read only';

  @override
  String get parentSession => 'Open parent session';

  @override
  String get hideSidebar => 'Hide sidebar';

  @override
  String get showSidebar => 'Show sidebar';

  @override
  String get locateConversation => 'Show current conversation';

  @override
  String get voiceReconnecting => 'Voice reconnecting…';

  @override
  String get voiceSpeaking => 'Assistant speaking';

  @override
  String get voiceUserSpeaking => 'You\'re speaking';

  @override
  String get voiceEndedByUser => 'Voice call ended';

  @override
  String voiceEndedReason(String reason) {
    return 'Voice call ended: $reason';
  }

  @override
  String get voiceFailedShort => 'Voice call dropped';

  @override
  String get voiceHangUp => 'Hang up';

  @override
  String get voiceModelDefault => 'Host default';

  @override
  String get voiceModelFrameless => 'gpt-live-1-codex (low latency)';

  @override
  String get voiceModelCustom => 'Custom…';

  @override
  String get voiceModelCustomLabel => 'Model id';

  @override
  String get voiceTaskContinues =>
      'Tasks the voice handed to Codex keep running after you hang up.';

  @override
  String get stepOutput => 'output';

  @override
  String get previewRendered => 'Preview';

  @override
  String get previewSource => 'Source';

  @override
  String get dictate => 'Dictate';

  @override
  String get dictationReady => 'ready';

  @override
  String get dictationConnecting => 'Connecting dictation…';

  @override
  String get dictationListening => 'Listening';

  @override
  String get dictationFinishing => 'Finishing…';

  @override
  String get dictationStop => 'Done (Enter)';

  @override
  String get dictationCancel => 'Discard (Esc)';

  @override
  String get dictationKeys => 'Enter done · Esc discard';

  @override
  String get dictationPermission => 'Microphone access was denied';

  @override
  String get dictationUnavailable => 'Dictation is unavailable on this host';

  @override
  String get dictationInterrupted =>
      'Dictation dropped; what was heard is kept';

  @override
  String get dictationVoiceBusy =>
      'The live voice call is using the microphone';

  @override
  String get providerAcpAgent => 'ACP agent';

  @override
  String get protocolAcpTag => 'ACP';

  @override
  String get acpHostHint =>
      'Runs an ACP-compatible agent on this device from a program and its arguments, and makes it available to your devices. Stopping hosting stops the agent.';

  @override
  String get acpAgentCustom => 'Custom agent';

  @override
  String get acpAgentName => 'Agent name';

  @override
  String get acpProgram => 'Program';

  @override
  String get acpProgramBrowse => 'Choose…';

  @override
  String get acpProgramHint =>
      'A full path, or a program name found on this device\'s PATH. Paths may contain spaces.';

  @override
  String get acpArguments => 'Arguments';

  @override
  String get acpArgumentsHint =>
      'One argument per row, passed exactly as typed: no shell, no splitting, no variables.';

  @override
  String get acpArgumentAdd => 'Add argument';

  @override
  String get acpArgumentRemove => 'Remove argument';

  @override
  String get acpArgumentMoveUp => 'Move up';

  @override
  String get acpArgumentMoveDown => 'Move down';

  @override
  String acpArgumentLabel(int index) {
    return 'Argument $index';
  }

  @override
  String get acpSecretsNote =>
      'Saved in plain text on this computer. Keep secrets out of the arguments.';

  @override
  String get acpProgramNotFound => 'This program was not found on this device.';

  @override
  String get claudeEnvTitle => 'Claude environment';

  @override
  String get claudeRuntimeTitle => 'Runtime components';

  @override
  String claudeAdapterInstalled(String path) {
    return 'ACP adapter: $path';
  }

  @override
  String claudeAdapterPrepared(String path) {
    return 'ACP adapter (prepared by this app): $path';
  }

  @override
  String claudeAdapterWillPrepare(String package) {
    return 'The ACP adapter is not installed. Prepare it now, or let Start hosting prepare it first. This downloads $package from npm into this app\'s cache; the global environment stays unchanged.';
  }

  @override
  String claudeAdapterMissing(String package) {
    return 'The ACP adapter ($package) is not installed, and it cannot be prepared here.';
  }

  @override
  String claudeNodeFound(String version, String path) {
    return 'Node.js $version: $path';
  }

  @override
  String claudeNodeTooOld(String version) {
    return 'Node.js $version is too old; the adapter needs Node.js 22 or newer.';
  }

  @override
  String get claudeNodeMissing => 'Node.js 22 or newer was not found.';

  @override
  String claudeCliFound(String path) {
    return 'Claude Code: $path';
  }

  @override
  String get claudeCliInherited =>
      'Claude Code: as set in CLAUDE_CODE_EXECUTABLE';

  @override
  String get claudeCliBundled =>
      'Claude Code was not found; the adapter uses its bundled copy.';

  @override
  String get claudePrepareEnvironment => 'Prepare ACP environment';

  @override
  String get claudeEnvironmentReady =>
      'ACP environment is ready. No host was started or restarted.';

  @override
  String get claudeEnvRecheck => 'Check again';

  @override
  String get claudePrepareCancel => 'Stop preparing';

  @override
  String get claudePrepareStopWaiting => 'Stop waiting';

  @override
  String get claudePrepareJoined =>
      'Another window started this preparation; this page only follows it.';

  @override
  String get claudePrepareLeftRunning =>
      'Stopped waiting. The preparation started elsewhere keeps running.';

  @override
  String get claudePrepareReplaced =>
      'Another preparation replaced the one this page followed, so its result is not shown here. Prepare ACP environment or Start hosting to retry.';

  @override
  String get quitStopping => 'Stopping hosted agents before quitting…';

  @override
  String quitBlocked(String error) {
    return 'The app stays open because hosting could not be stopped safely: $error';
  }

  @override
  String get quitRetry => 'Quit again';

  @override
  String claudePrepareFailed(String error) {
    return 'Preparing the Claude ACP adapter failed: $error';
  }

  @override
  String get claudePrepareCancelled =>
      'Preparing the Claude ACP adapter was stopped. Choose Prepare ACP environment or Start hosting to retry.';

  @override
  String acpProgramFound(String path) {
    return 'Found: $path';
  }

  @override
  String get acpSaveAgent => 'Save as a configuration';

  @override
  String get acpHostingUnsupported =>
      'Hosting ACP agents is not available on this platform yet. You can still use agents hosted on another device.';

  @override
  String get acpPhaseStarting => 'Starting';

  @override
  String get acpPhaseReady => 'Running';

  @override
  String get acpPhaseFailed => 'Not running';

  @override
  String get acpPhaseStopped => 'Stopped';

  @override
  String get acpAuthRequired =>
      'The agent says you need to sign in. Sign in with the agent itself on the host, then restart it.';

  @override
  String get acpRestart => 'Restart agent';

  @override
  String get acpGatewayLabel => 'Agent gateway';

  @override
  String get acpStopNote =>
      'Stopping hosting stops this agent and the processes it started.';

  @override
  String get acpFeatureReopen => 'Reopen earlier sessions';

  @override
  String get acpFeatureReopenLoad => 'With history';

  @override
  String get acpFeatureReopenResume => 'Without earlier history';

  @override
  String get acpFeatureList => 'Agent\'s own session list';

  @override
  String get acpFeatureImages => 'Image attachments';

  @override
  String get acpFeatureSupported => 'Supported';

  @override
  String get acpFeatureNotSupported => 'Not supported';

  @override
  String get acpFeatureUnknown => 'Shown after connecting to the running agent';

  @override
  String permissionFromAgent(String agent) {
    return '$agent asks for permission';
  }

  @override
  String get permissionRememberTitle => 'Remember this choice?';

  @override
  String permissionRememberBody(String option) {
    return '\"$option\" asks the agent to remember this decision. How long and how widely it applies is decided by the agent.';
  }

  @override
  String get permissionAnswerFailed => 'Could not send your answer. Try again.';

  @override
  String get agentOptions => 'Agent options';

  @override
  String get agentMode => 'Mode';

  @override
  String agentOptionSearch(String option) {
    return 'Search $option';
  }

  @override
  String get agentOptionNoMatch => 'No matching values.';

  @override
  String get historyGapNotice =>
      'Earlier history is not available from this agent host.';

  @override
  String contentOmittedNotice(String parts) {
    return 'Not kept because it was too large: $parts.';
  }

  @override
  String get contentUnavailableNotice =>
      'Part of this reply could not be read from the agent host.';

  @override
  String get omittedImages => 'images';

  @override
  String get omittedDiffs => 'file changes';

  @override
  String get omittedInput => 'tool input';

  @override
  String get omittedLocations => 'file locations';

  @override
  String get omittedPlan => 'plan steps';

  @override
  String get omittedContent => 'some output';

  @override
  String get omittedSeparator => ', ';

  @override
  String hostEditorTitle(String name) {
    return 'Host $name';
  }

  @override
  String get hostSectionAgent => 'Agent';

  @override
  String get hostSectionStatus => 'Status';

  @override
  String get hostSectionModel => 'Model service';

  @override
  String get hostSectionModelHint =>
      'Where this host\'s agent sends model requests. Saved per host on this device.';

  @override
  String get hostSourceInherited => 'Existing configuration';

  @override
  String get hostSourceOfficial => 'Official account';

  @override
  String get hostSourceCustom => 'Custom provider';

  @override
  String get hostInheritedAcp =>
      'The agent uses its own configuration files and environment, unchanged.';

  @override
  String hostInheritedCodexCustom(String provider) {
    return 'Codex\'s config.toml already selects provider $provider. Nothing more is needed.';
  }

  @override
  String get hostInheritedCodexAccount =>
      'Codex is already signed in. Nothing more is needed.';

  @override
  String get hostInheritedCodexInvalid =>
      'Codex\'s config.toml could not be read. Fix it, or choose an option below.';

  @override
  String get hostInheritedCodexNone =>
      'Codex has no sign-in or provider yet. Sign in or add a custom provider.';

  @override
  String hostInheritedCodexHome(String path) {
    return 'Configuration directory: $path';
  }

  @override
  String get hostProtocolLabel => 'API type';

  @override
  String get hostProtocolResponses => 'OpenAI Responses';

  @override
  String get hostProtocolAnthropic => 'Anthropic Messages';

  @override
  String get hostProtocolChat => 'OpenAI Chat Completions';

  @override
  String get hostBaseUrl => 'Base URL';

  @override
  String get hostApiKey => 'API key';

  @override
  String get hostApiKeyStored => 'A key is stored. Leave empty to keep it.';

  @override
  String get hostKeyEnvNote =>
      'Stored only on this device and handed to the agent when it starts, never on its command line.';

  @override
  String get hostApiKeyRemove => 'Remove stored key';

  @override
  String get hostModelOptional => 'Model (optional)';

  @override
  String get hostBaseUrlRequired => 'Enter the provider\'s base URL.';

  @override
  String get hostSaveConfig => 'Save';

  @override
  String get hostConfigSaved =>
      'Saved. The running host keeps its current configuration until it restarts.';

  @override
  String get hostConfigPending =>
      'Saved changes are not active yet. Restart the host to apply them.';

  @override
  String get hostApplyRestart => 'Apply and restart';

  @override
  String get hostSaveAndRestart => 'Save and restart';

  @override
  String get hostApplyNote =>
      'Restarting stops the agent briefly and disconnects open sessions. If the new configuration fails to start, the previous one is restored.';

  @override
  String get hostApplied => 'Restarted with the new configuration.';

  @override
  String hostApplyRestored(String error) {
    return 'The new configuration failed, so the previous one was restored: $error';
  }

  @override
  String hostApplyFailed(String error) {
    return 'Restart failed and the host is stopped: $error';
  }

  @override
  String get hostOfficialNote =>
      'Sign-in runs on a private local Codex server, so no host needs to be running.';

  @override
  String get hostAgentEditNote =>
      'Changing the launch command takes effect on restart.';

  @override
  String get hostCodexRuntimeFixed =>
      'What the host runs now. Edit the runtime below, save, then apply to restart with it.';

  @override
  String hostLoginRefreshFailed(String error) {
    return 'Could not check the sign-in; retrying: $error';
  }

  @override
  String get hostLoginNotOfficial =>
      'Codex finished the login, but not with a ChatGPT account. Sign in again.';

  @override
  String hostLoginFailed(String error) {
    return 'Sign-in failed: $error';
  }

  @override
  String get hostSourceUnsupported =>
      'The saved model service cannot be used with this agent. Choose another one.';

  @override
  String get hostModelRequired =>
      'Enter the default model this provider should use.';

  @override
  String get hostKeyRequired => 'Enter the provider\'s API key.';

  @override
  String get hostModelDefault => 'Default model';

  @override
  String get hostModelDefaultNote =>
      'The provider\'s model for new sessions. A conversation\'s own model choice is made in the conversation and is not stored here.';

  @override
  String get hostLeaveTitle => 'Discard unsaved changes?';

  @override
  String get hostLeaveBody => 'Your changes to this host have not been saved.';

  @override
  String get hostLeaveStay => 'Keep editing';

  @override
  String get hostLeaveDiscard => 'Discard';

  @override
  String hostApplyConfirmTitle(String name) {
    return 'Restart $name?';
  }

  @override
  String get hostApplyConfirmBody =>
      'The host stops and starts again with its saved configuration. A running turn is interrupted and open conversations reconnect. Drafts, attachments and queued messages on this device are kept. Other hosts are not affected.';

  @override
  String get hostReplaced =>
      'This host was restarted elsewhere since this page read it. Reload before applying.';

  @override
  String hostApplyStopped(String error, String restoreError) {
    return 'The new configuration failed ($error) and restoring the previous one failed too ($restoreError). The host is stopped.';
  }

  @override
  String get hostGone => 'This host is no longer running.';

  @override
  String get hostConfigReload => 'Reload';

  @override
  String get hostConfigNewer =>
      'A newer saved configuration exists. Reload to see it (your edits are discarded).';

  @override
  String get hostConfigLoadFailedTitle => 'Configuration unavailable';

  @override
  String hostConfigLoadFailed(String error) {
    return 'Could not read this host\'s saved configuration: $error';
  }

  @override
  String get hostRuntimeEditNote =>
      'Saved changes take effect when you apply them; the name stays.';

  @override
  String get hostAgentUnknown =>
      'This host does not report its agent, so it cannot be edited here.';

  @override
  String get hostApiProxyNote =>
      'This host also publishes a separate Responses API service (api:<name>). It always uses this device\'s official Codex sign-in, not the provider chosen here.';

  @override
  String get claudePrepareStageWorking => 'Preparing the Claude ACP adapter…';

  @override
  String get claudePrepareStageInstalling =>
      'Resolving and installing packages…';

  @override
  String get claudePrepareStageDownloading => 'Downloading packages…';

  @override
  String get claudePrepareStageVerifying => 'Checking the downloaded adapter…';

  @override
  String get claudePrepareStageDone => 'The Claude ACP adapter is ready';

  @override
  String get claudePrepareStageFailed => 'Preparation failed';

  @override
  String get claudePrepareStageCancelled => 'Preparation stopped';

  @override
  String claudePrepareElapsed(String time) {
    return '$time elapsed';
  }

  @override
  String claudePrepareDownloaded(String size) {
    return '$size downloaded';
  }

  @override
  String claudePrepareSpeed(String speed) {
    return 'Download speed $speed';
  }

  @override
  String get claudePrepareSpeedMeasuring => 'Measuring speed…';

  @override
  String get claudePrepareStepFetch => 'Download and install';

  @override
  String get claudePrepareStepVerify => 'Verify';

  @override
  String claudePrepareStep(int step, int total, String name) {
    return 'Step $step of $total: $name';
  }

  @override
  String get claudeEnvHint =>
      'Defaults inherit the environment available to this host (including the login shell on macOS). Turn on a field to override it for this host only. Save keeps the running host unchanged; restart to apply. Model support depends on your Claude version and provider.';

  @override
  String get claudeEnvProviderWins =>
      'The custom provider fields above control the URL, credential and default model. Environment connection overrides are used only with inherited configuration.';

  @override
  String get claudeEnvInherited => 'Inherit environment';

  @override
  String get claudeEnvCustom => 'Override for this host';

  @override
  String get claudeEnvStored => 'Stored securely · leave blank to keep';

  @override
  String get claudeEnvValue => 'Enter a value';

  @override
  String get claudeEnvRequired =>
      'Fill each enabled override, or switch it back to inherit.';

  @override
  String get claudeEnvConnection => 'Connection & credentials';

  @override
  String get claudeEnvModels => 'Models & aliases';

  @override
  String get claudeEnvReasoning => 'Reasoning & output';

  @override
  String get claudeEnvNetwork => 'Network & compatibility';

  @override
  String get claudeEnvBaseUrl => 'API base URL';

  @override
  String get claudeEnvAuthToken => 'Bearer token';

  @override
  String get claudeEnvApiKey => 'Anthropic API key';

  @override
  String get claudeEnvOAuthToken => 'Existing OAuth token';

  @override
  String get claudeEnvModel => 'Default model';

  @override
  String get claudeEnvOpus => 'Opus alias model';

  @override
  String get claudeEnvSonnet => 'Sonnet alias model';

  @override
  String get claudeEnvHaiku => 'Haiku alias model';

  @override
  String get claudeEnvCustomModel => 'Additional model picker option';

  @override
  String get claudeEnvSubagentModel => 'Subagent model';

  @override
  String get claudeEnvEffort => 'Reasoning effort';

  @override
  String get claudeEnvThinkingTokens => 'Thinking token budget';

  @override
  String get claudeEnvOutputTokens => 'Maximum output tokens';

  @override
  String get claudeEnvDisableThinking => 'Omit the thinking parameter';

  @override
  String get claudeEnvDisableAdaptive =>
      'Disable adaptive thinking (supported models)';

  @override
  String get claudeEnvTimeout => 'API timeout (milliseconds)';

  @override
  String get claudeEnvHttpProxy => 'HTTP proxy';

  @override
  String get claudeEnvHttpsProxy => 'HTTPS proxy';

  @override
  String get claudeEnvNoProxy => 'Proxy bypass hosts';

  @override
  String get claudeEnvDisableCaching => 'Disable prompt caching';

  @override
  String get claudeEnvDisableTraffic => 'Disable nonessential traffic';

  @override
  String get claudeEnvNeedsValue => 'Needs a value, or switch back to inherit.';

  @override
  String get claudeEnvFlagOn => 'On';

  @override
  String get claudeEnvFlagOff => 'Off';

  @override
  String get claudeEnvCredential => 'Credential';

  @override
  String get claudeEnvCredentialNote =>
      'Claude uses one credential. Choosing one replaces the others for this host.';

  @override
  String get claudeEnvGroupInherited => 'All inherited';

  @override
  String get claudeEnvGroupMissing => 'A value is still needed';

  @override
  String get claudeEnvAllInherited => 'Inherits everything';

  @override
  String claudeEnvOverrides(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count overrides',
      one: '1 override',
    );
    return '$_temp0';
  }

  @override
  String claudeEnvMissing(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count values needed',
      one: '1 value needed',
    );
    return '$_temp0';
  }

  @override
  String claudeEnvDormant(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other:
          '$count saved connection overrides are kept and apply again with inherited configuration.',
      one:
          '1 saved connection override is kept and applies again with inherited configuration.',
    );
    return '$_temp0';
  }

  @override
  String get claudeEnvEditDormant => 'Edit inactive overrides';

  @override
  String get nativeHistoryReadOnly => 'Local history · Read-only';

  @override
  String agentWelcomeTitle(String agent) {
    return 'Start with $agent';
  }

  @override
  String agentWelcomeOpening(String agent) {
    return 'Opening a $agent session in this folder…';
  }

  @override
  String get agentWelcomeReady => 'Ready. Type your first message.';

  @override
  String agentWelcomeReadyModel(String model) {
    return 'Ready · $model';
  }

  @override
  String agentWelcomeFailed(String reason) {
    return 'Couldn\'t open a session: $reason';
  }

  @override
  String agentWelcomeNeedsProject(String agent) {
    return 'Choose a folder for $agent to work in.';
  }

  @override
  String get agentWelcomeLater => 'The session opens with your first message.';

  @override
  String get agentWelcomeConnecting => 'Waiting for the host connection…';

  @override
  String get chooseFolder => 'Choose a folder…';

  @override
  String get recentProjects => 'Recent projects';

  @override
  String get agentHostSettings => 'Host settings';

  @override
  String agentOpening(String agent) {
    return 'Opening $agent…';
  }

  @override
  String get agentOpeningHint =>
      'The agent\'s model and options appear once its session is open. Anything you type stays here.';

  @override
  String get agentOptionsUnavailable => 'Options unavailable';

  @override
  String get agentOptionsRetry =>
      'Couldn\'t load the agent\'s options. Select to try again.';

  @override
  String get agentErrorHint =>
      'Check this host\'s model service and environment, then retry.';

  @override
  String get agentAuthHint =>
      'The agent reports that it needs credentials. Provide them through this host\'s environment, then retry.';

  @override
  String get folderOnThisComputer => 'Choose on this computer…';

  @override
  String get folderTypePath => 'Enter a path';

  @override
  String get folderPathFieldLabel => 'Folder path on the host';

  @override
  String get folderOutsideRootsNote =>
      'This path is outside the project folders, so it can\'t be browsed here. The host checks it when the conversation starts.';

  @override
  String get folderPickerNoRootsHint =>
      'You can still enter any folder path on the host.';

  @override
  String get folderNoSubfolders =>
      'No subfolders here. You can use this folder as it is.';

  @override
  String get folderWillUse => 'Will use';

  @override
  String get folderPathAbsolute =>
      'Enter a full path, starting at the root of the host\'s file system.';

  @override
  String get folderBreadcrumbTop => 'Start';

  @override
  String get noProjectsYet => 'No projects yet. Choose a folder to start one.';

  @override
  String get nativeLocalHistoryHint =>
      'Browse saved sessions on this computer without connecting to an agent. Native history stays read-only.';

  @override
  String get nativeHistoryLoadMore => 'Load more sessions';

  @override
  String get nativeHistoryLoadOlder => 'Load older messages';

  @override
  String get nativeHistoryWindowHint =>
      'A bounded history window is shown. Some content is omitted or outside this window.';

  @override
  String get providerOpenCode => 'OpenCode';

  @override
  String get agentChoiceNativeServer => 'Native server';

  @override
  String get hostInheritedOpenCode =>
      'Uses OpenCode\'s own providers and sign-ins on this computer.';

  @override
  String get openCodeRuntimeHint =>
      'Pocket starts and owns one opencode serve on this computer, reachable only through this host.';

  @override
  String get openCodeDetecting => 'Looking for OpenCode…';

  @override
  String openCodeFoundAt(String version, String path) {
    return 'OpenCode $version · $path';
  }

  @override
  String get openCodeNotFound =>
      'OpenCode was not found. Install it or choose its executable.';

  @override
  String get openCodeUnsupportedVersion =>
      'This OpenCode version is not supported. Update OpenCode.';

  @override
  String get openCodeNoProvider =>
      'No provider is signed in. Run `opencode auth login` on this computer, then refresh.';

  @override
  String get openCodeAlreadyHosted =>
      'Another host already uses this OpenCode history. Choose “This host only” for this host, or stop that host first.';

  @override
  String hostNameInUse(String name) {
    return 'A host on this computer already uses the name “$name”. Choose another name.';
  }

  @override
  String managedNameInUse(String name) {
    return 'A service on that computer already uses the name “$name”. Choose another name.';
  }

  @override
  String get openCodeHistoryTitle => 'History';

  @override
  String get openCodeHistoryShared => 'Shared with OpenCode';

  @override
  String get openCodeHistoryOwn => 'This host only';

  @override
  String get openCodeHistoryInfo =>
      'Shared: this computer\'s own OpenCode sessions, the same ones the OpenCode terminal app shows. One host can use them at a time.\n\nThis host only: sessions are kept in this host\'s own database, apart from other hosts and the OpenCode terminal app.';

  @override
  String openCodeHistoryTaken(String name) {
    return '“$name” is using the shared history, and only one host can at a time. Choose “This host only”.';
  }

  @override
  String get openCodePath => 'OpenCode executable';

  @override
  String get openCodePathRequired => 'Enter the OpenCode executable.';

  @override
  String openCodeRunning(String version) {
    return 'OpenCode $version is running';
  }

  @override
  String get openCodeProviders => 'Providers ready';

  @override
  String get openCodeModels => 'Models';

  @override
  String get openCodeDefaultModel => 'Default model';

  @override
  String get openCodeAgents => 'Agents';

  @override
  String get openCodeNone => 'None';

  @override
  String get openCodeConfigNote =>
      'Providers, models and agents come from OpenCode\'s own configuration on this computer, plus the custom provider this host\'s model service may add. Change OpenCode\'s configuration there, then Refresh; restart this host if a change does not show.';

  @override
  String get openCodeScopeNote =>
      'Every Pocket device writes through this one server, so its turns never overlap. OpenCode windows and other OpenCode processes on this computer are not blocked.';

  @override
  String get refresh => 'Refresh';

  @override
  String get draftOptionDefault => 'Default';

  @override
  String openCodeRefreshFailed(String error) {
    return 'Couldn\'t refresh: $error';
  }

  @override
  String get sessionHandBack => 'Hand back to other clients';

  @override
  String accessQueueHeld(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other:
          '$count queued messages are waiting while this session was followed. Retry sends them.',
      one:
          '1 queued message is waiting while this session was followed. Retry sends it.',
    );
    return '$_temp0';
  }

  @override
  String accessScopeEngine(String agent) {
    return 'Writing through this host. Other $agent clients on this computer are not blocked.';
  }

  @override
  String get accessScopePocket =>
      'Held by this host. Other Pocket hosts cannot write; native clients are not blocked.';

  @override
  String get accessScopeNone =>
      'Writing through this host. Other clients are not blocked.';

  @override
  String get accessClientClaude => 'Claude Code';

  @override
  String get accessClientOpenCode => 'Another OpenCode process';

  @override
  String get accessClientPocketHost => 'Another Pocket host';

  @override
  String get accessClientOther => 'Another client';

  @override
  String get accessStatusOpening => 'Opening';

  @override
  String get accessStatusHandedBack => 'Handed back';

  @override
  String get accessStatusNotOpen => 'Not open here';

  @override
  String get accessStatusChanging => 'Changing hands';

  @override
  String get accessStatusUnavailable => 'Unavailable';

  @override
  String get accessStatusUnknown => 'Writer unknown';

  @override
  String get accessOpening => 'Opening the original session…';

  @override
  String get accessHandedBackTitle => 'Handed back to other clients';

  @override
  String get accessHandedBackDetail =>
      'This host closed its writer, so another app can continue the original session. Nothing is taken back by itself.';

  @override
  String get accessNotOpenedTitle =>
      'This host is not keeping this session open';

  @override
  String get accessNotOpenedDetail =>
      'The host holds no admission for it: it may never have been opened there, or the host restarted or let it go to make room. Nothing opens it by itself; Check now or Continue here opens it when nobody else writes it.';

  @override
  String get accessDeletedDetail =>
      'It was deleted on the host. Your unsent draft and queued messages stay here.';

  @override
  String get accessTransitionTitle => 'This session is changing hands';

  @override
  String get accessTransitionDetail => 'The host is loading or closing it.';

  @override
  String accessHostStoppedTitle(String agent) {
    return 'The $agent host is not running';
  }

  @override
  String get accessHostStoppedDetail => 'Start it to continue this session.';

  @override
  String accessExternalRunningTitle(String client) {
    return '$client is running a turn in this session';
  }

  @override
  String get accessExternalRunningDetail =>
      'Following it live. Nothing can be sent from here meanwhile.';

  @override
  String accessExternalIdleTitle(String client) {
    return '$client still has this session open';
  }

  @override
  String get accessExternalIdleDetail =>
      'No turn is running. Close the session there, then check again.';

  @override
  String get accessSourceOpenTitle =>
      'Another OpenCode process has this history source open';

  @override
  String get accessSourceOpenDetail =>
      'Pocket cannot tell which session or turn it writes. Close or release it there, then check now.';

  @override
  String accessPocketHostTitle(String client) {
    return '$client is writing this session';
  }

  @override
  String get accessPocketHostRunning => 'It is running a turn now.';

  @override
  String get accessPocketHostIdle =>
      'No turn is running there, but it still holds the session.';

  @override
  String accessUnsupportedTitle(String agent) {
    return '$agent cannot reopen this session';
  }

  @override
  String get accessNotFoundTitle => 'This session is no longer on the host';

  @override
  String get accessUnknownTitle => 'Cannot tell who else writes this session';

  @override
  String get accessUnknownDetail =>
      'The host could not inspect the session\'s other clients.';

  @override
  String get accessNextAutomatic =>
      'When that changes, this view continues by itself if the host still keeps it open here; otherwise Check now opens it.';

  @override
  String get accessNextWhenClosed =>
      'When nobody else holds it, this view continues by itself if the host still keeps it open here; otherwise Check now opens it.';

  @override
  String accessHolderPid(int pid) {
    return 'pid $pid';
  }

  @override
  String accessHolderSince(String time) {
    return 'since $time';
  }

  @override
  String accessOpenIn(String host) {
    return 'Open in $host';
  }

  @override
  String get accessStartHost => 'Open host';

  @override
  String get accessCheckNow => 'Check now';

  @override
  String get accessContinueHere => 'Continue here';

  @override
  String get sessionAccessChanged =>
      'The session changed while it was being opened. Try again.';

  @override
  String get permissionAllowOnce => 'Allow once';

  @override
  String get permissionAllowAlways => 'Always allow';

  @override
  String get permissionRejectOnce => 'Reject';

  @override
  String get permissionRejectAlways => 'Always reject';

  @override
  String permissionAlwaysAllows(String patterns) {
    return '\"Always allow\" covers: $patterns';
  }

  @override
  String get userInputChooseAny => 'Choose any that apply.';

  @override
  String commandPaletteLabel(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count commands',
      one: '1 command',
    );
    return '$_temp0';
  }

  @override
  String get commandSourceMcp => 'MCP';

  @override
  String get commandSourceSkill => 'Skill';

  @override
  String get nativeSessionFree => 'Not open elsewhere';

  @override
  String get nativeSessionUnknown => 'Holder unknown';

  @override
  String nativeContinueOn(String host) {
    return 'Continue in $host';
  }

  @override
  String nativeHostToContinue(String agent) {
    return 'Host $agent to continue';
  }

  @override
  String nativeContinueElsewhere(String agent) {
    return 'Continue on the computer that hosts $agent.';
  }

  @override
  String get defaultProjectTitle => 'Default project folder';

  @override
  String get defaultProjectAutomatic =>
      'Automatic, chosen by the host computer';

  @override
  String get defaultProjectCreatedLater =>
      'Not created yet. It is created when the first conversation starts here.';

  @override
  String get defaultProjectMissing =>
      'This folder no longer exists on the host. Choose another one.';

  @override
  String get defaultProjectNone =>
      'No default folder. New conversations ask for one.';

  @override
  String get defaultProjectChange => 'Change folder';

  @override
  String get defaultProjectUseAutomatic => 'Use automatic folder';

  @override
  String get defaultProjectSaved => 'Default project folder saved.';

  @override
  String get defaultProjectConflict =>
      'Another device changed the project settings. Reload to see them.';

  @override
  String defaultProjectUnavailable(String error) {
    return 'Project settings unavailable. Service management is not affected. $error';
  }

  @override
  String defaultProjectLabel(String name) {
    return '$name (default)';
  }

  @override
  String projectRootsAdvanced(int count) {
    return 'Browsable folders ($count)';
  }

  @override
  String get projectRootsHint =>
      'Other devices can browse only inside these folders. Leave empty to use the host\'s defaults.';

  @override
  String get projectRootPathHint =>
      'Absolute path on the host, e.g. /Users/me/code';

  @override
  String get managedComputerTitle => 'Computer management';

  @override
  String managedComputerSummary(String os, String version) {
    return '$os · Pocket $version';
  }

  @override
  String get managedNoServices =>
      'No services on this computer yet. Add one below.';

  @override
  String get managedAddService => 'Add service';

  @override
  String get managedAddHint =>
      'Installed on that computer. Nothing is installed or signed in here.';

  @override
  String managedAddTitle(String device) {
    return 'New service on $device';
  }

  @override
  String managedEditTitle(String name, String device) {
    return '$name on $device';
  }

  @override
  String get managedNoRuntimes =>
      'No supported runtime was found on that computer.';

  @override
  String get managedRuntimeUnavailable => 'Not available on that computer.';

  @override
  String get managedNeedsHostLogin =>
      'Needs sign-in on that computer to use its own account; a custom provider works without it.';

  @override
  String get managedKindUnknown => 'Unknown agent';

  @override
  String get managedStateRunning => 'Running';

  @override
  String get managedStateStopped => 'Stopped';

  @override
  String get managedStateNew => 'New';

  @override
  String get managedStateNeedsAttention => 'Needs attention';

  @override
  String get managedStart => 'Start';

  @override
  String get managedEdit => 'Edit';

  @override
  String managedStarted(String name) {
    return '$name is running.';
  }

  @override
  String managedStopped(String name) {
    return '$name stopped. It stays listed and can be started again.';
  }

  @override
  String get managedSaved => 'Saved.';

  @override
  String managedStopConfirmTitle(String name) {
    return 'Stop $name?';
  }

  @override
  String get managedStopConfirmBody =>
      'Its conversations disconnect and a running turn is interrupted. The computer stays manageable.';

  @override
  String managedRestoreProblem(String problem) {
    return 'Automatic restore needs attention: $problem';
  }

  @override
  String managedConfigUnreadable(String problem) {
    return 'The saved configuration cannot be read ($problem). It can still be stopped; fix it on that computer.';
  }

  @override
  String get managedConflict =>
      'Another device saved this service since you opened it. Your edits are kept; reload to see the new version (your edits are then discarded).';

  @override
  String get managedOutcomeUnknown =>
      'The result is unknown (the answer was lost). The current state was refreshed; check it before trying again.';

  @override
  String get managedHostBusy => 'That computer is busy. Try again shortly.';

  @override
  String get managedAlreadyRunning =>
      'This service is already running. Refresh, or choose another name.';

  @override
  String get managedHostStopped =>
      'This service has stopped. Refresh its status.';

  @override
  String get managedHostChanged =>
      'This service was restarted or replaced elsewhere. Its state was refreshed.';

  @override
  String get managedLoginRequired =>
      'The service\'s own account is not signed in on that computer. Sign in there, or choose a custom provider.';

  @override
  String get managedPrepareRequired =>
      'Prepare the Claude adapter on that computer first (Add service → Prepare ACP environment).';

  @override
  String get managedInvalidConfiguration =>
      'Review and save this service\'s configuration before starting.';

  @override
  String get managedConfigurationChanged =>
      'Another device saved this service. Reload before saving.';

  @override
  String get managedOutsideRoots =>
      'Choose a folder within the host\'s browsable folders.';

  @override
  String get managedNameRequired => 'Enter a name for the service.';

  @override
  String get managedNameHelper =>
      'Unique on that computer; it names the service on your relay.';

  @override
  String get managedPortLabel => 'Port';

  @override
  String get managedPortHelper => '0 picks a free port.';

  @override
  String get managedPortInvalid => 'Enter a port from 0 to 65535.';

  @override
  String get managedRuntimeNote =>
      'Runs the program installed on that computer. Paths and arguments are managed there.';

  @override
  String get managedInheritedNote =>
      'Uses the configuration already on that computer.';

  @override
  String get managedOfficialNote =>
      'Uses the official account signed in on that computer. Sign in there if it is not yet.';

  @override
  String get managedDiagnostics => 'Diagnostics';

  @override
  String get managedKey => 'Management key';

  @override
  String get managedDevice => 'Device';

  @override
  String get managedSwitchTitle => 'Allow management from my other devices';

  @override
  String get managedSwitchOn => 'On';

  @override
  String get managedSwitchOff => 'Off';

  @override
  String get managedSwitchInfo =>
      'When on, your other signed-in devices can add, configure, start and stop services here while this computer is online. When off, only this computer can change them.';

  @override
  String get managedSwitchConnecting => 'On, not reachable yet.';

  @override
  String get managedSwitchUnavailable => 'Management status could not be read.';

  @override
  String hostRestoreBlocked(String problem) {
    return 'Automatic restore was skipped because its saved choices cannot be read: $problem';
  }

  @override
  String get managedRuntimeReady => 'Ready';

  @override
  String get managedRuntimeNeedsSetup => 'Needs setup';

  @override
  String get managedRuntimeSignInThere => 'Ready · sign in on that computer';

  @override
  String get managedRuntimeDetails => 'Details';

  @override
  String get managedRuntimePath => 'Executable';

  @override
  String get managedRuntimeVersion => 'Version';

  @override
  String get refreshServices => 'Refresh services';

  @override
  String get refreshServicesFailed =>
      'Services could not be refreshed. Tap to try again.';

  @override
  String get managedRefreshComputer => 'Refresh this computer';

  @override
  String get composerWrite => 'Write';

  @override
  String get composerSplit => 'Split';

  @override
  String get composerPreview => 'Preview';

  @override
  String get composerPreviewEmpty =>
      'Nothing to preview yet. Write Markdown on the left and it renders here.';

  @override
  String get composerPreviewEmptyTab => 'Nothing to preview yet.';

  @override
  String get reviewSessionSource => 'This conversation';

  @override
  String get reviewWorkspaceSource => 'Workspace';

  @override
  String get reviewNoSessionChanges =>
      'This conversation hasn\'t changed any files yet.';

  @override
  String reviewEdit(int index) {
    return 'Edit $index';
  }

  @override
  String get reviewAllEdits => 'Show every edit';

  @override
  String reviewEditCount(int count) {
    return '$count edits';
  }

  @override
  String get reviewFilterFiles => 'Filter files';

  @override
  String get reviewOpenFile => 'Open the whole file';

  @override
  String get reviewShowChanges => 'Show the changes';

  @override
  String get reviewFileLoading => 'Reading the file…';

  @override
  String get reviewFileUnavailable => 'This file can\'t be read from the host.';

  @override
  String get copyPath => 'Copy path';

  @override
  String get openInReview => 'Open in review';

  @override
  String get showTurnNavigation => 'Show conversation controls';

  @override
  String get automationRun => 'Automation';

  @override
  String get reviewWorkspaceLoading => 'Reading the working tree\'s changes…';

  @override
  String get reviewWorkspaceUnavailable =>
      'The working tree\'s changes couldn\'t be read.';

  @override
  String relayVirtualAddress(String host, String address) {
    return 'The system DNS resolved $host to $address, a proxy\'s or VPN\'s virtual address that nothing routes right now. Reconnect that proxy or VPN, then retry.';
  }

  @override
  String relayNoAnswer(String relay) {
    return '$relay didn\'t answer.';
  }

  @override
  String get homeRelayLocalOnly =>
      'The relay can\'t be reached, so this computer\'s own service opened directly.';

  @override
  String get hostStopping => 'Stopping…';

  @override
  String get hostSaving => 'Saving…';

  @override
  String get hostRestarting => 'Restarting…';

  @override
  String hostStoppedToast(String name) {
    return '$name stopped.';
  }

  @override
  String get conversationsLoading => 'Loading conversations…';
}
