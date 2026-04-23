#![windows_subsystem = "windows"]

mod ui;

use anyhow::{Context, Result, anyhow};
use mslnk::ShellLink;
use serde::{Deserialize, Serialize};
use std::env;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    STGM_READWRITE, STGM_SHARE_DENY_NONE,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    IShellLinkW, SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify,
    SetCurrentProcessExplicitAppUserModelID, ShellLink as CLSID_ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_YESNO, MessageBoxW, IDYES,
};
use windows::core::{ComInterface, PCWSTR, PWSTR};
use winreg::RegKey;
use winreg::enums::*;

const CREATE_NO_WINDOW_FLAG: u32 = 0x08000000;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
enum UpdateMethod {
    Installer,
    Winget,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AppConfig {
    update_method: UpdateMethod,
    interceptor_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            update_method: UpdateMethod::Installer,
            interceptor_enabled: false,
        }
    }
}

#[derive(Deserialize, Debug)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize, Debug)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

// --- Logic Functions ---

fn check_winget_installed() -> bool {
    Command::new("winget")
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn is_installed_via_winget() -> bool {
    let output = Command::new("winget")
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .args(["list", "--id", "ImputNet.Helium", "--source", "winget"])
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) => {
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("ImputNet.Helium")
        }
        Err(_) => false,
    }
}

fn get_system_architecture() -> &'static str {
    let arch = env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default();
    let arch_w6432 = env::var("PROCESSOR_ARCHITEW6432").unwrap_or_default();

    if arch.eq_ignore_ascii_case("ARM64") || arch_w6432.eq_ignore_ascii_case("ARM64") {
        "arm64"
    } else {
        "x64"
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum InstallScope {
    User,
    System,
}

fn scan_registry_for_helium() -> (bool, Option<String>, Option<InstallScope>) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let exact_paths = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Helium",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Helium",
    ];
    let base_paths = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];

    // 1. Try exact paths first (fastest)
    for (hive, scope) in [(&hkcu, InstallScope::User), (&hklm, InstallScope::System)] {
        for path in &exact_paths {
            if let Ok(key) = hive.open_subkey(path) {
                let version: String = key.get_value("DisplayVersion").unwrap_or_default();
                let normalized = normalize_version_label(&version);
                return (
                    true,
                    if normalized.is_empty() {
                        None
                    } else {
                        Some(normalized)
                    },
                    Some(scope),
                );
            }
        }
    }

    // 2. Scan all uninstall entries (slower)
    for (hive, scope) in [(&hkcu, InstallScope::User), (&hklm, InstallScope::System)] {
        for base_path in &base_paths {
            if let Ok(uninstall_key) = hive.open_subkey(base_path) {
                for name in uninstall_key.enum_keys().filter_map(|x| x.ok()) {
                    if let Ok(subkey) = uninstall_key.open_subkey(&name) {
                        let display_name: String =
                            subkey.get_value("DisplayName").unwrap_or_default();
                        if display_name.to_ascii_lowercase().contains("helium")
                            || name.to_lowercase().contains("helium")
                        {
                            let version: String =
                                subkey.get_value("DisplayVersion").unwrap_or_default();
                            let normalized = normalize_version_label(&version);
                            return (
                                true,
                                if normalized.is_empty() {
                                    None
                                } else {
                                    Some(normalized)
                                },
                                Some(scope),
                            );
                        }
                    }
                }
            }
        }
    }

    (false, None, None)
}

fn get_installed_version() -> Option<String> {
    let (installed, version, _) = scan_registry_for_helium();
    if version.is_some() {
        return version;
    }

    if installed {
        // We found it in registry but without version, maybe it's just winget?
        // But scan_registry_for_helium already returns true if found.
    }

    get_winget_installed_version()
}

fn get_winget_installed_version() -> Option<String> {
    if !check_winget_installed() {
        return None;
    }

    let output = Command::new("winget")
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .args([
            "list",
            "--id",
            "ImputNet.Helium",
            "--source",
            "winget",
            "--exact",
            "--accept-source-agreements",
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if !line.contains("ImputNet.Helium") {
            continue;
        }

        let columns: Vec<&str> = line.split_whitespace().collect();
        if let Some(id_index) = columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case("ImputNet.Helium"))
        {
            if let Some(version) = columns.get(id_index + 1) {
                let normalized = normalize_version_label(version);
                if !normalized.is_empty() {
                    return Some(normalized);
                }
            }
        }
    }

    None
}

fn normalize_version_label(version: &str) -> String {
    version
        .trim()
        .trim_start_matches(|c| c == 'v' || c == 'V')
        .trim()
        .to_string()
}

fn version_greater_than(a: &str, b: &str) -> bool {
    let a_parts: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let b_parts: Vec<u32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    for (a, b) in a_parts.iter().zip(b_parts.iter()) {
        if a != b {
            return a > b;
        }
    }
    a_parts.len() > b_parts.len()
}

async fn perform_winget_update() -> Result<()> {
    let is_installed = is_installed_via_winget();
    let action = if is_installed { "upgrade" } else { "install" };

    let status = Command::new("winget")
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .args([
            action,
            "--id",
            "ImputNet.Helium",
            "--source",
            "winget",
            "--silent",
            "--disable-interactivity",
            "--accept-package-agreements",
            "--accept-source-agreements",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if !status.success() {
        return Err(anyhow!(
            "winget {} failed with exit code {:?}",
            action,
            status.code()
        ));
    }

    let msg = if is_installed {
        "Helium update finished successfully (winget)."
    } else {
        "Helium installation finished successfully (winget)."
    };
    show_update_success_notification(msg);

    Ok(())
}

async fn perform_github_update() -> Result<()> {
    let client = reqwest::Client::builder().user_agent("hupdater").build()?;
    let release: Release = client
        .get("https://api.github.com/repos/imputnet/helium-windows/releases/latest")
        .send()
        .await?
        .json()
        .await?;
    let arch = get_system_architecture();
    let asset_name_part = format!("_{}-installer.exe", arch);
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(&asset_name_part))
        .ok_or_else(|| anyhow!("No installer found"))?;
    let temp_path = env::temp_dir().join(&asset.name);
    let response = reqwest::get(&asset.browser_download_url).await?;
    std::fs::write(&temp_path, &response.bytes().await?)?;

    let status = Command::new(&temp_path)
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .arg("/S")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if !status.success() {
        return Err(anyhow!(
            "Helium installer failed with exit code {:?}",
            status.code()
        ));
    }

    show_update_success_notification("Helium update finished successfully.");

    Ok(())
}

fn get_helium_exe_path() -> PathBuf {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let paths = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Helium",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Helium",
    ];
    for hive in &[&hkcu, &hklm] {
        for path in &paths {
            if let Ok(key) = hive.open_subkey(path) {
                if let Ok(loc) = key.get_value::<String, _>("InstallLocation") {
                    let p = Path::new(&loc).join("chrome.exe");
                    if p.exists() {
                        return p;
                    }
                    let p = Path::new(&loc).join("Helium.exe");
                    if p.exists() {
                        return p;
                    }
                }
            }
        }
    }
    let lap = env::var("LOCALAPPDATA").unwrap_or_default();
    let p = Path::new(&lap).join(r"imput\Helium\Application\chrome.exe");
    if p.exists() {
        return p;
    }

    let pf = env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
    let p = Path::new(&pf).join(r"imput\Helium\Application\chrome.exe");
    if p.exists() {
        return p;
    }

    p
}

fn get_shortcut_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();

    // 0: User Desktop
    if let Ok(d) = env::var("USERPROFILE") {
        v.push(Path::new(&d).join("Desktop").join("Helium.lnk"));
    } else {
        v.push(PathBuf::new());
    }
    
    // 1: Public Desktop
    if let Ok(p) = env::var("PUBLIC") {
        v.push(Path::new(&p).join("Desktop").join("Helium.lnk"));
    } else {
        v.push(PathBuf::new());
    }

    // 2: User Start Menu
    if let Ok(a) = env::var("APPDATA") {
        let base = Path::new(&a).join(r"Microsoft\Windows\Start Menu\Programs");
        let p1 = base.join("Helium.lnk");
        let p2 = base.join("Helium").join("Helium.lnk");
        if p2.exists() {
            v.push(p2);
        } else {
            v.push(p1);
        }
    } else {
        v.push(PathBuf::new());
    }

    // 3: Public Start Menu
    if let Ok(pd) = env::var("PROGRAMDATA") {
        let base = Path::new(&pd).join(r"Microsoft\Windows\Start Menu\Programs");
        let p1 = base.join("Helium.lnk");
        let p2 = base.join("Helium").join("Helium.lnk");
        if p2.exists() {
            v.push(p2);
        } else {
            v.push(p1);
        }
    } else {
        v.push(PathBuf::new());
    }

    // 4: Taskbar Pinned Icons
    if let Ok(a) = env::var("APPDATA") {
        v.push(
            Path::new(&a)
                .join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\Helium.lnk"),
        );
    } else {
        v.push(PathBuf::new());
    }

    v
}

fn is_valid_app_id(app_id: &str) -> bool {
    !app_id.is_empty() && !app_id.contains(' ') && app_id.len() <= 128
}

fn read_registry_string(hive: &RegKey, key_path: &str, value_name: &str) -> Option<String> {
    let key = hive.open_subkey(key_path).ok()?;
    let value: String = key.get_value(value_name).ok()?;
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn app_id_from_classes_key(hive: &RegKey, key_name: &str) -> Option<String> {
    let base = format!(r"Software\Classes\{}", key_name);
    let app_id = read_registry_string(hive, &base, "AppUserModelId").or_else(|| {
        read_registry_string(hive, &format!(r"{}\Application", base), "AppUserModelId")
    })?;
    if is_valid_app_id(&app_id) {
        Some(app_id)
    } else {
        None
    }
}

fn find_app_id_in_start_menu_internet(hive: &RegKey) -> Option<String> {
    let start_menu = hive
        .open_subkey(r"Software\Clients\StartMenuInternet")
        .ok()?;
    for key_name in start_menu.enum_keys().filter_map(|x| x.ok()) {
        if key_name.to_ascii_lowercase().starts_with("helium") && is_valid_app_id(&key_name) {
            return Some(key_name);
        }
    }
    None
}

fn find_app_id_in_classes_prefix(hive: &RegKey, prefix: &str) -> Option<String> {
    let classes = hive.open_subkey(r"Software\Classes").ok()?;
    for key_name in classes.enum_keys().filter_map(|x| x.ok()) {
        if key_name.starts_with(prefix) {
            if let Some(app_id) = app_id_from_classes_key(hive, &key_name) {
                return Some(app_id);
            }
        }
    }
    None
}

fn resolve_helium_app_id() -> Result<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    for hive in [&hkcu, &hklm] {
        if let Some(app_id) = app_id_from_classes_key(hive, "HeliumHTM") {
            return Ok(app_id);
        }
        if let Some(app_id) = app_id_from_classes_key(hive, "HeliumPDF") {
            return Ok(app_id);
        }
    }

    for hive in [&hkcu, &hklm] {
        if let Some(app_id) = find_app_id_in_start_menu_internet(hive) {
            return Ok(app_id);
        }
    }

    for hive in [&hkcu, &hklm] {
        if let Some(app_id) = find_app_id_in_classes_prefix(hive, "HeliumHTM") {
            return Ok(app_id);
        }
        if let Some(app_id) = find_app_id_in_classes_prefix(hive, "HeliumPDF") {
            return Ok(app_id);
        }
    }

    Err(anyhow!(
        "Could not resolve Helium AppUserModelID from the registry."
    ))
}

fn to_wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn escape_for_powershell_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn show_interceptor_setup_failure_notification(error: &anyhow::Error) {
    let details = format!("{:#}", error);
    let body = format!(
        "Could not enable launch interception.\n\nReason:\n{}\n\nRecommended fixes:\n1) Launch Helium once, then close it.\n2) Set Helium as the default browser once.\n3) Restore or recreate Helium shortcuts and unpin/repin the taskbar icon.\n4) If it still fails, repair or reinstall Helium.\n5) Try \"Enable Interceptor\" again.",
        details
    );
    let title_w = to_wide_null("HUpdater - Interceptor Setup Failed");
    let body_w = to_wide_null(&body);

    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(body_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn show_update_success_notification(message: &str) {
    let title_w = to_wide_null("HUpdater - Update Completed");
    let body_w = to_wide_null(message);
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(body_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn refresh_shell_icons() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

fn restart_explorer_shell() -> Result<()> {
    let stop_status = Command::new("taskkill")
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .args(["/F", "/IM", "explorer.exe"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to stop Explorer")?;

    if !stop_status.success() {
        return Err(anyhow!(
            "Stopping Explorer returned exit code {:?}",
            stop_status.code()
        ));
    }

    std::thread::sleep(std::time::Duration::from_millis(600));

    Command::new("explorer.exe")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to restart Explorer")?;

    Ok(())
}

fn set_shortcut_aumid(path: &Path, aumid: &str) -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let shell_link: IShellLinkW =
            CoCreateInstance(&CLSID_ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let persist_file: windows::Win32::System::Com::IPersistFile = shell_link.cast()?;

        let path_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        persist_file.Load(
            PCWSTR(path_wide.as_ptr()),
            STGM_READWRITE | STGM_SHARE_DENY_NONE,
        )?;

        let property_store: IPropertyStore = shell_link.cast()?;

        // PKEY_AppUserModel_ID: {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5
        let pkey = windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY {
            fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
            pid: 5,
        };

        let mut pv = PROPVARIANT::default();
        let mut aumid_wide: Vec<u16> = aumid.encode_utf16().chain(std::iter::once(0)).collect();
        (*pv.Anonymous.Anonymous).vt = VT_LPWSTR;
        (*pv.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(aumid_wide.as_mut_ptr());

        property_store.SetValue(&pkey, &pv)?;
        property_store.Commit()?;

        persist_file.Save(PCWSTR(path_wide.as_ptr()), true)?;
    }
    Ok(())
}

async fn setup_interceptor() -> Result<()> {
    let app_id = resolve_helium_app_id().context("Failed to resolve Helium AppUserModelID")?;
    let exe = env::current_exe()?;
    let wd = exe.parent().unwrap_or(Path::new("."));

    let shortcut_paths: Vec<PathBuf> = get_shortcut_paths()
        .into_iter()
        .filter(|p| p.exists())
        .collect();
    if shortcut_paths.is_empty() {
        return Ok(());
    }

    let mut backups = Vec::new();
    for p in &shortcut_paths {
        let bytes = tokio::fs::read(p).await.with_context(|| {
            format!(
                "Failed to backup shortcut before interception: {}",
                p.display()
            )
        })?;
        backups.push((p.clone(), bytes));
    }

    let apply_result: Result<()> = (|| {
        for p in &shortcut_paths {
            let mut sl = ShellLink::new(&exe)?;
            sl.set_arguments(Some("--launch".to_string()));
            sl.set_working_dir(Some(wd.to_string_lossy().to_string()));
            sl.set_icon_location(Some(exe.to_string_lossy().to_string()));
            sl.create_lnk(p)
                .with_context(|| format!("Failed to rewrite shortcut target: {}", p.display()))?;

            set_shortcut_aumid(p, &app_id).with_context(|| {
                format!("Failed to set shortcut AppUserModelID: {}", p.display())
            })?;
        }
        Ok(())
    })();

    if let Err(error) = apply_result {
        for (path, bytes) in backups {
            let _ = tokio::fs::write(path, bytes).await;
        }
        return Err(error.context("Interceptor setup failed and changes were rolled back."));
    }

    refresh_shell_icons();

    Ok(())
}

async fn remove_interceptor() -> Result<()> {
    let h_exe = get_helium_exe_path();
    let wd = h_exe.parent().unwrap_or(Path::new("."));
    for p in get_shortcut_paths() {
        if p.exists() {
            let mut sl = ShellLink::new(&h_exe)?;
            sl.set_working_dir(Some(wd.to_string_lossy().to_string()));
            sl.create_lnk(&p)?;
        }
    }
    refresh_shell_icons();
    Ok(())
}

async fn fetch_latest_github_version() -> Option<String> {
    let client = reqwest::Client::builder()
        .user_agent("hupdater")
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;

    let release = client
        .get("https://api.github.com/repos/imputnet/helium-windows/releases/latest")
        .send()
        .await
        .ok()?
        .json::<Release>()
        .await
        .ok()?;

    let latest = normalize_version_label(&release.tag_name);
    if latest.is_empty() {
        None
    } else {
        Some(latest)
    }
}

async fn check_hupdater_updates() {
    let client = match reqwest::Client::builder()
        .user_agent("hupdater")
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let release: Release = match client
        .get("https://api.github.com/repos/xxanqw/hupdater/releases/latest")
        .send()
        .await
    {
        Ok(r) => match r.json().await {
            Ok(j) => j,
            Err(_) => return,
        },
        Err(_) => return,
    };

    let latest = normalize_version_label(&release.tag_name);
    let current = env!("CARGO_PKG_VERSION");

    if !latest.is_empty() && version_greater_than(&latest, current) {
        let msg = format!(
            "A new version of HUpdater (v{}) is available! Current version is v{}.\n\nWould you like to open the download page?",
            latest, current
        );
        let title_w = to_wide_null("HUpdater Update Available");
        let msg_w = to_wide_null(&msg);

        unsafe {
            let res = MessageBoxW(
                None,
                PCWSTR(msg_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                MB_YESNO | MB_ICONINFORMATION,
            );

            if res == IDYES {
                let _ = Command::new("cmd")
                    .args([
                        "/c",
                        "start",
                        "https://github.com/xxanqw/hupdater/releases/latest",
                    ])
                    .spawn();
            }
        }
    }
}

async fn handle_silent_launch() -> Result<()> {
    // 0. Set AppID to match Helium so taskbar grouping is correct
    if let Ok(app_id) = resolve_helium_app_id() {
        let app_id: Vec<u16> = app_id.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(app_id.as_ptr()));
        }
    }

    let h_exe = get_helium_exe_path();
    let wd = h_exe.parent().unwrap_or(Path::new("."));
    let args: Vec<String> = env::args().skip(2).collect();
    let installed = get_installed_version();

    if h_exe.exists() {
        Command::new(&h_exe)
            .creation_flags(CREATE_NO_WINDOW_FLAG)
            .args(args)
            .current_dir(wd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    } else {
        return Err(anyhow!("Helium executable not found at {:?}", h_exe));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .user_agent("hupdater")
        .build()?;
    if let Ok(resp) = client
        .get("https://api.github.com/repos/imputnet/helium-windows/releases/latest")
        .send()
        .await
    {
        if let Ok(rel) = resp.json::<Release>().await {
            let latest = rel.tag_name.trim_start_matches('v');
            if let Some(inst) = installed {
                if inst != latest {
                    let arch = get_system_architecture();
                    let part = format!("_{}-installer.exe", arch);
                    if let Some(a) = rel.assets.iter().find(|x| x.name.contains(&part)) {
                        let url = a.browser_download_url.clone();
                        let asset_name = a.name.clone();
                        if let Ok(r) = reqwest::get(&url).await {
                            if let Ok(b) = r.bytes().await {
                                let t = env::temp_dir().join(&asset_name);
                                if tokio::fs::write(&t, &b).await.is_ok() {
                                    if let Ok(status) = Command::new(&t)
                                        .creation_flags(CREATE_NO_WINDOW_FLAG)
                                        .arg("/S")
                                        .stdin(std::process::Stdio::null())
                                        .stdout(std::process::Stdio::null())
                                        .stderr(std::process::Stdio::null())
                                        .status()
                                    {
                                        if status.success() {
                                            show_update_success_notification(
                                                "Helium was updated successfully in the background.",
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn handle_self_install() -> Result<()> {
    let current_exe = env::current_exe()?;
    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_default();
    let target_dir = Path::new(&local_app_data).join("hupdater");
    let target_exe = target_dir.join("hupdater.exe");

    if current_exe == target_exe {
        return Ok(());
    }

    std::fs::create_dir_all(&target_dir)?;
    std::fs::copy(&current_exe, &target_exe)?;

    let args: Vec<String> = env::args().skip(1).collect();
    Command::new(&target_exe)
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    std::process::exit(0);
}

async fn uninstall_self() -> Result<()> {
    let _ = remove_interceptor().await;
    let local_app_data = env::var("LOCALAPPDATA").context("LOCALAPPDATA is not set")?;
    let app_data = env::var("APPDATA").context("APPDATA is not set")?;
    let local_target = Path::new(&local_app_data).join("hupdater");
    let roaming_target = Path::new(&app_data).join("hupdater");
    let script_path = env::temp_dir().join(format!("hupdater_cleanup_{}.ps1", std::process::id()));

    let cleanup_script = format!(
        r#"$pidToWait = {pid}
$targets = @(
    '{local_target}',
    '{roaming_target}'
)

$failedPaths = New-Object System.Collections.Generic.List[string]

for ($i = 0; $i -lt 120; $i++) {{
    if (-not (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue)) {{
        break
    }}
    Start-Sleep -Milliseconds 250
}}

foreach ($target in $targets) {{
    if ([string]::IsNullOrWhiteSpace($target)) {{
        continue
    }}

    if (-not (Test-Path -LiteralPath $target)) {{
        continue
    }}

    $removed = $false
    for ($attempt = 0; $attempt -lt 12; $attempt++) {{
        try {{
            Remove-Item -LiteralPath $target -Recurse -Force -ErrorAction Stop
            $removed = $true
            break
        }} catch {{
            Start-Sleep -Milliseconds 300
        }}
    }}

    if (-not $removed -and (Test-Path -LiteralPath $target)) {{
        $failedPaths.Add($target)
    }}
}}

Add-Type -AssemblyName System.Windows.Forms
if ($failedPaths.Count -eq 0) {{
    [System.Windows.Forms.MessageBox]::Show(
        "HUpdater removed successfully.",
        "HUpdater Uninstall",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Information
    ) | Out-Null
}} else {{
    $details = ($failedPaths | ForEach-Object {{ "- $_" }}) -join "`r`n"
    [System.Windows.Forms.MessageBox]::Show(
        "HUpdater was removed partially.`r`n`r`nCould not delete:`r`n$details",
        "HUpdater Uninstall",
        [System.Windows.Forms.MessageBoxButtons]::OK,
        [System.Windows.Forms.MessageBoxIcon]::Warning
    ) | Out-Null
}}

$scriptPath = $MyInvocation.MyCommand.Path
if (-not [string]::IsNullOrWhiteSpace($scriptPath)) {{
    $escapedScriptPath = $scriptPath.Replace("'", "''")
    $deleteCommand = "Start-Sleep -Milliseconds 500; Remove-Item -LiteralPath '$escapedScriptPath' -Force -ErrorAction SilentlyContinue"
    Start-Process -FilePath "powershell.exe" -WindowStyle Hidden -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-Command", $deleteCommand
    ) | Out-Null
}}
"#,
        pid = std::process::id(),
        local_target = escape_for_powershell_single_quote(local_target.to_string_lossy().as_ref()),
        roaming_target =
            escape_for_powershell_single_quote(roaming_target.to_string_lossy().as_ref()),
    );

    tokio::fs::write(&script_path, cleanup_script)
        .await
        .with_context(|| format!("Failed to write cleanup script: {}", script_path.display()))?;

    Command::new("powershell.exe")
        .creation_flags(CREATE_NO_WINDOW_FLAG)
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-WindowStyle")
        .arg("Hidden")
        .arg("-File")
        .arg(&script_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to launch uninstall cleanup script")?;

    std::process::exit(0);
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = handle_self_install();
    tokio::spawn(check_hupdater_updates());

    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "--launch" {
        handle_silent_launch().await?;
        return Ok(());
    }

    let config: AppConfig = confy::load("hupdater", None).unwrap_or_default();
    let config_lock = Arc::new(Mutex::new(config.clone()));

    let mut initial_state = ui::UiState::default();
    initial_state.use_winget = config.update_method == UpdateMethod::Winget;
    initial_state.interceptor_enabled = config.interceptor_enabled;
    initial_state.controls_enabled = false;
    initial_state.task_info = "Checking Helium installation...".to_string();

    // Status Check (Async)
    tokio::spawn(async move {
        let (reg, installed_version_raw, scope) = scan_registry_for_helium();
        let winget = if check_winget_installed() {
            is_installed_via_winget()
        } else {
            false
        };
        let base_status = if reg && winget {
            "Installed (Registry & Winget)"
        } else if reg {
            "Installed (via Installer)"
        } else if winget {
            "Installed (via Winget)"
        } else {
            "Not detected"
        };

        let installed_version = installed_version_raw
            .map(|v| normalize_version_label(&v))
            .filter(|v| !v.is_empty());
        let latest_version = if reg || winget {
            fetch_latest_github_version().await
        } else {
            None
        };

        let status = if let Some(installed) = installed_version {
            let version_state = if let Some(latest) = latest_version {
                if installed.eq_ignore_ascii_case(&latest) {
                    "latest".to_string()
                } else {
                    format!("outdated, latest v{}", latest)
                }
            } else {
                "latest unknown".to_string()
            };

            format!("{} | v{} ({})", base_status, installed, version_state)
        } else {
            base_status.to_string()
        };

        let helium_installed = reg || winget;
        ui::events::post_ui_event(ui::events::UiEvent::SetInstallStatus(status));
        ui::events::post_ui_event(ui::events::UiEvent::SetHeliumInstalled(helium_installed));
        ui::events::post_ui_event(ui::events::UiEvent::SetControlsEnabled(true));
        ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo("Ready".to_string()));
        if let Some(s) = scope {
            ui::events::post_ui_event(ui::events::UiEvent::SetInstallScope(
                if s == InstallScope::System { "system" } else { "user" }.to_string(),
            ));
        }
    });

    // Callbacks
    let c_lock = config_lock.clone();
    let on_save_settings: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |use_winget| {
        let mut config = c_lock.lock().unwrap();
        config.update_method = if use_winget {
            UpdateMethod::Winget
        } else {
            UpdateMethod::Installer
        };
        let _ = confy::store("hupdater", None, &*config);
    });

    let interceptor_lock = config_lock.clone();
    let on_enable_interceptor: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let interceptor_lock = interceptor_lock.clone();
        tokio::spawn(async move {
            let res = setup_interceptor().await;
            let explorer_res = if res.is_ok() { restart_explorer_shell() } else { Ok(()) };

            match res {
                Ok(_) => {
                    ui::events::post_ui_event(ui::events::UiEvent::SetInterceptorEnabled(true));
                    {
                        let mut config = interceptor_lock.lock().unwrap();
                        config.interceptor_enabled = true;
                        let _ = confy::store("hupdater", None, &*config);
                    }

                    match explorer_res {
                        Ok(_) => ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(
                            "Interceptor enabled. Explorer restarted.".to_string(),
                        )),
                        Err(e) => ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(
                            format!("Interceptor enabled, but Explorer restart failed: {}. Please restart Explorer manually.", e),
                        )),
                    }
                }
                Err(e) => {
                    ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(
                        "Interceptor setup failed. See notification for fixes.".to_string(),
                    ));
                    show_interceptor_setup_failure_notification(&e);
                }
            }
            ui::events::post_ui_event(ui::events::UiEvent::SetIsBusy(false));
        });
    });

    let interceptor_lock = config_lock.clone();
    let on_restore_interceptor: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let interceptor_lock = interceptor_lock.clone();
        tokio::spawn(async move {
            let res = remove_interceptor().await;
            let explorer_res = if res.is_ok() { restart_explorer_shell() } else { Ok(()) };

            match res {
                Ok(_) => {
                    ui::events::post_ui_event(ui::events::UiEvent::SetInterceptorEnabled(false));
                    {
                        let mut config = interceptor_lock.lock().unwrap();
                        config.interceptor_enabled = false;
                        let _ = confy::store("hupdater", None, &*config);
                    }

                    match explorer_res {
                        Ok(_) => ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(
                            "Restored original shortcuts. Explorer restarted.".to_string(),
                        )),
                        Err(e) => ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(
                            format!("Restored original shortcuts, but Explorer restart failed: {}. Please restart Explorer manually.", e),
                        )),
                    }
                }
                Err(e) => {
                    ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(format!("Error: {}", e)));
                }
            }
            ui::events::post_ui_event(ui::events::UiEvent::SetIsBusy(false));
        });
    });

    let on_uninstall_updater: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        tokio::spawn(async move {
            let _ = uninstall_self().await;
        });
    });

    let c_lock = config_lock.clone();
    let on_trigger_update: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let method = c_lock.lock().unwrap().update_method.clone();
        tokio::spawn(async move {
            let res = match method {
                UpdateMethod::Winget => perform_winget_update().await,
                UpdateMethod::Installer => perform_github_update().await,
            };

            ui::events::post_ui_event(ui::events::UiEvent::SetIsBusy(false));
            match res {
                Ok(_) => ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(
                    "Update completed successfully.".to_string(),
                )),
                Err(e) => ui::events::post_ui_event(ui::events::UiEvent::SetTaskInfo(
                    format!("Error: {}", e),
                )),
            }
        });
    });

    ui::run_app(ui::AppCallbacks {
        on_trigger_update,
        on_enable_interceptor,
        on_restore_interceptor,
        on_uninstall_updater,
        on_save_settings,
    }, initial_state)?;

    Ok(())
}
