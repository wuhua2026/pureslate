fn main() {
    // v0.1.3：release 构建嵌入 requireAdministrator 清单——启动时 UAC 自动提权，
    // MFT 快速扫描引擎（需管理员）在安装/便携双形态恒可用，HKLM 启动项管理恢复可写。
    // debug/test 保持默认 asInvoker：tauri dev 与 cargo test 在非提权终端不受影响
    //（PROFILE 由 build script 环境注入）。
    let attributes = if std::env::var("PROFILE").as_deref() == Ok("release") {
        // 清单内容 = tauri-build 默认（Common-Controls v6 依赖）+ requireAdministrator。
        tauri_build::WindowsAttributes::new().app_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v2">
    <security>
      <requestedPrivileges xmlns="urn:schemas-microsoft-com:asm.v3">
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#,
        )
    } else {
        tauri_build::WindowsAttributes::new()
    };
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(attributes))
        .expect("tauri build script failed");
}
