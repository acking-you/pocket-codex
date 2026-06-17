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
    return '$device · remote control';
  }

  @override
  String get appServiceTitle => 'App-server';

  @override
  String get connecting => 'Connecting…';

  @override
  String get connectFailed => 'Couldn\'t connect to the app-server';

  @override
  String get conversationsSection => 'Conversations';

  @override
  String get newConversation => 'New conversation';

  @override
  String get noThreads => 'No conversations yet';

  @override
  String get untitledThread => '(untitled)';

  @override
  String get messageHint => 'Message…';

  @override
  String get send => 'Send';

  @override
  String get interrupt => 'Interrupt';

  @override
  String get thinking => 'Thinking…';

  @override
  String get emptyConversation => 'Send a message to start the conversation';

  @override
  String get turnFailed => 'The turn failed';

  @override
  String get disconnect => 'Disconnect';

  @override
  String get connectionLost => 'Connection lost';

  @override
  String get reconnect => 'Reconnect';

  @override
  String get projectsSection => 'Projects';

  @override
  String get newProject => 'New project';

  @override
  String get currentProject => 'Project';

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
  String get modeAutoDesc => 'Write in workspace; ask only on failure';

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
  String get openLink => 'Open link';

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
  String resetsIn(String span) {
    return 'resets in $span';
  }

  @override
  String get moreActions => 'More';

  @override
  String get backToProjects => 'Back to projects';

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
  String get compacted => 'Conversation compacted';

  @override
  String get turnStopped => 'Stopped';

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
  String get subscribedAlive => 'Subscribed';

  @override
  String get subscribedDead => 'Dropped';

  @override
  String runningSessions(int count) {
    return '$count running';
  }

  @override
  String get compact => 'Compact conversation';

  @override
  String get compactConfirm =>
      'Summarise and shrink this conversation to free up context? This can\'t be undone.';

  @override
  String get viewDiff => 'View changes';

  @override
  String get changesTitle => 'Changes';

  @override
  String get noChanges => 'No changes vs the main branch.';

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
  String get toolThinking => 'Thinking';

  @override
  String get toolPlan => 'Plan';

  @override
  String get toolActivity => 'Activity';

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

  @override
  String get newSessionTitle => 'What should the remote Codex work on?';

  @override
  String get newSessionSubtitle =>
      'Pick a starting point, or just type your task below.';

  @override
  String get suggestExploreTitle => 'Explore the project';

  @override
  String get suggestExplorePrompt =>
      'Give me an overview of this project — its structure, main modules, and tech stack.';

  @override
  String get suggestTestsTitle => 'Run & fix tests';

  @override
  String get suggestTestsPrompt =>
      'Run the test suite and fix any failing tests.';

  @override
  String get suggestDiffTitle => 'Review changes';

  @override
  String get suggestDiffPrompt =>
      'Summarize the current working-tree changes against the main branch.';

  @override
  String get suggestPlanTitle => 'Plan a feature';

  @override
  String get suggestPlanPrompt =>
      'Help me plan out a new feature before writing any code.';

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
  String get groupEarlier => 'Earlier';

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
  String get localSessionsHint =>
      'Sessions under this CODEX_HOME, including ones the desktop app or CLI created. Resume a finished one here.';

  @override
  String get noLocalSessions => 'No local sessions';

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
    return 'This session is held open by $n other process(es). Pocket-Codex will try to terminate them, then resume it here. Any unsaved work in those processes will be lost.';
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
}
