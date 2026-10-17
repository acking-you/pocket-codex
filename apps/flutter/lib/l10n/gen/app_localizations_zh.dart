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
      'PocketCodex 需要本地网络与文件访问，暂不支持 Web。\n请使用 Android / iOS / 桌面版。';

  @override
  String get onboardingTitle => '连接到 pb-mapper relay';

  @override
  String get importFieldLabel => 'pcx1: 分享串（一键导入）';

  @override
  String get importButton => '导入';

  @override
  String get relayFieldLabel => 'relay host:port';

  @override
  String get keyFieldLabel => 'MSG_HEADER_KEY（32 字节）';

  @override
  String get save => '保存';

  @override
  String get cancel => '取消';

  @override
  String get relayEmpty => 'relay 地址不能为空';

  @override
  String get keyLengthError => 'MSG_HEADER_KEY 必须是 32 字节';

  @override
  String get accountWelcomeHeadline => '欢迎使用 PocketCodex';

  @override
  String get accountWelcomeSubtitle => '登录后，在任意设备上继续你的对话。';

  @override
  String get accountSignInTitle => '登录';

  @override
  String get accountSignInButton => '使用 GitHub 登录';

  @override
  String get accountUseDeviceCode => '设备码登录';

  @override
  String get accountSessionExpiredTitle => '登录已过期';

  @override
  String get accountSessionExpiredMessage => '当前登录状态已失效，无法继续自动续期。请重新登录以恢复服务连接。';

  @override
  String get accountSignInAgain => '重新登录';

  @override
  String get accountWebFailed => '登录未完成，请重试。';

  @override
  String get accountWebTrouble =>
      '浏览器登录未完成 —— 可能是 GitHub 页面打不开，也可能是浏览器无法把结果交回给应用。下方的设备码登录不需要这一步交接。';

  @override
  String get accountSignedIn => '已登录';

  @override
  String accountSignedInAs(String login) {
    return '已登录为 @$login';
  }

  @override
  String get accountEnterCode => '在 GitHub 上输入此代码以完成登录：';

  @override
  String get accountCopyCode => '复制代码';

  @override
  String get accountOpenGitHub => '打开 GitHub';

  @override
  String get accountWaiting => '等待你在 GitHub 上授权…';

  @override
  String get accountCodeExpired => '代码已过期，请重试。';

  @override
  String get accountDenied => 'GitHub 登录被拒绝。';

  @override
  String get accountAdvancedSelfHost => '改用自建 relay';

  @override
  String get accountAdvanced => '高级 / 自部署';

  @override
  String get accountBackendHint => '后端地址（留空用默认）';

  @override
  String get accountSection => '账户';

  @override
  String get accountSignOut => '退出登录';

  @override
  String get connecting => '连接中…';

  @override
  String get conversationsSection => '会话';

  @override
  String get newConversation => '新建对话';

  @override
  String newConversationInProject(String project) {
    return '在 $project 中新建会话';
  }

  @override
  String get copyProjectPath => '复制项目路径';

  @override
  String get noThreads => '暂无会话';

  @override
  String get untitledThread => '（未命名）';

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
  String get imagesNotAccepted => '该智能体不接受图片';

  @override
  String get attachmentRevoked => '未添加：读取文件期间会话已变化，请重新添加。';

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
    return '保存图片失败：$error';
  }

  @override
  String imageOnHost(String path) {
    return '主机上的图片文件：$path';
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
  String get fileOnlyMessage => '[文件]';

  @override
  String get emptyConversation => '发送消息开始对话';

  @override
  String get olderHistoryHint => '加载更早的消息';

  @override
  String get historyStart => '对话开始';

  @override
  String get historyLoading => '正在加载历史…';

  @override
  String get historyRetry => '加载失败，点击重试';

  @override
  String get turnFailed =>
      '本轮未完成 —— 连接中断或远程 codex 异常。请重试，或检查主机上的 codex（可能需重新登录）。';

  @override
  String get sandboxHelperUnavailable =>
      '此自带会话无法启动命令沙箱，因此智能体在这里无法执行命令或读取文件。请切换到「完全放行」模式（不使用沙箱运行），或连接到外部/远程 codex 主机。';

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
  String get modeAuto => '替我审批';

  @override
  String get modeAutoDesc => '由 Codex 自动评估审批请求，保留工作区沙箱限制。';

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
  String get modelWithdrawn => '此主机已不再提供所选模型。请重新选择模型后再发送。';

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
  String get backToSessions => '返回会话列表';

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
  String get compactingContext => '正在自动压缩上下文';

  @override
  String get compacted => '对话已压缩';

  @override
  String get turnStopped => '已停止';

  @override
  String get modelErrorTitle => '模型未能回复';

  @override
  String modelErrorModel(String model) {
    return '模型：$model';
  }

  @override
  String modelErrorStatus(String status) {
    return 'HTTP $status';
  }

  @override
  String get modelErrorRegion => '此模型在你所在的地区不可用。请换一个模型后重试。';

  @override
  String get modelErrorAuthentication => '服务商无法验证身份。请检查 API 密钥或登录状态后重试。';

  @override
  String get modelErrorPermission => '服务商拒绝访问此模型。请换一个模型或检查账户权限。';

  @override
  String get modelErrorUnavailable => '模型或其服务端点不可用。请换一个模型或检查服务商配置。';

  @override
  String get modelErrorRateLimited => '服务商请求过多。请稍等片刻后重试。';

  @override
  String get modelErrorQuota => '服务商的用量额度已用完。请检查账户或换一个模型。';

  @override
  String get modelErrorContext => '对话长度超出此模型的上限。请换用上下文更大的模型，或开始新对话。';

  @override
  String get modelErrorOutput => '模型在完成前达到了输出上限。可以让它继续。';

  @override
  String get modelErrorProvider => '服务商暂时不可用。请稍后重试或换一个模型。';

  @override
  String get modelErrorInvalid => '服务商拒绝了此请求。请检查模型和消息后重试。';

  @override
  String get modelErrorUnknown => '模型未能完成回复。请重试，或换一个模型。';

  @override
  String get modelErrorRetry => '重试';

  @override
  String get modelErrorChangeModel => '更换模型';

  @override
  String get modelErrorIncomplete => '这条失败的消息在此处不完整，无法按原样重新发送。请编辑后发送，或发送新消息。';

  @override
  String modelChangeFailed(String reason) {
    return '无法切换模型：$reason。仍保留之前的模型，草稿未改动。';
  }

  @override
  String get modelErrorRetryHint => '以当前选择的模型重新发送这条消息。';

  @override
  String get reasoningOutputNote => '模型将这段内容作为思考返回，没有单独的回答。';

  @override
  String get queuedPausedHint => '已暂停。重试或发送消息后继续。';

  @override
  String turnElapsed(String duration) {
    return '用时 $duration';
  }

  @override
  String turnProcessed(String duration) {
    return '已处理 $duration';
  }

  @override
  String turnProcessing(int count) {
    return '正在处理 $count 步';
  }

  @override
  String turnActivityCount(int count) {
    return '$count 步操作';
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
  String get unreachableReason => '中继上的注册仍在，但远端 app-server 没有响应——它可能未启动，或已经宕机。';

  @override
  String get unreachableAuthRejected =>
      '中继拒绝了这次连接：认证码缺失或已失效。隧道本身是通的，远端并没有宕机——请在「服务管理」里重新登录账号，或重新填写中继密钥。';

  @override
  String get unreachableSilent =>
      '隧道已建立，但远端 app-server 在超时前没有应答——它可能正在启动、已卡死，或已经退出。';

  @override
  String get apiUnreachableReason => '中继上的注册仍在，但远端 API 服务没有响应——它可能未启动，或已经宕机。';

  @override
  String get subscribedAlive => '已订阅';

  @override
  String get subscribedDead => '已断开';

  @override
  String get compact => '压缩对话';

  @override
  String get compactConfirm => '总结并压缩当前对话以释放上下文？此操作不可撤销。';

  @override
  String get viewDiff => '查看变更';

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
  String turnProgressStep(int current, int total) {
    return '第 $current / $total 步';
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
  String get toolCollaborated => '协作智能体';

  @override
  String get toolSubAgent => '子智能体动态';

  @override
  String get toolViewedImage => '查看图片';

  @override
  String get toolGeneratedImage => '生成图片';

  @override
  String get toolWaited => '等待';

  @override
  String get toolHook => '钩子提示';

  @override
  String get toolEnteredReview => '进入审查';

  @override
  String get toolExitedReview => '退出审查';

  @override
  String get toolThinking => '思考';

  @override
  String get toolPlan => '计划';

  @override
  String get toolActivity => '活动';

  @override
  String get relayNotConfigured => '（未配置 relay）';

  @override
  String get retry => '重试';

  @override
  String get discoverFailed => '无法连接到 relay';

  @override
  String get localPortLabel => '本地端口';

  @override
  String get startSubscription => '启动订阅';

  @override
  String get stop => '停止';

  @override
  String get portRangeError => '端口必须是 1 到 65535 之间的整数';

  @override
  String get noAuthWarning => '⚠ 本地端点无鉴权，仅监听 127.0.0.1。仅在 App 前台存活。';

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
  String get settingsAccountConnection => '账户与连接';

  @override
  String get settingsServicesSubscriptions => '服务与订阅';

  @override
  String get settingsAdvanced => '高级';

  @override
  String get settingsSelfHostedRelay => '自建 Relay 备用';

  @override
  String get settingsSelfHostedKey => '自建 Relay 密钥';

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
  String get notConfigured => '（未配置）';

  @override
  String get keyRow => 'MSG_HEADER_KEY';

  @override
  String get keySet => '••••••••（已设置）';

  @override
  String get keyNotSet => '（未设置）';

  @override
  String get activeSubscriptions => '活跃订阅';

  @override
  String get none => '（无）';

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
  String get noLocalSessions => '没有本地会话';

  @override
  String get localSessionsUnavailable => '此设备无法读取本地会话';

  @override
  String get localSessionsMobileHint =>
      '此页面用于读取本机保存的 Codex 会话。查看电脑上的会话，请到服务管理连接电脑；已缓存的对话仍可在聊天中查看。';

  @override
  String get noLocalSessionsHint =>
      '这台电脑还没有可查看的 Codex 会话。使用 Codex 创建会话后刷新，或连接其他设备。';

  @override
  String get noHostSessions => '这台设备暂无会话';

  @override
  String get noHostSessionsHint => '这台设备还没有可查看的会话。请在该设备上创建 Codex 会话后刷新。';

  @override
  String get sessionsReadFailed => '暂时无法读取会话';

  @override
  String get localSessionsReadHint => '请确认当前用户已有 Codex 会话，且有权限读取会话目录，然后重试。';

  @override
  String get remoteSessionsReadHint => '暂时无法读取该设备的会话，请检查设备连接后重试。';

  @override
  String get errorDetails => '错误详情';

  @override
  String get moveTurnNavigation => '拖动可移动对话导航按钮';

  @override
  String historyLoaded(int count) {
    return '已加载 $count 条较早的记录';
  }

  @override
  String get historyAlreadyVisible => '这部分历史已在当前对话中显示';

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
  String get localHostStopFailed => '未能停止：请再次停止';

  @override
  String get localHostPort => '端口';

  @override
  String get localHostName => '实例名';

  @override
  String get codexBinaryPath => 'codex 程序路径';

  @override
  String get codexNotFound => '未在 PATH 中找到 codex —— 请在下方填写完整路径，或安装后点击「重新检测」。';

  @override
  String get codexRedetect => '重新检测';

  @override
  String get codexSourceExternal => '外置 Codex';

  @override
  String get codexSourceBuiltin => '内置引擎';

  @override
  String get codexBuiltinNote => '暂未实现，请使用外置 Codex。';

  @override
  String get codexBuiltinUnavailable => '暂未实现';

  @override
  String get hostRuntimeInfo => '运行信息';

  @override
  String get hostCodexPath => '路径';

  @override
  String get hostProxyLabel => '代理';

  @override
  String get hostProxyInherit => '继承 app 的运行环境';

  @override
  String get localHostDialogTitle => '在本机托管';

  @override
  String get localHostHint => '在本机运行 codex 并发布到你的中转，让你的其它设备可以驱动它。';

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
  String get tunnelOffline => '已下架';

  @override
  String get tunnelNotConnected => '未连通';

  @override
  String get logsTitle => '运行日志';

  @override
  String get logsLive => '实时采集';

  @override
  String get logsLevelAll => '全部';

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
  String get servicesLocalDevice => '本机';

  @override
  String get servicesPlatformUnknown => '平台未知';

  @override
  String get servicesPublisherInstance => '服务进程实例';

  @override
  String get servicesHostInstance => '托管实例';

  @override
  String get servicesCapabilities => '可用能力';

  @override
  String get servicesChatCapability => '对话服务';

  @override
  String get servicesApiCapability => 'Responses API';

  @override
  String get servicesSessionsCapability => '会话共享';

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
  String get hostSessions => '主机历史会话';

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
  String get homeAutoRetryNote => '会自动持续检测新主机。';

  @override
  String get switchServiceFailed => '无法连接该主机，已保持当前主机不变。';

  @override
  String get hostSwitchTitle => '切换主机';

  @override
  String acpSavedAgentsFull(int count) {
    return '此配置未保存：已保存 $count 个配置。';
  }

  @override
  String get hostSwitchManage => '管理主机…';

  @override
  String get agentChoiceNative => '原生 app-server';

  @override
  String get agentChoiceSaved => '已保存 · ACP';

  @override
  String get agentChoicePreset => '预设 · ACP';

  @override
  String get agentChoiceNew => '新建配置';

  @override
  String get agentChoiceBuiltinTitle => '内置预设';

  @override
  String get agentChoiceBuiltinNote => '使用默认启动设置，智能体从本机已安装的程序运行。';

  @override
  String get agentChoiceSavedTitle => '已保存配置';

  @override
  String get agentChoiceSavedNote => '复用你保存的启动设置，或新建一个配置。';

  @override
  String get agentChoiceCustomTitle => '自定义配置';

  @override
  String get agentChoiceCustomNote => '自行设置程序和参数，可保存供下次复用。';

  @override
  String get hostNameHelper => '本机内唯一，用作该主机在中转上的名称。';

  @override
  String get hostAdvanced => '高级';

  @override
  String get localHostPortInvalid => '请输入 0 到 65535 的端口（0 表示自动选择空闲端口）。';

  @override
  String get localHostPortHelper => '填 0 会自动选择空闲端口。';

  @override
  String get hostPortConflict => '本机已有其他主机在使用这个端口。填 0 自动选择空闲端口，或换一个端口。';

  @override
  String hostSwitchingTo(String host) {
    return '正在切换到 $host…';
  }

  @override
  String get hostSwitchProbing => '正在确认主机是否在线…';

  @override
  String get hostSwitchConnecting => '正在建立连接…';

  @override
  String get hostSwitchLoading => '正在加载会话…';

  @override
  String hostSwitchFailedTitle(String host) {
    return '无法切换到 $host';
  }

  @override
  String hostSwitchStayed(String host) {
    return '仍在使用 $host。';
  }

  @override
  String hostSwitched(String host) {
    return '已切换到 $host';
  }

  @override
  String get hostSwitchLastFailed => '上次尝试失败';

  @override
  String hostInUse(String name) {
    return '$name · 使用中';
  }

  @override
  String hostCurrent(String host) {
    return '主机：$host';
  }

  @override
  String hostSwitchHint(String host) {
    return '主机：$host。切换主机';
  }

  @override
  String get pickFolderTitle => '选择项目文件夹';

  @override
  String get useThisFolder => '就用这个文件夹';

  @override
  String get folderUp => '上一级';

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
    return '下载失败：$error';
  }

  @override
  String fileUploaded(Object name) {
    return '已上传 $name';
  }

  @override
  String hostFileUploadFailed(Object error) {
    return '上传失败：$error';
  }

  @override
  String get browseProjectFolder => '浏览项目文件夹';

  @override
  String get orEnterPathManually => '或手动输入路径：';

  @override
  String get projectFolders => '项目文件夹';

  @override
  String get addProjectFolder => '添加文件夹';

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
  String get codexChatNeedsSetup => '模型访问尚未配置 —— 请先设置此托管的模型服务再对话。';

  @override
  String get codexSetupStepTitle => '配置 Codex 模型访问';

  @override
  String get codexSetupStepDesc =>
      '用 ChatGPT 官方登录，或在托管时为它设置自己的供应商，并可切换「非降智」system prompt。';

  @override
  String get codexSetupProviderSection => '自定义供应商';

  @override
  String get codexSetupLoginSection => 'ChatGPT 登录';

  @override
  String get codexSetupLoginDesc => '用你的 ChatGPT 订阅运行模型，凭证由 Codex 自己生成。';

  @override
  String get codexSetupLoginButton => '使用 ChatGPT 登录';

  @override
  String get codexSetupLoginWaiting => '等待浏览器完成登录…';

  @override
  String get codexSetupMethodsTitle => '模型接入方式';

  @override
  String get codexSetupMethodsHint =>
      '如果 Codex 自身的配置已选用自定义供应商，会优先使用该供应商，而不是 ChatGPT 登录。';

  @override
  String get codexSetupInUse => '使用中';

  @override
  String get codexSetupProxyNote => '网络受限时，登录需要代理：请在托管的编辑页中填写代理，并在那里登录。';

  @override
  String get codexSetupAdvanced => '高级';

  @override
  String get codexSetupNonDegradedTitle => '非降智 system prompt';

  @override
  String get codexSetupNonDegradedDesc =>
      '去掉打断思维链的 commentary / 进度汇报指令，把预算留给推理。';

  @override
  String get codexSetupNonDegradedScope => '对之后新建的会话生效';

  @override
  String get codexSetupNonDegradedIssue => 'openai/codex#30364';

  @override
  String get codexSetupLoginOpened => '已在浏览器打开登录页，完成后自动检测…';

  @override
  String get codexSetupDeviceOpened => '已在浏览器打开验证页面。请在页面中输入下方验证码，完成后自动检测…';

  @override
  String get codexSetupDeviceCodeLabel => '验证码';

  @override
  String get codexSetupDeviceReopen => '重新打开验证页面';

  @override
  String get codexSetupLoginSuccess => '登录成功。';

  @override
  String get codexSetupLoginTimeout => '登录超时，请重试。';

  @override
  String get codexSetupStatusProvider => '已配置自定义供应商';

  @override
  String get codexSetupStatusNeedSetup => '尚未配置';

  @override
  String get codexSetupStatusNeedSetupHint => 'Codex 还不能发起模型调用。请在下面任选一种方式配置。';

  @override
  String get codexSetupStatusReadyHint => 'Codex 可以发起模型调用了。';

  @override
  String get codexSetupCodexHomeLabel => '配置目录';

  @override
  String get codexSignedInChatgpt => '已通过 ChatGPT 登录';

  @override
  String get codexSignedInApiKey => '正在使用 API 密钥';

  @override
  String get codexSignedIn => '已登录';

  @override
  String get codexSetupStatusInvalid => '无法读取 Codex 的 config.toml';

  @override
  String get codexSetupStatusInvalidHint => '修复这个文件后，选择「现有配置」的托管才能使用它。';

  @override
  String get codexSetupProviderPerHost => '每个托管都可以在自己的编辑页中选用自定义供应商。';

  @override
  String get codexSetupProviderScope =>
      '本页配置 Codex 本身（上方的配置目录）：ChatGPT 登录和提示词选项。选择「现有配置」的托管会使用这些设置；托管自己的自定义供应商或官方账号，在该托管的编辑页中选择。';

  @override
  String get codexSetupGlobalProvider =>
      'Codex 自身的配置已选用一个自定义供应商，选择「现有配置」的托管会继续使用它。';

  @override
  String get codexSetupConfigureHost => '配置托管';

  @override
  String get codexSetupHostingDesktopOnly => '托管只在桌面端运行，请在桌面端为托管设置供应商。';

  @override
  String get codexSetupLoginDesktopOnly => '请在装有 Codex 的电脑上登录。';

  @override
  String get settingsHosting => '托管与模型服务';

  @override
  String get settingsHostingDesc => '托管的智能体、模型服务与登录';

  @override
  String get hostPickTitle => '配置哪个托管？';

  @override
  String get hostPickHint =>
      '每个托管有各自的模型服务。选择后只会打开它的编辑页。保存不会改动正在运行的托管，保存的配置在「应用并重启」后才生效。';

  @override
  String get hostPickNew => '新建托管';

  @override
  String get hostPickLoadFailed => '无法读取正在运行的托管，仍可新建托管。';

  @override
  String get resizeComposer => '拖动调整输入框高度；双击恢复默认';

  @override
  String get historyGap => '加载此处的历史消息';

  @override
  String get reviewRequest => '自动审核请求';

  @override
  String get reviewResult => '模型审核结果';

  @override
  String get reviewAllowed => '允许';

  @override
  String get reviewDenied => '拒绝';

  @override
  String get reviewLow => '低';

  @override
  String get reviewMedium => '中';

  @override
  String get reviewHigh => '高';

  @override
  String get reviewCritical => '严重';

  @override
  String get reviewUnknown => '未知';

  @override
  String reviewRisk(String level) {
    return '风险：$level';
  }

  @override
  String reviewAuthorization(String level) {
    return '授权程度：$level';
  }

  @override
  String get reviewRaw => '查看原始内容';

  @override
  String get conversationOutline => '会话目录';

  @override
  String get searchTurns => '搜索内容或输入轮次编号';

  @override
  String turnPosition(int current, int total) {
    return '第 $current / $total 轮';
  }

  @override
  String stepRange(int first, int last, int total) {
    return '步骤 $first–$last / $total';
  }

  @override
  String get previousSteps => '上一段步骤';

  @override
  String get nextSteps => '下一段步骤';

  @override
  String get latestSteps => '最新步骤';

  @override
  String get jumpToStep => '跳到步骤';

  @override
  String get stepNumber => '步骤编号';

  @override
  String get noMatchingTurns => '没有匹配的轮次';

  @override
  String loadingTurn(int number) {
    return '正在定位第 $number 轮…';
  }

  @override
  String get historySyncing => '正在同步历史…';

  @override
  String get historyCachedOffline => '本地缓存 · 等待同步';

  @override
  String get historyCacheTitle => '会话缓存';

  @override
  String get historyCacheDescription => '所有主机共用的磁盘上限，单位 MB；0 表示关闭缓存。';

  @override
  String historyCacheUsage(String used, int limit) {
    return '已用 $used MB · 上限 $limit MB';
  }

  @override
  String get historyCacheInvalid => '请输入 0 到 64000 之间的整数。';

  @override
  String get modeAsk => '向我询问';

  @override
  String get modeAskDesc => '在工作区内执行；需要审批的操作由你确认。';

  @override
  String get fastMode => 'Fast';

  @override
  String get fastModeHint => '后续轮次使用模型支持的 Fast 服务档位。';

  @override
  String get steerMessage => '补充说明';

  @override
  String get steerMessageHint => '在当前轮次的下一个步骤带上；不选则排队到下个轮次。';

  @override
  String get queueNextTurn => '排队到下个轮次';

  @override
  String get steerTurnEnded => '当前轮次已变更或结束。草稿已保留，可作为新轮次发送。';

  @override
  String get steerUnavailableHint => '当前连接未提供运行中补充说明，点击查看原因。';

  @override
  String get steerQueueOnly => '仅支持排队';

  @override
  String get steerUnavailableTitle => '补充说明不可用';

  @override
  String steerUnavailableDetails(String agent) {
    return '$agent 没有通过当前 ACP 连接声明运行中补充说明的能力。现在发送的消息会排队，在当前轮次结束后发送。\n\n连接的代理提供此能力后才能使用补充说明；切换模型不会开启它。';
  }

  @override
  String get steerQueuedAfterEnd => '轮次已结束，补充内容将作为下一轮发送。';

  @override
  String get retryUnverified => '无法确认主机是否已收到这条消息，因此没有重新发送。请在主机可连接后重试。';

  @override
  String get queuedUncertain => '可能已经发送。只有你点重试时才会再次发送。';

  @override
  String get queuedUncertainRetry =>
      '这条消息可能已经到达主机，因此它留在队列中，不会自动发送。请先查看对话：点重试会再次发送，点它可以编辑。';

  @override
  String get steerAnswerStale => '回复到达时，此对话的主机或视图已经变化，因此你的草稿保持原样。再次发送前请先查看对话。';

  @override
  String get sendHostChanged => '发送前主机连接已变化，因此没有发送这条消息。';

  @override
  String get sendAnswerStale => '主机在连接变化后才回复，无法确定这条消息是否已到达。';

  @override
  String get sendReturnedToDraft => '消息已放回输入框；再次发送前请先查看对话。';

  @override
  String get advancedSettings => '高级设置';

  @override
  String get planModeDescription => '先规划，不执行。大多数任务建议使用普通模式。';

  @override
  String get imageGenerating => '正在生成图片…';

  @override
  String get imageLoading => '正在加载图片…';

  @override
  String get imageGenerationFailed => '图片生成失败';

  @override
  String get imageGenerationIncomplete => '图片生成未完成';

  @override
  String get imageGenerationUnavailable => '未返回图片';

  @override
  String get imageGenerationLimit => '已达到生图用量上限';

  @override
  String get fileLinkPreview => '预览';

  @override
  String get fileLinkDownload => '下载';

  @override
  String get fileLinkUnsupported => '此文件暂不支持预览，请下载后使用设备上的应用打开。';

  @override
  String get fileLinkTruncated => '当前仅显示部分预览，下载可获取完整文件。';

  @override
  String get fileLinkLoading => '正在加载文件…';

  @override
  String get fileLinkRemoteWebTitle => '宿主上的网页';

  @override
  String get fileLinkRemoteWeb =>
      '此地址指向被控制的设备。请通过 pb-mapper 在宿主上注册网页服务端口，再在当前设备订阅映射端口，然后打开映射后的本地地址。保留原网址的路径和查询参数。';

  @override
  String get fileLinkCheckingHost => '正在确认宿主位置…';

  @override
  String get fileLinkFailed => '无法读取文件。请确认文件仍然存在，且宿主已更新到支持文件链接的版本。';

  @override
  String get fileLinkClose => '关闭';

  @override
  String get providerLabel => '服务提供方';

  @override
  String get providerCodex => 'Codex';

  @override
  String get hostMetaLabel => 'Meta 服务';

  @override
  String get draft => '草稿';

  @override
  String get editMessage => '编辑消息';

  @override
  String get done => '完成';

  @override
  String get expandComposer => '展开编辑器';

  @override
  String get resetComposerHeight => '重置输入框高度';

  @override
  String get selectText => '选择文本';

  @override
  String get sharePrompt => '分享提示';

  @override
  String get shareMessage => '分享消息';

  @override
  String get messageShareFailed => '无法打开分享，可先复制文字。';

  @override
  String get messageCopyFailed => '复制失败，请重试。';

  @override
  String get addToDraft => '加入输入框';

  @override
  String get editMessageHint => '将文字作为新消息使用，保留原有历史，不会重新附加原消息的附件。';

  @override
  String get editMessageDraftHint => '保留当前草稿和附件，编辑后的文字将加在草稿末尾。';

  @override
  String get voiceStart => '开始语音';

  @override
  String get voiceStartHint => '与当前项目实时语音交流。语音可以驱动 Codex 执行任务，权限设置与普通对话一致。';

  @override
  String get voiceUnavailable => '语音暂不可用';

  @override
  String get voiceVoice => '音色';

  @override
  String get voiceModel => '语音模型';

  @override
  String get voiceModelHint => '留空使用主机默认模型';

  @override
  String get voiceConnecting => '正在连接语音…';

  @override
  String get voiceListening => '语音已连接 · 正在聆听';

  @override
  String get voiceMuted => '麦克风已静音';

  @override
  String get voiceStopping => '正在结束语音…';

  @override
  String get voiceStop => '结束语音';

  @override
  String get voiceMute => '静音';

  @override
  String get voiceUnmute => '取消静音';

  @override
  String get voiceHistory => '语音记录';

  @override
  String get voiceHistoryEmpty => '这一页暂无语音记录';

  @override
  String get voiceHistoryMore => '加载后续记录';

  @override
  String get voiceErrorDetails => '查看错误详情';

  @override
  String get voiceNewSession => '新建语音会话';

  @override
  String get restoringSessionSettings => '正在恢复会话设置…';

  @override
  String get reviewInProgress => '模型审核中';

  @override
  String get reviewTimedOut => '审核超时';

  @override
  String get reviewAborted => '审核已中止';

  @override
  String get guardianSession => '模型审批';

  @override
  String get childSession => '子会话';

  @override
  String childSessions(int count) {
    return '子会话（$count）';
  }

  @override
  String get guardianReadOnly => '模型审批会话 · 只读';

  @override
  String get parentSession => '打开父会话';

  @override
  String get hideSidebar => '隐藏侧边栏';

  @override
  String get showSidebar => '显示侧边栏';

  @override
  String get locateConversation => '定位到当前会话';

  @override
  String get voiceReconnecting => '语音重连中…';

  @override
  String get voiceSpeaking => '助手正在回复';

  @override
  String get voiceUserSpeaking => '你正在说话';

  @override
  String get voiceEndedByUser => '语音通话已结束';

  @override
  String voiceEndedReason(String reason) {
    return '语音通话已结束：$reason';
  }

  @override
  String get voiceFailedShort => '语音连接已断开';

  @override
  String get voiceHangUp => '挂断';

  @override
  String get voiceModelDefault => '主机默认';

  @override
  String get voiceModelFrameless => 'gpt-live-1-codex（低延迟）';

  @override
  String get voiceModelCustom => '自定义…';

  @override
  String get voiceModelCustomLabel => '模型 ID';

  @override
  String get voiceTaskContinues => '挂断不会取消语音已交给 Codex 的任务。';

  @override
  String get stepOutput => '输出';

  @override
  String get previewRendered => '预览';

  @override
  String get previewSource => '源码';

  @override
  String get dictate => '语音输入';

  @override
  String get dictationReady => '已就绪';

  @override
  String get dictationConnecting => '正在连接语音输入…';

  @override
  String get dictationListening => '正在听写';

  @override
  String get dictationFinishing => '正在收尾…';

  @override
  String get dictationStop => '完成（Enter）';

  @override
  String get dictationCancel => '放弃（Esc）';

  @override
  String get dictationKeys => 'Enter 完成 · Esc 放弃';

  @override
  String get dictationPermission => '麦克风权限被拒绝';

  @override
  String get dictationUnavailable => '此主机暂不能语音输入';

  @override
  String get dictationInterrupted => '语音输入中断，已保留听到的内容';

  @override
  String get dictationVoiceBusy => '实时语音通话正在使用麦克风';

  @override
  String get providerAcpAgent => 'ACP 智能体';

  @override
  String get protocolAcpTag => 'ACP';

  @override
  String get acpHostHint => '按程序和参数在本机运行兼容 ACP 的智能体，并提供给你的设备使用。停止托管会停止该智能体。';

  @override
  String get acpAgentCustom => '自定义智能体';

  @override
  String get acpAgentName => '智能体名称';

  @override
  String get acpProgram => '程序';

  @override
  String get acpProgramBrowse => '选择…';

  @override
  String get acpProgramHint => '完整路径，或本机 PATH 中的程序名。路径可以包含空格。';

  @override
  String get acpArguments => '参数';

  @override
  String get acpArgumentsHint => '每行一个参数，按原样传递：不经过 shell，不拆分，不展开变量。';

  @override
  String get acpArgumentAdd => '添加参数';

  @override
  String get acpArgumentRemove => '删除参数';

  @override
  String get acpArgumentMoveUp => '上移';

  @override
  String get acpArgumentMoveDown => '下移';

  @override
  String acpArgumentLabel(int index) {
    return '参数 $index';
  }

  @override
  String get acpSecretsNote => '以明文保存在本机，请勿在参数中放入密钥。';

  @override
  String get acpProgramNotFound => '本机未找到该程序。';

  @override
  String get claudeEnvTitle => 'Claude 环境变量';

  @override
  String get claudeRuntimeTitle => '运行组件';

  @override
  String claudeAdapterInstalled(String path) {
    return 'ACP 适配器：$path';
  }

  @override
  String claudeAdapterPrepared(String path) {
    return 'ACP 适配器（由本应用准备）：$path';
  }

  @override
  String claudeAdapterWillPrepare(String package) {
    return '尚未安装 ACP 适配器。可以先准备环境，也可在开始托管时自动准备。将从 npm 下载 $package 到本应用缓存，不改动全局环境。';
  }

  @override
  String claudeAdapterMissing(String package) {
    return '尚未安装 ACP 适配器（$package），且本机无法自动准备。';
  }

  @override
  String claudeNodeFound(String version, String path) {
    return 'Node.js $version：$path';
  }

  @override
  String claudeNodeTooOld(String version) {
    return 'Node.js $version 版本过低；适配器需要 Node.js 22 或更高版本。';
  }

  @override
  String get claudeNodeMissing => '未找到 Node.js 22 或更高版本。';

  @override
  String claudeCliFound(String path) {
    return 'Claude Code：$path';
  }

  @override
  String get claudeCliInherited => 'Claude Code：使用 CLAUDE_CODE_EXECUTABLE 的设置';

  @override
  String get claudeCliBundled => '未找到 Claude Code；适配器将使用其自带版本。';

  @override
  String get claudePrepareEnvironment => '准备 ACP 环境';

  @override
  String get claudeEnvironmentReady => 'ACP 环境已就绪，未启动或重启任何托管。';

  @override
  String get claudeEnvRecheck => '重新检测';

  @override
  String get claudePrepareCancel => '停止准备';

  @override
  String get claudePrepareStopWaiting => '停止等待';

  @override
  String get claudePrepareJoined => '这次准备由其他窗口启动；本页只跟随它的进度。';

  @override
  String get claudePrepareLeftRunning => '已停止等待。其他地方启动的准备仍在继续。';

  @override
  String get claudePrepareReplaced =>
      '本页跟随的准备已被另一次准备取代，这里不显示它的结果。点击“准备 ACP 环境”或“开始托管”可重试。';

  @override
  String get quitStopping => '正在停止托管的智能体，然后退出…';

  @override
  String quitBlocked(String error) {
    return '无法安全停止托管，应用保持打开：$error';
  }

  @override
  String get quitRetry => '再次退出';

  @override
  String claudePrepareFailed(String error) {
    return '准备 Claude ACP 适配器失败：$error';
  }

  @override
  String get claudePrepareCancelled =>
      '已停止准备 Claude ACP 适配器。点击“准备 ACP 环境”或“开始托管”可重试。';

  @override
  String acpProgramFound(String path) {
    return '已找到：$path';
  }

  @override
  String get acpSaveAgent => '保存为配置';

  @override
  String get acpHostingUnsupported => '此平台暂不支持托管 ACP 智能体。你仍可使用其他设备托管的智能体。';

  @override
  String get acpPhaseStarting => '正在启动';

  @override
  String get acpPhaseReady => '运行中';

  @override
  String get acpPhaseFailed => '未运行';

  @override
  String get acpPhaseStopped => '已停止';

  @override
  String get acpAuthRequired => '智能体提示需要登录。请在主机上用智能体自己的方式登录，然后重启它。';

  @override
  String get acpRestart => '重启智能体';

  @override
  String get acpGatewayLabel => '智能体网关';

  @override
  String get acpStopNote => '停止托管会停止该智能体及其启动的进程。';

  @override
  String get acpFeatureReopen => '重新打开以前的会话';

  @override
  String get acpFeatureReopenLoad => '包含历史';

  @override
  String get acpFeatureReopenResume => '不含以前的历史';

  @override
  String get acpFeatureList => '智能体自己的会话列表';

  @override
  String get acpFeatureImages => '图片附件';

  @override
  String get acpFeatureSupported => '支持';

  @override
  String get acpFeatureNotSupported => '不支持';

  @override
  String get acpFeatureUnknown => '连接到运行中的智能体后显示';

  @override
  String permissionFromAgent(String agent) {
    return '$agent 请求权限';
  }

  @override
  String get permissionRememberTitle => '记住这个选择？';

  @override
  String permissionRememberBody(String option) {
    return '“$option”会让智能体记住这个决定。生效的时长和范围由智能体决定。';
  }

  @override
  String get permissionAnswerFailed => '未能发送你的回答，请重试。';

  @override
  String get agentOptions => '智能体选项';

  @override
  String get agentMode => '模式';

  @override
  String agentOptionSearch(String option) {
    return '搜索$option';
  }

  @override
  String get agentOptionNoMatch => '没有匹配的选项。';

  @override
  String get historyGapNotice => '此智能体主机无法提供更早的历史记录。';

  @override
  String contentOmittedNotice(String parts) {
    return '内容过大，未保留：$parts。';
  }

  @override
  String get contentUnavailableNotice => '此回复的部分内容无法从智能体主机读取。';

  @override
  String get omittedImages => '图片';

  @override
  String get omittedDiffs => '文件改动';

  @override
  String get omittedInput => '工具输入';

  @override
  String get omittedLocations => '文件位置';

  @override
  String get omittedPlan => '计划步骤';

  @override
  String get omittedContent => '部分输出';

  @override
  String get omittedSeparator => '、';

  @override
  String hostEditorTitle(String name) {
    return '主机 $name';
  }

  @override
  String get hostSectionAgent => '智能体';

  @override
  String get hostSectionStatus => '状态';

  @override
  String get hostSectionModel => '模型服务';

  @override
  String get hostSectionModelHint => '此主机的智能体向何处发送模型请求。按主机保存在本设备上。';

  @override
  String get hostSourceInherited => '现有配置';

  @override
  String get hostSourceOfficial => '官方账号';

  @override
  String get hostSourceCustom => '自定义供应商';

  @override
  String get hostInheritedAcp => '智能体沿用它自己的配置文件和环境变量，不做改动。';

  @override
  String hostInheritedCodexCustom(String provider) {
    return 'Codex 的 config.toml 已选择供应商 $provider，无需其他设置。';
  }

  @override
  String get hostInheritedCodexAccount => 'Codex 已登录，无需其他设置。';

  @override
  String get hostInheritedCodexInvalid =>
      '无法读取 Codex 的 config.toml。请修复它，或在下方选择其他方式。';

  @override
  String get hostInheritedCodexNone => 'Codex 尚未登录，也没有配置供应商。请登录或添加自定义供应商。';

  @override
  String hostInheritedCodexHome(String path) {
    return '配置目录：$path';
  }

  @override
  String get hostProtocolLabel => 'API 类型';

  @override
  String get hostProtocolResponses => 'OpenAI Responses';

  @override
  String get hostProtocolAnthropic => 'Anthropic Messages';

  @override
  String get hostProtocolChat => 'OpenAI Chat Completions';

  @override
  String get hostBaseUrl => 'Base URL';

  @override
  String get hostApiKey => 'API Key';

  @override
  String get hostApiKeyStored => '已保存密钥。留空则保持不变。';

  @override
  String get hostKeyEnvNote => '仅保存在本设备，在智能体启动时交给它，不会出现在命令行中。';

  @override
  String get hostApiKeyRemove => '删除已保存的密钥';

  @override
  String get hostModelOptional => '模型（可选）';

  @override
  String get hostBaseUrlRequired => '请输入供应商的 Base URL。';

  @override
  String get hostSaveConfig => '保存';

  @override
  String get hostConfigSaved => '已保存。运行中的主机在重启前仍使用当前配置。';

  @override
  String get hostConfigPending => '已保存的更改尚未生效。重启主机以应用。';

  @override
  String get hostApplyRestart => '应用并重启';

  @override
  String get hostSaveAndRestart => '保存并重启';

  @override
  String get hostApplyNote => '重启会短暂停止智能体并断开已打开的会话。若新配置启动失败，将恢复之前的配置。';

  @override
  String get hostApplied => '已使用新配置重启。';

  @override
  String hostApplyRestored(String error) {
    return '新配置启动失败，已恢复之前的配置：$error';
  }

  @override
  String hostApplyFailed(String error) {
    return '重启失败，主机已停止：$error';
  }

  @override
  String get hostOfficialNote => '登录在本机私有的 Codex 服务上进行，无需先运行主机。';

  @override
  String get hostAgentEditNote => '修改启动命令将在重启后生效。';

  @override
  String get hostCodexRuntimeFixed => '主机当前的运行配置。在下方修改运行时并保存，然后应用以使用它重启。';

  @override
  String hostLoginRefreshFailed(String error) {
    return '无法检查登录状态，正在重试：$error';
  }

  @override
  String get hostLoginNotOfficial => 'Codex 已完成登录，但不是 ChatGPT 账户。请重新登录。';

  @override
  String hostLoginFailed(String error) {
    return '登录失败：$error';
  }

  @override
  String get hostSourceUnsupported => '已保存的模型服务不适用于此智能体，请另选一个。';

  @override
  String get hostModelRequired => '请输入此供应商使用的默认模型。';

  @override
  String get hostKeyRequired => '请输入供应商的 API 密钥。';

  @override
  String get hostModelDefault => '默认模型';

  @override
  String get hostModelDefaultNote => '供应商用于新会话的模型。对话自身的模型在对话中选择，不保存在这里。';

  @override
  String get hostLeaveTitle => '放弃未保存的更改？';

  @override
  String get hostLeaveBody => '对此主机的更改尚未保存。';

  @override
  String get hostLeaveStay => '继续编辑';

  @override
  String get hostLeaveDiscard => '放弃';

  @override
  String hostApplyConfirmTitle(String name) {
    return '重启 $name?';
  }

  @override
  String get hostApplyConfirmBody =>
      '主机将停止并以已保存的配置重新启动。正在进行的回合会被中断，打开的对话会重新连接。本设备上的草稿、附件和排队消息会保留。其他主机不受影响。';

  @override
  String get hostReplaced => '此页面读取后，该主机已在别处重启。请先重新加载再应用。';

  @override
  String hostApplyStopped(String error, String restoreError) {
    return '新配置启动失败（$error），恢复之前的配置也失败了（$restoreError）。主机已停止。';
  }

  @override
  String get hostGone => '此主机已不再运行。';

  @override
  String get hostConfigReload => '重新加载';

  @override
  String get hostConfigNewer => '存在更新的已保存配置。重新加载以查看（将放弃你的编辑）。';

  @override
  String get hostConfigLoadFailedTitle => '配置不可用';

  @override
  String hostConfigLoadFailed(String error) {
    return '无法读取此主机已保存的配置：$error';
  }

  @override
  String get hostRuntimeEditNote => '保存的更改在应用后生效；名称保持不变。';

  @override
  String get hostAgentUnknown => '此主机未报告其智能体，无法在此编辑。';

  @override
  String get hostApiProxyNote =>
      '此主机还会发布一个独立的 Responses API 服务（api:<name>）。它始终使用本设备的官方 Codex 登录，而不是这里选择的供应商。';

  @override
  String get claudePrepareStageWorking => '正在准备 Claude ACP 适配器…';

  @override
  String get claudePrepareStageInstalling => '正在解析并安装依赖包…';

  @override
  String get claudePrepareStageDownloading => '正在下载依赖包…';

  @override
  String get claudePrepareStageVerifying => '正在校验下载的适配器…';

  @override
  String get claudePrepareStageDone => 'Claude ACP 适配器已就绪';

  @override
  String get claudePrepareStageFailed => '准备失败';

  @override
  String get claudePrepareStageCancelled => '已停止准备';

  @override
  String claudePrepareElapsed(String time) {
    return '已用时 $time';
  }

  @override
  String claudePrepareDownloaded(String size) {
    return '已下载 $size';
  }

  @override
  String claudePrepareSpeed(String speed) {
    return '下载速度 $speed';
  }

  @override
  String get claudePrepareSpeedMeasuring => '正在测速…';

  @override
  String get claudePrepareStepFetch => '下载并安装';

  @override
  String get claudePrepareStepVerify => '校验';

  @override
  String claudePrepareStep(int step, int total, String name) {
    return '第 $step 步（共 $total 步）：$name';
  }

  @override
  String get claudeEnvHint =>
      '默认继承当前主机可获取的环境变量（macOS 包括登录 shell）。打开某项后仅覆盖此主机；保存不影响运行中的主机，重启后应用。模型能力以 Claude 版本和服务商支持为准。';

  @override
  String get claudeEnvProviderWins =>
      '上方自定义服务商字段决定 URL、凭据和默认模型。环境连接覆盖仅在使用继承配置时生效。';

  @override
  String get claudeEnvInherited => '继承环境';

  @override
  String get claudeEnvCustom => '此主机自定义';

  @override
  String get claudeEnvStored => '已安全保存 · 留空保留';

  @override
  String get claudeEnvValue => '填写变量值';

  @override
  String get claudeEnvRequired => '请填写每个已启用的覆盖项，或将其切回继承环境。';

  @override
  String get claudeEnvConnection => '连接与凭据';

  @override
  String get claudeEnvModels => '模型与别名';

  @override
  String get claudeEnvReasoning => '推理与输出';

  @override
  String get claudeEnvNetwork => '网络与兼容性';

  @override
  String get claudeEnvBaseUrl => 'API 基础地址';

  @override
  String get claudeEnvAuthToken => 'Bearer 令牌';

  @override
  String get claudeEnvApiKey => 'Anthropic API 密钥';

  @override
  String get claudeEnvOAuthToken => '已有 OAuth 令牌';

  @override
  String get claudeEnvModel => '默认模型';

  @override
  String get claudeEnvOpus => 'Opus 别名对应模型';

  @override
  String get claudeEnvSonnet => 'Sonnet 别名对应模型';

  @override
  String get claudeEnvHaiku => 'Haiku 别名对应模型';

  @override
  String get claudeEnvCustomModel => '模型选择器附加选项';

  @override
  String get claudeEnvSubagentModel => '子智能体模型';

  @override
  String get claudeEnvEffort => '推理强度';

  @override
  String get claudeEnvThinkingTokens => '思考 token 预算';

  @override
  String get claudeEnvOutputTokens => '最大输出 token';

  @override
  String get claudeEnvDisableThinking => '省略 thinking 参数';

  @override
  String get claudeEnvDisableAdaptive => '关闭自适应思考（支持的模型）';

  @override
  String get claudeEnvTimeout => 'API 超时（毫秒）';

  @override
  String get claudeEnvHttpProxy => 'HTTP 代理';

  @override
  String get claudeEnvHttpsProxy => 'HTTPS 代理';

  @override
  String get claudeEnvNoProxy => '不经过代理的主机';

  @override
  String get claudeEnvDisableCaching => '关闭提示词缓存';

  @override
  String get claudeEnvDisableTraffic => '关闭非必要网络请求';

  @override
  String get claudeEnvNeedsValue => '需要填写值，或切回继承环境。';

  @override
  String get claudeEnvFlagOn => '开启';

  @override
  String get claudeEnvFlagOff => '关闭';

  @override
  String get claudeEnvCredential => '凭据';

  @override
  String get claudeEnvCredentialNote => 'Claude 只使用一种凭据；选择一种会替换此主机的其他凭据。';

  @override
  String get claudeEnvGroupInherited => '全部继承';

  @override
  String get claudeEnvGroupMissing => '仍有值待填写';

  @override
  String get claudeEnvAllInherited => '全部继承环境';

  @override
  String claudeEnvOverrides(int count) {
    return '$count 项覆盖';
  }

  @override
  String claudeEnvMissing(int count) {
    return '$count 项待填写';
  }

  @override
  String claudeEnvDormant(int count) {
    return '已保留 $count 项连接覆盖，切回继承配置后重新生效。';
  }

  @override
  String get claudeEnvEditDormant => '编辑未启用的覆盖项';

  @override
  String get nativeHistoryReadOnly => '本地历史 · 只读';

  @override
  String agentWelcomeTitle(String agent) {
    return '与 $agent 开始对话';
  }

  @override
  String agentWelcomeOpening(String agent) {
    return '正在此文件夹中打开 $agent 会话…';
  }

  @override
  String get agentWelcomeReady => '已就绪，输入第一条消息即可开始。';

  @override
  String agentWelcomeReadyModel(String model) {
    return '已就绪 · $model';
  }

  @override
  String agentWelcomeFailed(String reason) {
    return '无法打开会话：$reason';
  }

  @override
  String agentWelcomeNeedsProject(String agent) {
    return '为 $agent 选择一个工作文件夹。';
  }

  @override
  String get agentWelcomeLater => '发送第一条消息时打开会话。';

  @override
  String get agentWelcomeConnecting => '正在等待主机连接…';

  @override
  String get chooseFolder => '选择文件夹…';

  @override
  String get recentProjects => '最近的项目';

  @override
  String get agentHostSettings => '主机设置';

  @override
  String agentOpening(String agent) {
    return '正在打开 $agent…';
  }

  @override
  String get agentOpeningHint => '会话打开后会显示智能体的模型和选项。已输入的内容会保留在这里。';

  @override
  String get agentOptionsUnavailable => '选项不可用';

  @override
  String get agentOptionsRetry => '未能加载智能体选项，选择以重试。';

  @override
  String get agentErrorHint => '请检查该主机的模型服务和环境配置，然后重试。';

  @override
  String get agentAuthHint => '智能体报告需要凭据。请通过该主机的环境配置提供凭据，然后重试。';

  @override
  String get folderOnThisComputer => '在本机上选择…';

  @override
  String get folderTypePath => '输入路径';

  @override
  String get folderPathFieldLabel => '主机上的文件夹路径';

  @override
  String get folderOutsideRootsNote => '该路径不在项目文件夹内，无法在此浏览；会话开始时由主机检查。';

  @override
  String get folderPickerNoRootsHint => '仍可输入主机上任意文件夹的路径。';

  @override
  String get folderNoSubfolders => '这里没有子文件夹，可以直接使用此文件夹。';

  @override
  String get folderWillUse => '将使用';

  @override
  String get folderPathAbsolute => '请输入完整路径（从主机文件系统的根目录开始）。';

  @override
  String get folderBreadcrumbTop => '起点';

  @override
  String get noProjectsYet => '还没有项目。选择一个文件夹即可开始。';

  @override
  String get nativeLocalHistoryHint => '直接查看本机保存的会话，无需连接智能体。原生历史始终只读。';

  @override
  String get nativeHistoryLoadMore => '加载更多会话';

  @override
  String get nativeHistoryLoadOlder => '加载更早消息';

  @override
  String get nativeHistoryWindowHint => '当前显示有限的历史窗口，部分内容已省略或位于窗口之外。';

  @override
  String get providerOpenCode => 'OpenCode';

  @override
  String get agentChoiceNativeServer => '原生服务';

  @override
  String get hostInheritedOpenCode => '使用这台电脑上 OpenCode 自己的提供方和登录。';

  @override
  String get openCodeRuntimeHint =>
      'Pocket 在这台电脑上启动并管理一个 opencode serve，只能通过这个主机访问。';

  @override
  String get openCodeDetecting => '正在查找 OpenCode…';

  @override
  String openCodeFoundAt(String version, String path) {
    return 'OpenCode $version · $path';
  }

  @override
  String get openCodeNotFound => '未找到 OpenCode。请安装它或选择其可执行文件。';

  @override
  String get openCodeUnsupportedVersion => '不支持此 OpenCode 版本，请更新 OpenCode。';

  @override
  String get openCodeNoProvider =>
      '没有已登录的提供方。请在这台电脑上运行 `opencode auth login`，然后刷新。';

  @override
  String get openCodeAlreadyHosted =>
      '这份 OpenCode 历史记录已由另一个托管主机使用。请为这个主机选择“仅此主机”，或先停止那个主机。';

  @override
  String hostNameInUse(String name) {
    return '这台电脑已有托管实例使用“$name”这个名称。请换一个名称。';
  }

  @override
  String managedNameInUse(String name) {
    return '那台电脑已有服务使用“$name”这个名称。请换一个名称。';
  }

  @override
  String get openCodeHistoryTitle => '历史记录';

  @override
  String get openCodeHistoryShared => '与 OpenCode 共用';

  @override
  String get openCodeHistoryOwn => '仅此主机';

  @override
  String get openCodeHistoryInfo =>
      '共用：显示这台电脑上 OpenCode 自己的会话，与终端里的 OpenCode 相同；同一时间只能有一个主机使用。\n\n仅此主机：会话保存在这个主机自己的数据库中，与其他主机和终端里的 OpenCode 分开。';

  @override
  String openCodeHistoryTaken(String name) {
    return '“$name”正在使用共用历史记录，同一时间只能有一个主机使用。请选择“仅此主机”。';
  }

  @override
  String get openCodePath => 'OpenCode 可执行文件';

  @override
  String get openCodePathRequired => '请填写 OpenCode 可执行文件。';

  @override
  String openCodeRunning(String version) {
    return 'OpenCode $version 运行中';
  }

  @override
  String get openCodeProviders => '可用提供方';

  @override
  String get openCodeModels => '模型';

  @override
  String get openCodeDefaultModel => '默认模型';

  @override
  String get openCodeAgents => '智能体';

  @override
  String get openCodeNone => '无';

  @override
  String get openCodeConfigNote =>
      '提供方、模型和智能体来自这台电脑上 OpenCode 自己的配置，以及此主机模型服务可能添加的自定义提供方。请在那里修改 OpenCode 的配置后点刷新；如果没有生效，请重启这个主机。';

  @override
  String get openCodeScopeNote =>
      '所有 Pocket 设备都通过这一个服务写入，因此它们的回合不会重叠。这台电脑上的 OpenCode 窗口和其他 OpenCode 进程不受阻止。';

  @override
  String get refresh => '刷新';

  @override
  String get draftOptionDefault => '默认';

  @override
  String openCodeRefreshFailed(String error) {
    return '无法刷新：$error';
  }

  @override
  String get sessionHandBack => '交还给其他客户端';

  @override
  String accessQueueHeld(int count) {
    return '跟随期间有 $count 条排队消息在等待。重试会发送它们。';
  }

  @override
  String accessScopeEngine(String agent) {
    return '通过这个主机写入。这台电脑上其他 $agent 客户端不受阻止。';
  }

  @override
  String get accessScopePocket => '由这个主机持有。其他 Pocket 主机不能写入；原生客户端不受阻止。';

  @override
  String get accessScopeNone => '通过这个主机写入。其他客户端不受阻止。';

  @override
  String get accessClientClaude => 'Claude Code';

  @override
  String get accessClientOpenCode => '另一个 OpenCode 进程';

  @override
  String get accessClientPocketHost => '另一个 Pocket 主机';

  @override
  String get accessClientOther => '另一个客户端';

  @override
  String get accessStatusOpening => '正在打开';

  @override
  String get accessStatusHandedBack => '已交还';

  @override
  String get accessStatusNotOpen => '未在这里打开';

  @override
  String get accessStatusChanging => '正在交接';

  @override
  String get accessStatusUnavailable => '不可用';

  @override
  String get accessStatusUnknown => '写入方未知';

  @override
  String get accessOpening => '正在打开原会话…';

  @override
  String get accessHandedBackTitle => '已交还给其他客户端';

  @override
  String get accessHandedBackDetail => '这个主机已关闭它的写入方，其他应用可以继续原会话。不会自动收回。';

  @override
  String get accessNotOpenedTitle => '这个主机没有为该会话保持打开';

  @override
  String get accessNotOpenedDetail =>
      '主机没有为它保留准入：它可能从未在这个主机上打开过，或主机已重启，或为了腾出空间而放开了它。不会自动打开；没有其他人写入时，点“立即检查”或“在这里继续”即可打开。';

  @override
  String get accessDeletedDetail => '它已在主机上被删除。未发送的草稿和排队消息保留在这里。';

  @override
  String get accessTransitionTitle => '这个会话正在交接';

  @override
  String get accessTransitionDetail => '主机正在加载或关闭它。';

  @override
  String accessHostStoppedTitle(String agent) {
    return '$agent 主机没有运行';
  }

  @override
  String get accessHostStoppedDetail => '启动它以继续这个会话。';

  @override
  String accessExternalRunningTitle(String client) {
    return '$client 正在这个会话中运行一个回合';
  }

  @override
  String get accessExternalRunningDetail => '正在实时跟随。在此期间这里无法发送。';

  @override
  String accessExternalIdleTitle(String client) {
    return '$client 仍打开着这个会话';
  }

  @override
  String get accessExternalIdleDetail => '没有正在运行的回合。请在那里关闭会话，然后重新检查。';

  @override
  String get accessSourceOpenTitle => '另一个 OpenCode 进程打开了这个历史来源';

  @override
  String get accessSourceOpenDetail =>
      'Pocket 无法判断它正在写入哪个会话或回合。请在那里关闭或释放它，然后立即检查。';

  @override
  String accessPocketHostTitle(String client) {
    return '$client 正在写入这个会话';
  }

  @override
  String get accessPocketHostRunning => '它正在运行一个回合。';

  @override
  String get accessPocketHostIdle => '那里没有正在运行的回合，但它仍持有这个会话。';

  @override
  String accessUnsupportedTitle(String agent) {
    return '$agent 无法重新打开这个会话';
  }

  @override
  String get accessNotFoundTitle => '这个会话已不在主机上';

  @override
  String get accessUnknownTitle => '无法判断还有谁在写入这个会话';

  @override
  String get accessUnknownDetail => '主机无法检查这个会话的其他客户端。';

  @override
  String get accessNextAutomatic => '情况变化后，如果主机仍为这里保持打开，会自动继续；否则点“立即检查”即可打开。';

  @override
  String get accessNextWhenClosed =>
      '当没有其他人持有它时，如果主机仍为这里保持打开，会自动继续；否则点“立即检查”即可打开。';

  @override
  String accessHolderPid(int pid) {
    return 'pid $pid';
  }

  @override
  String accessHolderSince(String time) {
    return '自 $time';
  }

  @override
  String accessOpenIn(String host) {
    return '在 $host 中打开';
  }

  @override
  String get accessStartHost => '打开主机';

  @override
  String get accessCheckNow => '立即检查';

  @override
  String get accessContinueHere => '在这里继续';

  @override
  String get sessionAccessChanged => '打开时会话已变化，请重试。';

  @override
  String get permissionAllowOnce => '允许一次';

  @override
  String get permissionAllowAlways => '始终允许';

  @override
  String get permissionRejectOnce => '拒绝';

  @override
  String get permissionRejectAlways => '始终拒绝';

  @override
  String permissionAlwaysAllows(String patterns) {
    return '“始终允许”涵盖：$patterns';
  }

  @override
  String get userInputChooseAny => '可多选。';

  @override
  String commandPaletteLabel(int count) {
    return '$count 个命令';
  }

  @override
  String get commandSourceMcp => 'MCP';

  @override
  String get commandSourceSkill => '技能';

  @override
  String get nativeSessionFree => '未在别处打开';

  @override
  String get nativeSessionUnknown => '持有者未知';

  @override
  String nativeContinueOn(String host) {
    return '在 $host 中继续';
  }

  @override
  String nativeHostToContinue(String agent) {
    return '托管 $agent 以继续';
  }

  @override
  String nativeContinueElsewhere(String agent) {
    return '请在托管 $agent 的电脑上继续。';
  }

  @override
  String get defaultProjectTitle => '默认项目目录';

  @override
  String get defaultProjectAutomatic => '自动：由主机电脑决定';

  @override
  String get defaultProjectCreatedLater => '尚未创建。首次在这里开始会话时才会创建。';

  @override
  String get defaultProjectMissing => '主机上已没有这个目录，请另选一个。';

  @override
  String get defaultProjectNone => '没有默认目录，新会话会请你选择。';

  @override
  String get defaultProjectChange => '更改目录';

  @override
  String get defaultProjectUseAutomatic => '改回自动目录';

  @override
  String get defaultProjectSaved => '默认项目目录已保存。';

  @override
  String get defaultProjectConflict => '其他设备改过项目设置，请重新加载后再修改。';

  @override
  String defaultProjectUnavailable(String error) {
    return '项目设置暂不可用，不影响服务管理。$error';
  }

  @override
  String defaultProjectLabel(String name) {
    return '$name（默认）';
  }

  @override
  String projectRootsAdvanced(int count) {
    return '可浏览目录（$count）';
  }

  @override
  String get projectRootsHint => '其他设备只能在这些目录内浏览。留空则使用主机默认范围。';

  @override
  String get projectRootPathHint => '主机上的绝对路径，如 /Users/me/code';

  @override
  String get managedComputerTitle => '电脑管理';

  @override
  String managedComputerSummary(String os, String version) {
    return '$os · Pocket $version';
  }

  @override
  String get managedNoServices => '这台电脑还没有服务，可在下方添加。';

  @override
  String get managedAddService => '添加服务';

  @override
  String get managedAddHint => '那台电脑上已安装的运行时。此处不会安装或登录。';

  @override
  String managedAddTitle(String device) {
    return '在 $device 上新建服务';
  }

  @override
  String managedEditTitle(String name, String device) {
    return '$device 上的 $name';
  }

  @override
  String get managedNoRuntimes => '那台电脑上没有找到支持的运行时。';

  @override
  String get managedRuntimeUnavailable => '那台电脑上不可用。';

  @override
  String get managedNeedsHostLogin => '使用其自有账号需要先在那台电脑上登录；自定义提供方无需登录。';

  @override
  String get managedKindUnknown => '未知智能体';

  @override
  String get managedStateRunning => '运行中';

  @override
  String get managedStateStopped => '已停止';

  @override
  String get managedStateNew => '新建';

  @override
  String get managedStateNeedsAttention => '需要处理';

  @override
  String get managedStart => '启动';

  @override
  String get managedEdit => '编辑';

  @override
  String managedStarted(String name) {
    return '$name 已启动。';
  }

  @override
  String managedStopped(String name) {
    return '$name 已停止，仍保留在列表中，可再次启动。';
  }

  @override
  String get managedSaved => '已保存。';

  @override
  String managedStopConfirmTitle(String name) {
    return '停止 $name？';
  }

  @override
  String get managedStopConfirmBody => '它的会话会断开，进行中的回合会被中断。电脑仍可继续管理。';

  @override
  String managedRestoreProblem(String problem) {
    return '自动恢复需要处理：$problem';
  }

  @override
  String managedConfigUnreadable(String problem) {
    return '无法读取已保存的配置（$problem）。仍可停止它，请在那台电脑上修复。';
  }

  @override
  String get managedConflict => '打开后其他设备保存过这个服务。你的修改仍保留；重新加载可查看新版本（会丢弃你的修改）。';

  @override
  String get managedOutcomeUnknown => '结果未知（响应丢失）。已刷新当前状态，请确认后再操作。';

  @override
  String get managedHostBusy => '那台电脑正忙，请稍后再试。';

  @override
  String get managedAlreadyRunning => '这个服务已在运行。请刷新，或换一个名称。';

  @override
  String get managedHostStopped => '这个服务已停止，请刷新状态。';

  @override
  String get managedHostChanged => '这个服务已在别处重启或替换，已刷新状态。';

  @override
  String get managedLoginRequired => '那台电脑上尚未登录该服务的账号。请在那台电脑上登录，或改用自定义提供方。';

  @override
  String get managedPrepareRequired =>
      '请先在那台电脑上准备 Claude 适配器（添加服务 → 准备 ACP 环境）。';

  @override
  String get managedInvalidConfiguration => '启动前请检查并保存这个服务的配置。';

  @override
  String get managedConfigurationChanged => '其他设备已保存过这个服务，请重新加载后再保存。';

  @override
  String get managedOutsideRoots => '请在主机的可浏览目录内选择。';

  @override
  String get managedNameRequired => '请输入服务名称。';

  @override
  String get managedNameHelper => '在那台电脑上唯一，用于在中转上标识服务。';

  @override
  String get managedPortLabel => '端口';

  @override
  String get managedPortHelper => '0 表示自动选择空闲端口。';

  @override
  String get managedPortInvalid => '请输入 0 到 65535 之间的端口。';

  @override
  String get managedRuntimeNote => '运行那台电脑上已安装的程序，路径和参数在那台电脑上管理。';

  @override
  String get managedInheritedNote => '使用那台电脑上已有的配置。';

  @override
  String get managedOfficialNote => '使用那台电脑上已登录的官方账号；若未登录，请在那台电脑上登录。';

  @override
  String get managedDiagnostics => '诊断详情';

  @override
  String get managedKey => '管理服务键';

  @override
  String get managedDevice => '设备';

  @override
  String get managedSwitchTitle => '允许我的其他设备管理这台电脑';

  @override
  String get managedSwitchOn => '已开启';

  @override
  String get managedSwitchOff => '已关闭';

  @override
  String get managedSwitchInfo =>
      '开启后，你已登录的其他设备可在这台电脑在线时添加、配置、启动和停止它的服务；关闭后只有这台电脑能修改。';

  @override
  String get managedSwitchConnecting => '已开启，暂未连通。';

  @override
  String get managedSwitchUnavailable => '无法读取管理状态。';

  @override
  String hostRestoreBlocked(String problem) {
    return '已跳过自动恢复，因为无法读取已保存的恢复选择：$problem';
  }

  @override
  String get managedRuntimeReady => '可用';

  @override
  String get managedRuntimeNeedsSetup => '需要准备';

  @override
  String get managedRuntimeSignInThere => '可用 · 需在那台电脑上登录';

  @override
  String get managedRuntimeDetails => '详情';

  @override
  String get managedRuntimePath => '可执行文件';

  @override
  String get managedRuntimeVersion => '版本';

  @override
  String get refreshServices => '刷新服务';

  @override
  String get refreshServicesFailed => '服务刷新失败，点按重试。';

  @override
  String get managedRefreshComputer => '刷新这台电脑';

  @override
  String get composerWrite => '编辑';

  @override
  String get composerSplit => '分栏';

  @override
  String get composerPreview => '预览';

  @override
  String get composerPreviewEmpty => '还没有可预览的内容。在左侧用 Markdown 书写，这里会实时渲染。';

  @override
  String get composerPreviewEmptyTab => '还没有可预览的内容。';

  @override
  String get reviewSessionSource => '本对话';

  @override
  String get reviewWorkspaceSource => '工作区';

  @override
  String get reviewNoSessionChanges => '本对话还没有修改文件。';

  @override
  String reviewEdit(int index) {
    return '第 $index 次修改';
  }

  @override
  String get reviewAllEdits => '显示全部修改';

  @override
  String reviewEditCount(int count) {
    return '$count 次修改';
  }

  @override
  String get reviewFilterFiles => '筛选文件';

  @override
  String get reviewOpenFile => '查看完整文件';

  @override
  String get reviewShowChanges => '查看变更';

  @override
  String get reviewFileLoading => '正在读取文件…';

  @override
  String get reviewFileUnavailable => '无法从主机读取此文件。';

  @override
  String get copyPath => '复制路径';

  @override
  String get openInReview => '在审阅中打开';

  @override
  String get showTurnNavigation => '显示对话导航';

  @override
  String get automationRun => '自动化任务';

  @override
  String get reviewWorkspaceLoading => '正在读取工作区变更…';

  @override
  String get reviewWorkspaceUnavailable => '无法读取工作区的变更。';

  @override
  String relayVirtualAddress(String host, String address) {
    return '系统 DNS 把 $host 解析成了代理或 VPN 的虚拟地址 $address，目前没有路由能到达它。重新连接那个代理或 VPN 后再试。';
  }

  @override
  String relayNoAnswer(String relay) {
    return '$relay 没有响应。';
  }

  @override
  String get homeRelayLocalOnly => 'relay 暂时连不上，已直接打开本机的服务。';

  @override
  String get hostStopping => '停止中…';

  @override
  String get hostSaving => '保存中…';

  @override
  String get hostRestarting => '重启中…';

  @override
  String hostStoppedToast(String name) {
    return '$name 已停止。';
  }

  @override
  String get conversationsLoading => '正在加载会话…';
}
