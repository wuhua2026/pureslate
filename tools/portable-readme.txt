PureSlate 便携版使用说明
========================

【这是什么】
免安装版本：解压到任意目录，直接运行 pureslate.exe 即可，
无需安装器。功能与安装版完全一致。

【运行前提】
- Windows 10 / 11（x64）；
- 需要 Microsoft Edge WebView2 Runtime（Win11 自带；Win10 大多自带，
  缺失时从 https://developer.microsoft.com/microsoft-edge/webview2/
  下载 Evergreen Bootstrapper 安装一次即可）。

【重要：便携 = 免安装，不是零写入】
PureSlate 的安全设计要求固定数据位置，以下内容仍会写入本机：
- %LOCALAPPDATA%\PureSlate\        设置、审计日志、事务 journal、崩溃转储
- <各卷根>\.pureslate-quarantine\  隔离区（隐藏目录，14 天可还原）
请勿删除这些目录，否则隔离区内的文件将无法还原。

【升级方式】
设置页"检查更新"提示新版本后：
1. 关闭正在运行的 PureSlate；
2. 下载新版便携 zip，解压覆盖本目录（pureslate.exe 与 rules\）；
3. 重新运行。设置、审计日志与隔离区数据不受影响。

【安全提示】
- 本版本未签名（开源签名计划申请中）：SmartScreen 提示时点击
  "更多信息"→"仍要运行"；
- 唯一官方发布渠道 = https://github.com/wuhua2026/pureslate/releases
  请勿从其他来源下载。
