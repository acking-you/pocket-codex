// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get appTitle => 'Pocket-Codex';

  @override
  String get webUnsupported =>
      'Pocket-Codex 需要本地网络与文件访问,暂不支持 Web。\n请使用 Android / iOS / 桌面版。';

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
  String get messageHint => '输入消息…';

  @override
  String get send => '发送';

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
  String get changesTitle => '变更';

  @override
  String get noChanges => '与主分支相比没有变更。';

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
  String get trayShow => '显示主窗口';

  @override
  String get trayQuit => '退出';

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
  String get newSessionTitle => '想让远程 Codex 做点什么?';

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
  String get groupEarlier => '更早';

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
    return '该会话正被另外 $n 个进程占用。Pocket-Codex 将尝试终止它们后在此恢复。这些进程中未保存的工作将会丢失。';
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
  String get codexNotFound => '未在 PATH 中找到 codex —— 请在下方填写完整路径。';

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
  String get deregisterFailed => '注销失败';

  @override
  String get tunnelAppLabel => 'App-server';

  @override
  String get tunnelApiLabel => 'API';

  @override
  String get tunnelOffline => '已下架';
}
