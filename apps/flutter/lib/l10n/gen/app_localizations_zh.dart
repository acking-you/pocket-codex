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
  String get apiServicesSection => 'API 服务';

  @override
  String get appServerServices => 'App-server 服务';

  @override
  String appServerSubtitle(String device) {
    return '$device · 会话功能见 P2';
  }

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
}
