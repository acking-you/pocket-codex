// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get appTitle => 'PocketCodex';

  @override
  String get webUnsupported =>
      'PocketCodex 需要本地网络与文件访问,暂不支持 Web。\n请使用 Android / iOS / 桌面版。';

  @override
  String get onboardingTitle => '连接到 pb-mapper relay';

  @override
  String get importFieldLabel => 'pcx1: 分享串(一键导入)';

  @override
  String get importButton => '导入';

  @override
  String get relayFieldLabel => 'relay host:port';

  @override
  String get keyFieldLabel => 'MSG_HEADER_KEY (32 字节)';

  @override
  String get save => '保存';

  @override
  String get cancel => '取消';

  @override
  String get relayEmpty => 'relay 地址不能为空';

  @override
  String get keyLengthError => 'MSG_HEADER_KEY 必须是 32 字节';

  @override
  String get accountSignInTitle => '登录';

  @override
  String get accountSignInButton => '使用 GitHub 登录';

  @override
  String get accountUseDeviceCode => '改用设备码登录';

  @override
  String get accountWebFailed => '登录未完成,请重试。';

  @override
  String get accountWebTrouble => '浏览器登录未完成。如果 GitHub 页面打不开,请改用下方的设备码登录。';

  @override
  String get accountSignedIn => '已登录';

  @override
  String accountSignedInAs(String login) {
    return '已登录为 @$login';
  }

  @override
  String get accountEnterCode => '在 GitHub 上输入此代码以完成登录:';

  @override
  String get accountCopyCode => '复制代码';

  @override
  String get accountOpenGitHub => '打开 GitHub';

  @override
  String get accountWaiting => '等待你在 GitHub 上授权…';

  @override
  String get accountCodeExpired => '代码已过期,请重试。';

  @override
  String get accountDenied => 'GitHub 登录被拒绝。';

  @override
  String get accountAdvancedSelfHost => '改用自建 relay';

  @override
  String get accountAdvanced => '高级 / 自部署';

  @override
  String get accountBackendHint => '后端地址(留空用默认)';

  @override
  String get accountSection => '账户';

  @override
  String get accountSignOut => '退出登录';

  @override
  String get apiServicesSection => 'API 服务';

  @override
  String get appServerServices => 'App-server 服务';

  @override
  String get navApi => 'API';

  @override
  String get navAppServer => 'App-server';

  @override
  String get navSessions => '会话';

  @override
  String get navHosting => '托管';

  @override
  String get sessionsHostLabel => '主机';

  @override
  String get sessionsNoHost => '请先连接一个 app-server，其会话会显示在这里。';

  @override
  String appServerSubtitle(String device) {
    return '$device · 远程控制';
  }

  @override
  String get appServiceTitle => 'App-server';

  @override
  String get connecting => '连接中…';

  @override
  String get connectFailed => '无法连接到 app-server';

  @override
  String get conversationsSection => '会话';

  @override
  String get newConversation => '新建对话';

  @override
  String get noThreads => '暂无会话';

  @override
  String get untitledThread => '(未命名)';

  @override
  String get renameConversation => '重命名';

  @override
  String get renameConversationHint => '会话名称';

  @override
  String get renameHint => '单击可重命名';

  @override
  String retryingAttempt(int attempt, int max) {
    return '重试中… $attempt/$max';
  }

  @override
  String get renameFailed => '重命名失败';

  @override
  String showMoreCount(int count) {
    return '展开显示（$count）';
  }

  @override
  String get showLess => '收起';

  @override
  String get messageHint => '输入消息…';

  @override
  String get send => '发送';

  @override
  String get chatRoleYou => '你';

  @override
  String get chatRoleAgent => 'Codex';

  @override
  String get voiceLive => '实时语音';

  @override
  String get voiceSessionEnded => '实时语音 · 已结束';

  @override
  String get voiceYou => '你';

  @override
  String get voiceAgent => '助手';

  @override
  String get addAttachment => '添加';

  @override
  String get turnSettings => '本轮设置';

  @override
  String get prevTurn => '上一轮对话';

  @override
  String get nextTurn => '下一轮对话';

  @override
  String get jumpToLatest => '回到最新';

  @override
  String get attachImage => '附加图片';

  @override
  String get removeImage => '移除图片';

  @override
  String get imagePickFailed => '无法读取所选图片';

  @override
  String imageTooMany(int count) {
    return '每条消息最多附加 $count 张图片';
  }

  @override
  String get imageSave => '保存图片';

  @override
  String get previewImage => '点击预览';

  @override
  String get imageLoadFailed => '无法加载此图片';

  @override
  String imageSaved(String path) {
    return '已保存到 $path';
  }

  @override
  String imageSaveFailed(String error) {
    return '保存图片失败:$error';
  }

  @override
  String imageOnHost(String path) {
    return '主机上的图片文件:$path';
  }

  @override
  String get imageOnlyMessage => '[图片]';

  @override
  String get attachFile => '附加文件';

  @override
  String get dropToAttach => '松开以添加附件';

  @override
  String queuedCount(int count) {
    String _temp0 = intl.Intl.pluralLogic(
      count,
      locale: localeName,
      other: '$count 条消息排队中',
    );
    return '$_temp0';
  }

  @override
  String get queuedHint => '将在当前回复结束后发送。按 Esc 取回最后一条。';

  @override
  String get removeQueued => '移出队列';

  @override
  String get removeFile => '移除文件';

  @override
  String get filePickFailed => '无法读取所选文件';

  @override
  String fileTooMany(int count) {
    return '每条消息最多附加 $count 个文件';
  }

  @override
  String fileTooLarge(int mb) {
    return '文件超过 $mb MB 上限';
  }

  @override
  String get fileUploadFailed => '上传文件到主机失败';

  @override
  String get fileOnlyMessage => '[文件]';

  @override
  String get interrupt => '打断';

  @override
  String get thinking => '思考中…';

  @override
  String get emptyConversation => '发送消息开始对话';

  @override
  String get turnFailed =>
      '本轮未完成 —— 连接中断或远程 codex 异常。请重试,或检查主机上的 codex(可能需重新登录)。';

  @override
  String get sandboxHelperUnavailable =>
      '此自带会话无法启动命令沙箱,因此智能体在这里无法执行命令或读取文件。请切换到「完全放行」模式(不使用沙箱运行),或连接到外部/远程 codex 主机。';

  @override
  String get disconnect => '断开连接';

  @override
  String get connectionLost => '连接已断开';

  @override
  String get reconnect => '重新连接';

  @override
  String get projectsSection => '项目';

  @override
  String get newProject => '新建项目';

  @override
  String get effortFaster => '更快';

  @override
  String get effortSmarter => '更聪明';

  @override
  String get searchProjects => '搜索项目';

  @override
  String get noMatchingProjects => '没有匹配的项目';

  @override
  String get workOutsideProject => '不在项目中工作';

  @override
  String get switchProjectTip => '切换项目';

  @override
  String newSessionTitleIn(String project) {
    return '要在 $project 内开发什么?';
  }

  @override
  String get currentProject => '项目';

  @override
  String get remotePathLabel => '远端项目文件夹路径（主机上）';

  @override
  String get remotePathHint => '如 /home/ubuntu/myproject — 留空用主机默认目录';

  @override
  String get model => '模型';

  @override
  String get modelDefault => '默认模型';

  @override
  String get defaultFolder => '默认目录';

  @override
  String get permissionMode => '权限';

  @override
  String get modeReadOnly => '只读';

  @override
  String get modeReadOnlyDesc => '执行前询问；不写文件';

  @override
  String get modeAuto => '自动';

  @override
  String get modeAutoDesc => '工作区内可写；仅失败时询问';

  @override
  String get modeFull => '完全放行';

  @override
  String get modeFullDesc => '无沙箱、从不询问（谨慎使用）';

  @override
  String get approvalPrompt => '智能体请求执行命令';

  @override
  String get approvalFilePrompt => '智能体请求修改文件';

  @override
  String get approvalPermissionPrompt => '智能体请求额外权限';

  @override
  String get approve => '允许';

  @override
  String get approveForSession => '本会话内允许';

  @override
  String get deny => '拒绝';

  @override
  String get userInputTitle => '智能体需要你补充信息';

  @override
  String get userInputSubmit => '提交';

  @override
  String get userInputOther => '其他…';

  @override
  String get planMode => '计划';

  @override
  String get planReadyTitle => '计划已就绪';

  @override
  String get implementPlan => '实现此计划';

  @override
  String get keepPlanning => '继续规划';

  @override
  String get implementPlanPrompt => '请按上面的计划开始实现。';

  @override
  String get noModelForMode => '无法切换模式：没有可用的模型';

  @override
  String get effort => '思考强度';

  @override
  String get effortMinimal => '最低';

  @override
  String get effortMinimalDesc => '思考最少，最快';

  @override
  String get effortLow => '低';

  @override
  String get effortLowDesc => '少量思考';

  @override
  String get effortMedium => '中';

  @override
  String get effortMediumDesc => '均衡（通常默认）';

  @override
  String get effortHigh => '高';

  @override
  String get effortHighDesc => '较充分';

  @override
  String get effortXhigh => '极高';

  @override
  String get effortXhighDesc => '最充分，最慢';

  @override
  String get openLink => '打开链接';

  @override
  String get linkOpenFailed => '无法打开链接';

  @override
  String get contextLabel => '上下文';

  @override
  String get contextUsageTitle => '上下文与用量';

  @override
  String get quota5h => '5 小时额度';

  @override
  String get quotaWeekly => '每周额度';

  @override
  String get quotaUnavailable => '暂无额度信息。';

  @override
  String get quotaRemaining => '剩余用量';

  @override
  String resetsIn(String span) {
    return '$span 后重置';
  }

  @override
  String get moreActions => '更多';

  @override
  String get backToProjects => '返回项目';

  @override
  String get stateReady => '就绪';

  @override
  String get stateWorking => '运行中…';

  @override
  String get statePlanning => '计划中…';

  @override
  String get statePlanMode => '计划模式';

  @override
  String get stateDisconnected => '已断开';

  @override
  String get stateReconnecting => '重连中…';

  @override
  String get compacted => '对话已压缩';

  @override
  String get turnStopped => '已停止';

  @override
  String turnElapsed(String duration) {
    return '用时 $duration';
  }

  @override
  String completedAt(String time) {
    return '完成于 $time';
  }

  @override
  String get activeModelTooltip => '当前模型与设置 — 点按查看详情';

  @override
  String get runtimeSheetTitle => '运行时配置';

  @override
  String get runtimeEffortModelDefault => '模型默认';

  @override
  String get runtimeCollabDefault => '默认';

  @override
  String runtimeConfirmedAt(String time) {
    return '服务器已确认 · $time';
  }

  @override
  String runtimeFromSnapshot(String time) {
    return '来自服务器会话快照 · $time';
  }

  @override
  String get runtimeUnavailable => '该服务器不上报运行时配置；当前显示应用发送的设置。';

  @override
  String get runtimeConfirmed => '服务器已确认';

  @override
  String get runtimeUnconfirmed => '按应用发送值显示——服务器未反馈';

  @override
  String turnHandledBy(String model) {
    return '本轮由 $model 处理';
  }

  @override
  String get modelReroutedNote => '服务器在本轮中改用了此模型';

  @override
  String get refreshStatus => '刷新状态';

  @override
  String get statusOnline => '在线';

  @override
  String get statusConnected => '已连接';

  @override
  String get statusChecking => '检测中…';

  @override
  String get statusUnreachable => '不可达';

  @override
  String get unreachableReason => '中继上的注册仍在,但远端 app-server 没有响应——它可能未启动,或已经宕机。';

  @override
  String get unreachableAuthRejected =>
      '中继拒绝了这次连接:认证码缺失或已失效。隧道本身是通的,远端并没有宕机——请在「服务管理」里重新登录账号,或重新填写中继密钥。';

  @override
  String get unreachableSilent =>
      '隧道已建立,但远端 app-server 在超时前没有应答——它可能正在启动、已卡死,或已经退出。';

  @override
  String get apiUnreachableReason => '中继上的注册仍在,但远端 API 服务没有响应——它可能未启动,或已经宕机。';

  @override
  String get subscribedAlive => '已订阅';

  @override
  String get subscribedDead => '已断开';

  @override
  String runningSessions(int count) {
    return '$count 个运行中';
  }

  @override
  String get compact => '压缩对话';

  @override
  String get compactConfirm => '总结并压缩当前对话以释放上下文？此操作不可撤销。';

  @override
  String get viewDiff => '查看变更';

  @override
  String get loadingDiff => '正在加载变更…';

  @override
  String get cancelDiffLoad => '取消加载变更';

  @override
  String get changesTitle => '变更';

  @override
  String get noChanges => '与主分支相比没有变更。';

  @override
  String get envTitle => '环境';

  @override
  String get envLocal => '本地';

  @override
  String get envProject => '项目';

  @override
  String get envSource => '来源';

  @override
  String get envRefresh => '刷新';

  @override
  String diffTruncated(int count) {
    return '另有 $count 行未显示 —— 复制路径查看完整差异';
  }

  @override
  String diffUnmodified(int count) {
    return '$count 行未更改';
  }

  @override
  String diffExpandFailed(int count) {
    return '$count 行未更改（无法加载）';
  }

  @override
  String get reviewTitle => '审阅';

  @override
  String get reviewNoFiles => '没有变更的文件。';

  @override
  String get reviewPickFile => '选择一个文件查看其变更。';

  @override
  String envFilesChanged(int count) {
    return '$count 个文件有变更';
  }

  @override
  String get start => '开始';

  @override
  String get create => '创建';

  @override
  String get copy => '复制';

  @override
  String get copied => '已复制';

  @override
  String get toolSearched => '联网搜索';

  @override
  String get toolRan => '执行命令';

  @override
  String get toolEdited => '修改文件';

  @override
  String get toolCalled => '调用工具';

  @override
  String get toolThinking => '思考';

  @override
  String get toolPlan => '计划';

  @override
  String get toolActivity => '活动';

  @override
  String get selectApiService => '选择一个 API 服务';

  @override
  String get relayNotConfigured => '(未配置 relay)';

  @override
  String get noServicesFound => '该 relay 上没有发现服务';

  @override
  String get retry => '重试';

  @override
  String get discoverFailed => '无法连接到 relay';

  @override
  String get apiServiceTitle => 'API 服务';

  @override
  String get localPortLabel => '本地端口';

  @override
  String get startSubscription => '启动订阅';

  @override
  String get stop => '停止';

  @override
  String get portRangeError => '端口必须是 1 到 65535 之间的整数';

  @override
  String get noAuthWarning => '⚠ 本地端点无鉴权,仅监听 127.0.0.1。仅在 App 前台存活。';

  @override
  String get subscribeFailed => '无法启动订阅';

  @override
  String get settingsTitle => '设置';

  @override
  String get utilityChat => '对话';

  @override
  String get utilityPages => '页面';

  @override
  String get utilitySwitchPage => '切换页面';

  @override
  String get settingsGeneral => '通用';

  @override
  String get settingsGeneralDescription => '外观、语言与常用偏好。';

  @override
  String get settingsCodexDescription => 'Provider、认证方式与 Codex 行为。';

  @override
  String get settingsAccountConnection => '账户与连接';

  @override
  String get settingsAccountConnectionDescription => '集中管理账户、Relay 与连接密钥。';

  @override
  String get settingsServicesSubscriptions => '服务与订阅';

  @override
  String get settingsServicesSubscriptionsDescription => '查看当前设备上的活跃服务连接。';

  @override
  String get settingsAdvanced => '高级';

  @override
  String get settingsAdvancedDescription => '配置导出与诊断工具。';

  @override
  String get settingsAppearanceDescription => '明暗主题保持一致的信息层级。';

  @override
  String get settingsLanguageDescription => '界面显示语言';

  @override
  String get settingsUsingAccountBroker => '当前使用账户 Broker · 自建 Relay 备用未配置';

  @override
  String settingsUsingAccountBrokerWithRelay(String relay) {
    return '当前使用账户 Broker · 自建 Relay 备用：$relay';
  }

  @override
  String get settingsSelfHostedRelay => '自建 Relay 备用';

  @override
  String get settingsSelfHostedKey => '自建 Relay 密钥';

  @override
  String get settingsSelfHostedKeySet => '自建 Relay 备用密钥已设置';

  @override
  String get settingsSelfHostedKeyNotSet => '自建 Relay 备用密钥未设置';

  @override
  String get settingsRemoveCredentials => '移除本机保存的账户凭据。';

  @override
  String get settingsExportDescription => '复制可用于其它设备的 pcx1: 配置分享串。';

  @override
  String get settingsExportUnavailable => '需要先配置自建 Relay 地址和 32 字节密钥。';

  @override
  String settingsOperationFailed(String error) {
    return '操作失败：$error';
  }

  @override
  String get settingsDiagnostics => '诊断';

  @override
  String get settingsConfigure => '配置';

  @override
  String get settingsEdit => '编辑';

  @override
  String get trayShow => '显示主窗口';

  @override
  String get trayQuit => '退出';

  @override
  String get windowMinimize => '最小化';

  @override
  String get windowMaximize => '最大化';

  @override
  String get windowRestore => '还原';

  @override
  String get windowClose => '关闭';

  @override
  String get relayRow => 'relay';

  @override
  String get notConfigured => '(未配置)';

  @override
  String get keyRow => 'MSG_HEADER_KEY';

  @override
  String get keySet => '•••••••• (已设置)';

  @override
  String get keyNotSet => '(未设置)';

  @override
  String get activeSubscriptions => '活跃订阅';

  @override
  String get none => '(无)';

  @override
  String get exportShareString => '导出 pcx1: 分享串';

  @override
  String get copiedShareString => '已复制 pcx1: 分享串';

  @override
  String get language => '语言';

  @override
  String get languageSystem => '跟随系统';

  @override
  String get languageChinese => '简体中文';

  @override
  String get languageEnglish => 'English';

  @override
  String get appearance => '外观';

  @override
  String get appearanceSystem => '跟随系统';

  @override
  String get appearanceLight => '明亮';

  @override
  String get appearanceDark => '暗黑';

  @override
  String get newSessionTitle => '我们该构建什么?';

  @override
  String get newSessionSubtitle => '选一个起点,或直接在下方输入你的任务。';

  @override
  String get suggestExploreTitle => '了解项目';

  @override
  String get suggestExplorePrompt => '介绍一下这个项目的结构、主要模块和技术栈。';

  @override
  String get suggestTestsTitle => '运行并修复测试';

  @override
  String get suggestTestsPrompt => '运行测试套件,并修复所有失败的用例。';

  @override
  String get suggestDiffTitle => '审查改动';

  @override
  String get suggestDiffPrompt => '总结当前工作区相对主分支的改动。';

  @override
  String get suggestPlanTitle => '规划功能';

  @override
  String get suggestPlanPrompt => '在写代码之前,帮我规划一个新功能。';

  @override
  String get searchConversations => '搜索会话';

  @override
  String get searchLocalSessions => '搜索会话内容 / 目录 / 来源';

  @override
  String get noMatchingThreads => '没有匹配的会话';

  @override
  String get groupActive => '进行中';

  @override
  String get groupToday => '今天';

  @override
  String get groupYesterday => '昨天';

  @override
  String get groupEarlier => '更早';

  @override
  String get activityView => '动态';

  @override
  String get running => '运行中…';

  @override
  String get timeJustNow => '刚刚';

  @override
  String timeMinutesAgo(int n) {
    return '$n 分钟前';
  }

  @override
  String timeHoursAgo(int n) {
    return '$n 小时前';
  }

  @override
  String get timeYesterday => '昨天';

  @override
  String timeDaysAgo(int n) {
    return '$n 天前';
  }

  @override
  String get modelLabel => '模型';

  @override
  String get permissionLabel => '权限';

  @override
  String get localSessions => '本地会话';

  @override
  String get localSessionsTitle => '本地会话';

  @override
  String get localSessionsHint =>
      '此 CODEX_HOME 下的会话，包含桌面端或 CLI 创建的。可在此恢复已结束的会话。';

  @override
  String get noLocalSessions => '没有本地会话';

  @override
  String get sessionResumable => '可恢复';

  @override
  String get sessionUnfinished => '上一轮被中断';

  @override
  String get sessionRunningElsewhere => '其他进程运行中';

  @override
  String get sessionInUseElsewhere => '被其他进程占用';

  @override
  String get sessionReadOnly => '只读';

  @override
  String get readOnlyViewing => '只读 — 其他客户端正在使用此会话';

  @override
  String get sessionTranscriptEmpty => '暂无可显示的内容';

  @override
  String get resumeSession => '恢复';

  @override
  String get forceTakeover => '强制接管';

  @override
  String get takeoverTitle => '强制接管？';

  @override
  String takeoverBody(int n) {
    return '该会话正被另外 $n 个进程占用。PocketCodex 将尝试终止它们后在此恢复。这些进程中未保存的工作将会丢失。';
  }

  @override
  String get takeoverWillTerminate => '将终止';

  @override
  String get takeoverConfirm => '终止并恢复';

  @override
  String get takeoverResumed => '会话已恢复';

  @override
  String takeoverKilled(int n) {
    return '已终止 $n 个进程';
  }

  @override
  String get takeoverStillHeld => '仍被占用——已照常恢复';

  @override
  String takeoverResumeFailed(String error) {
    return '恢复失败：$error';
  }

  @override
  String get takeoverNoTarget => '请先连接一个 app-server 服务再恢复。';

  @override
  String holderRow(String name, int pid) {
    return '$name · PID $pid';
  }

  @override
  String get localHostingSection => '本地托管';

  @override
  String get localHostTitle => '本地 codex';

  @override
  String get localHostStopped => '已停止';

  @override
  String get localHostRunning => '托管中';

  @override
  String get localHostStarting => '启动中…';

  @override
  String get localHostUnresponsive => '无响应';

  @override
  String get startHosting => '开始托管';

  @override
  String get stopHosting => '停止托管';

  @override
  String get localHostPort => '端口';

  @override
  String get localHostName => '实例名';

  @override
  String get codexBinaryPath => 'codex 程序路径';

  @override
  String get codexNotFound => '未在 PATH 中找到 codex —— 请在下方填写完整路径,或安装后点击「重新检测」。';

  @override
  String get codexRedetect => '重新检测';

  @override
  String get codexSourceExternal => '外接 codex';

  @override
  String get codexSourceBuiltin => '自带';

  @override
  String get codexBuiltinNote => '进程内运行 app 自带的 codex，无需安装。';

  @override
  String get hostRuntimeInfo => '运行信息';

  @override
  String get hostRuntimeMode => 'codex';

  @override
  String get hostCodexVersion => '版本';

  @override
  String get hostCodexPath => '路径';

  @override
  String get hostProxyLabel => '代理';

  @override
  String get hostProxyInherit => '继承 app 的运行环境';

  @override
  String get localHostDialogTitle => '托管本地 app-server';

  @override
  String get localHostHint => '在本机运行 codex 并注册到你的账号，让你的其它设备可以驱动它。';

  @override
  String localHostListening(String addr) {
    return '正在监听 $addr';
  }

  @override
  String localHostStartError(String error) {
    return '启动托管失败：$error';
  }

  @override
  String codexFoundAt(String path) {
    return '已找到 codex：$path';
  }

  @override
  String get chooseCodexPath => '选择 codex 程序…';

  @override
  String get codexPathRequired => '请先选择 codex 程序再继续。';

  @override
  String get useProxy => '使用代理';

  @override
  String get proxyLabel => '代理';

  @override
  String get proxyRequired => '请填写代理，或关闭「使用代理」。';

  @override
  String get noProxyWarning => '未使用代理时，本机的 codex 可能无法连接 chatgpt.com。';

  @override
  String get addLocalHost => '再托管一个…';

  @override
  String get customizeCodexPath => '自定义路径';

  @override
  String appServerSubtitleLocal(String device) {
    return '$device · 本地托管';
  }

  @override
  String get deregister => '注销';

  @override
  String get reregister => '重新注册';

  @override
  String get deregisterTitle => '注销该服务？';

  @override
  String deregisterWarning(String name) {
    return '将「$name」从你账号的中继列表中移除。如果仍有主机在运行它，它会在几秒内重新注册——要彻底移除请停掉该主机。';
  }

  @override
  String deregisterLocalWarning(String name) {
    return '把「$name」从中继下架。codex 和 API 代理仍在运行——随时可以在「本地托管」卡片里重新注册。';
  }

  @override
  String get deregisterOrphanTitle => '移除该不可达服务？';

  @override
  String deregisterOrphanWarning(String name) {
    return '「$name」没有响应——它的注册残留在中继上、但没有可达的主机。此操作会把它从你的列表中移除（即使残留注册仍在也会保持隐藏），并请求后端将其丢弃。';
  }

  @override
  String get remove => '移除';

  @override
  String get deregisterFailed => '注销失败';

  @override
  String get hostNameConflict =>
      '该名称已有另一个在线的服务器实例。请先停止那个实例，或换一个名称托管。（刚停止的实例约 15 秒内释放名称。）';

  @override
  String get batchRemoveEnter => '清理不可达';

  @override
  String get batchRemoveHint => '点选要移除的不可达服务。';

  @override
  String get batchSelectAll => '全选';

  @override
  String get batchClear => '清空';

  @override
  String batchRemoveSelected(int count) {
    return '移除（$count）';
  }

  @override
  String get batchRemoveTitle => '移除这些不可达服务？';

  @override
  String batchRemoveWarning(int count) {
    return '从此设备的列表中移除 $count 个不可达服务。若其中某个恢复，会重新出现。';
  }

  @override
  String batchRemovedSnack(int count) {
    return '已移除 $count 个服务';
  }

  @override
  String get tunnelAppLabel => 'App-server';

  @override
  String get tunnelApiLabel => 'API';

  @override
  String get tunnelMetaLabel => '会话（meta）';

  @override
  String get tunnelOffline => '已下架';

  @override
  String get logsTitle => '运行日志';

  @override
  String get logsLive => '实时采集';

  @override
  String get logsLevel => '级别';

  @override
  String get logsLevelAll => '全部';

  @override
  String get logsKeyword => '关键词';

  @override
  String get logsKeywordHint => 'error、host、tunnel…';

  @override
  String get logsCopy => '复制可见日志';

  @override
  String get logsClear => '清空日志';

  @override
  String get logsScrollBottom => '滚动到底部';

  @override
  String get logsEmpty => '暂无日志';

  @override
  String logsVisible(int visible, int total) {
    return '已显示 $visible/$total';
  }

  @override
  String logsCopied(int count) {
    return '已复制 $count 行日志';
  }

  @override
  String get manageServices => '服务管理';

  @override
  String get servicesHostThisDevice => '托管本机';

  @override
  String get servicesDevices => '设备';

  @override
  String servicesDeviceCapabilityCount(int count) {
    return '$count 项能力';
  }

  @override
  String get servicesCurrentDevice => '当前设备';

  @override
  String get servicesLocalDevice => '本机';

  @override
  String get servicesConnectOtherDevice => '连接其他设备';

  @override
  String get servicesConnectOtherDeviceHint => '使用相同账户自动发现';

  @override
  String get servicesDeviceHelp => '服务按设备组织；App-server、API 等协议类型只在设备能力中出现。';

  @override
  String get servicesCapabilities => '可用能力';

  @override
  String servicesCapabilitiesSummary(int count) {
    return '该设备向你提供 $count 项能力';
  }

  @override
  String get servicesChatCapability => '对话服务';

  @override
  String get servicesChatCapabilityDescription => '打开 PocketCodex 对话与实时审批';

  @override
  String get servicesApiCapability => 'Responses API';

  @override
  String get servicesApiCapabilityDescription => 'OpenAI 兼容的 /v1/responses 端点';

  @override
  String get servicesSessionsCapability => '会话共享';

  @override
  String get servicesSessionsCapabilityDescription => '浏览该主机上的会话与附件';

  @override
  String get servicesProvidedWithHost => '随对话主机提供';

  @override
  String get servicesDefault => '默认';

  @override
  String get servicesSetDefault => '设为默认';

  @override
  String get servicesOpen => '打开';

  @override
  String get servicesManage => '管理';

  @override
  String get servicesBrowse => '浏览';

  @override
  String get servicesNoCapabilities => '此设备暂时没有可用能力';

  @override
  String get servicesLocalHostingDescription => '在本机运行服务，供其他设备连接。';

  @override
  String get servicesAccountMode => '账户模式';

  @override
  String get servicesSelfHostMode => '自建 Relay';

  @override
  String get hostSessions => '主机历史会话';

  @override
  String get switchService => '切换主机';

  @override
  String get homeConnecting => '正在连接 Codex 主机…';

  @override
  String get homeRestoringHost => '正在恢复托管…';

  @override
  String get homeNoServiceTitle => '没有可用的 Codex 主机';

  @override
  String get homeNoServiceDesktopHint => '在本机开启托管后即可直接开始对话，手机也能随时连回来。';

  @override
  String get homeNoServiceMobileHint =>
      '在电脑上打开 PocketCodex 并开启托管（app-server），这里就会自动进入对话。';

  @override
  String get homeNoServiceSelfHostHint =>
      '在装有 codex 的机器上运行 `pocket-codex serve` 把 app-server 发布到中转，或登录账号后直接在本应用内托管。';

  @override
  String get homeAutoRetryNote => '会自动持续检测新主机。';

  @override
  String get switchServiceFailed => '无法连接该主机，已保持当前主机不变。';

  @override
  String get pickFolderTitle => '选择项目文件夹';

  @override
  String get useThisFolder => '就用这个文件夹';

  @override
  String get folderUp => '上一级';

  @override
  String get folderPickerEmpty => '这里没有子文件夹';

  @override
  String get folderPickerNoRoots => '该主机还没有配置项目文件夹。请在电脑端的托管设置里添加。';

  @override
  String get gitRepoLabel => 'Git 仓库';

  @override
  String get hostFiles => '主机文件';

  @override
  String get fileBrowserEmpty => '此文件夹为空';

  @override
  String get fileDownload => '下载';

  @override
  String get fileUpload => '上传';

  @override
  String fileDownloaded(Object path) {
    return '已下载到 $path';
  }

  @override
  String fileDownloadFailed(Object error) {
    return '下载失败:$error';
  }

  @override
  String fileUploaded(Object name) {
    return '已上传 $name';
  }

  @override
  String hostFileUploadFailed(Object error) {
    return '上传失败:$error';
  }

  @override
  String get browseProjectFolder => '浏览项目文件夹';

  @override
  String get orEnterPathManually => '或手动输入路径：';

  @override
  String get projectFolders => '项目文件夹';

  @override
  String get projectFoldersHint => '手机可浏览这些文件夹来开启会话。点亮星标即设为新会话的默认目录。';

  @override
  String get noProjectFolders => '还没有项目文件夹。';

  @override
  String get addProjectFolder => '添加文件夹';

  @override
  String get defaultProjectFolder => '默认目录';

  @override
  String get setAsDefault => '设为默认';

  @override
  String get removeProjectFolder => '移除';

  @override
  String get welcomeTitle => '欢迎使用 PocketCodex';

  @override
  String get welcomeSubtitleDesktop => '只差一步：在本机开启托管，手机就能随时随地远程控制这台电脑上的 Codex。';

  @override
  String get welcomeSubtitleMobile =>
      'PocketCodex 远程控制运行在你电脑上的 Codex。在电脑上完成一次托管配置即可开始。';

  @override
  String get welcomeStepHost => '在本机开启托管';

  @override
  String get welcomeStepHostDesc => '一键启动 codex app-server 并发布到你的账号，默认配置即可用。';

  @override
  String get welcomeHostRunning => '托管已开启';

  @override
  String get welcomeStepFolders => '配置项目文件夹（可选）';

  @override
  String get welcomeStepFoldersDesc => '手机可视化浏览这些文件夹，在正确的项目里开启会话。';

  @override
  String get welcomeFoldersLocked => '开启托管后即可配置。';

  @override
  String get welcomeMobileStep1 => '在电脑上安装并打开 PocketCodex';

  @override
  String get welcomeMobileStep2 => '登录同一个 GitHub 账号';

  @override
  String get welcomeMobileStep3 => '点击「开始托管」，一键完成配置';

  @override
  String get welcomeWaitingHost => '正在等待主机上线…';

  @override
  String welcomeHostFound(String label) {
    return '已发现主机：$label';
  }

  @override
  String get welcomeDownloadDesktop => '获取桌面版';

  @override
  String get welcomeEnterChat => '进入聊天';

  @override
  String get welcomeSkip => '跳过引导';

  @override
  String get codexSetup => '配置 Codex';

  @override
  String get codexChatNeedsSetup => '模型访问尚未配置 —— 请先完成 Codex 认证再对话。';

  @override
  String get codexSetupSettingsSubtitle => 'Provider / 官方登录 / 非降智 prompt';

  @override
  String get codexSetupStepTitle => '配置 Codex 模型访问';

  @override
  String get codexSetupStepDesc =>
      '填写 provider(URL + API Key)或用 ChatGPT 官方登录,并可切换「非降智」system prompt。';

  @override
  String get codexSetupProviderSection => '方式一 · 自定义 Provider';

  @override
  String get codexSetupProviderDesc =>
      '填写服务商的 Base URL 和 API Key,自动写入 codex 配置,无需官方登录。';

  @override
  String get codexSetupModelLabel => '模型(可选,默认 gpt-5.5)';

  @override
  String get codexSetupSaveProvider => '保存并使用';

  @override
  String get codexSetupLoginSection => '方式二 · 官方 ChatGPT 登录';

  @override
  String get codexSetupLoginDesc =>
      '通过浏览器完成 OpenAI 官方登录,codex 自动生成凭证(无需手工配置)。若在受限网络,请在启动本机托管时设置代理。';

  @override
  String get codexSetupLoginButton => '使用 ChatGPT 登录';

  @override
  String get codexSetupLoginWaiting => '等待浏览器完成登录…';

  @override
  String get codexSetupNonDegradedTitle => '使用非降智 system prompt';

  @override
  String get codexSetupNonDegradedDesc =>
      '移除会打断思维链的 commentary / 进度汇报指令(openai/codex#30364),让模型把预算用在推理上。对之后新建的会话生效。';

  @override
  String get codexSetupNeedFields => '请填写 Base URL 和 API Key。';

  @override
  String get codexSetupProviderSaved => 'Provider 已保存,重新开始本机托管后即可使用。';

  @override
  String get codexSetupNeedHost => '请先在「服务」页启动本机托管,再进行官方登录。';

  @override
  String get codexSetupLoginOpened => '已在浏览器打开登录页,完成后自动检测…';

  @override
  String get codexSetupDeviceOpened => '已在浏览器打开验证页面。请在页面中输入下方验证码,完成后自动检测…';

  @override
  String get codexSetupDeviceCodeLabel => '验证码';

  @override
  String get codexSetupDeviceReopen => '重新打开验证页面';

  @override
  String codexSetupLoginSuccess(String method) {
    return '登录成功($method)。';
  }

  @override
  String get codexSetupLoginTimeout => '登录超时,请重试。';

  @override
  String get codexSetupStatusProvider => '已配置自定义 Provider';

  @override
  String codexSetupStatusAuth(String method) {
    return '已登录($method)';
  }

  @override
  String get codexSetupStatusNeedSetup => '尚未配置:请在下方选择一种方式。';

  @override
  String get codexSetupCredentialExists => '凭证已存在';
}
