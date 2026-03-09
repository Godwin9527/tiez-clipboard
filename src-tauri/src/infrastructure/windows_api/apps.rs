use serde::{Serialize, Deserialize};
use std::process::Command;

use std::os::windows::process::CommandExt;
use std::path::Path;
use crate::error::{AppResult, AppError};
use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCSTR_FRIENDLYAPPNAME, ASSOCSTR_EXECUTABLE, ASSOCF_VERIFY};
use windows::core::{PCWSTR, PWSTR};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
}

#[tauri::command]
pub async fn scan_installed_apps() -> AppResult<Vec<AppInfo>> {
    let mut apps = Vec::new();
    println!("Starting app scan...");
    
    // 1. Add known system apps directly (Backend Fallback)
    let sys_root = std::env::var("SystemRoot").unwrap_or("C:\\Windows".to_string());
    
    let common_apps = vec![
        ("Notepad (记事本)", format!(r"{}\System32\notepad.exe", sys_root)),
        ("Paint (画图)", format!(r"{}\System32\mspaint.exe", sys_root)),
        ("Calculator (计算器)", format!(r"{}\System32\calc.exe", sys_root)),
        ("Command Prompt (CMD)", format!(r"{}\System32\cmd.exe", sys_root)),
        ("PowerShell", format!(r"{}\System32\WindowsPowerShell\v1.0\powershell.exe", sys_root)),
        ("Registry Editor", format!(r"{}\regedit.exe", sys_root)),
        ("Snipping Tool", format!(r"{}\System32\SnippingTool.exe", sys_root)),
        ("Explorer", format!(r"{}\explorer.exe", sys_root)),
    ];

    for (name, path) in common_apps {
        if Path::new(&path).exists() {
            apps.push(AppInfo { name: name.to_string(), path });
        }
    }

    // Check for common browsers
    let program_files = std::env::var("ProgramFiles").unwrap_or(r"C:\Program Files".to_string());
    let program_files_x86 = std::env::var("ProgramFiles(x86)").unwrap_or(r"C:\Program Files (x86)".to_string());

    let chrome_path = format!(r"{}\Google\Chrome\Application\chrome.exe", program_files);
    if Path::new(&chrome_path).exists() {
        apps.push(AppInfo { name: "Google Chrome".to_string(), path: chrome_path });
    }
    
    let edge_path = format!(r"{}\Microsoft\Edge\Application\msedge.exe", program_files_x86);
    if Path::new(&edge_path).exists() {
        apps.push(AppInfo { name: "Microsoft Edge".to_string(), path: edge_path });
    }
    
    // 2. Run PowerShell Scan (Best Effort)
    let ps_script = r#"
        $ErrorActionPreference = 'SilentlyContinue'
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
        
        $apps = Get-StartApps | Select-Object Name, AppID
        
        $results = New-Object System.Collections.Generic.List[Object]
        foreach ($app in $apps) {
            if (![string]::IsNullOrEmpty($app.AppID)) {
                $obj = @{ name = $app.Name; path = $app.AppID }
                $results.Add($obj)
            }
        }
        
        if ($results.Count -eq 0) {
            Write-Output "[]"
        } else {
            $results | ConvertTo-Json -Depth 2 -Compress
        }
    "#;

    let output_res = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps_script])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();

    if let Ok(output) = output_res {
        if output.status.success() {
            let json_str = String::from_utf8_lossy(&output.stdout);
            if let Ok(scanned) = serde_json::from_str::<Vec<AppInfo>>(&json_str) {
                apps.extend(scanned);
            } else if let Ok(single) = serde_json::from_str::<AppInfo>(&json_str) {
                 apps.push(single);
            }
        }
    }

    // 3. Deduplicate and Filter
    let invalid_keywords = ["uninstall", "卸载", "setup", "install", "config", "help", "readme", "update", "修复", "remove"];
    
    apps.retain(|app| {
        let name_lower = app.name.to_lowercase();
        !invalid_keywords.iter().any(|&k| name_lower.contains(k))
    });

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.path.eq_ignore_ascii_case(&b.path));

    Ok(apps)
}

#[tauri::command]
pub async fn get_associated_apps(extension: String) -> AppResult<Vec<AppInfo>> {
    let ext = if extension.starts_with('.') { extension.clone() } else { format!(".{}", extension) };
    
    let ps_script = format!(r#"
        $ErrorActionPreference = 'SilentlyContinue'
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
        
        $ext = "{}"
        $list = New-Object System.Collections.Generic.List[Object]
        $addedPaths = New-Object System.Collections.Generic.HashSet[String]

        $regPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$ext\OpenWithList"
        if (Test-Path $regPath) {{
            $mru = Get-ItemProperty $regPath
            $mru.PSObject.Properties | Where-Object {{ $_.Name -match "^[a-zA-Z]$" }} | ForEach-Object {{
                $exeName = $_.Value
                if ($exeName -and $exeName.EndsWith(".exe")) {{
                    try {{
                        $cmd = Get-Command $exeName -ErrorAction SilentlyContinue
                        if ($cmd) {{
                            $fullPath = $cmd.Source
                             if (-not $addedPaths.Contains($fullPath)) {{
                                $list.Add(@{{ name = $cmd.Name; path = $fullPath }})
                                $addedPaths.Add($fullPath) | Out-Null
                             }}
                        }} else {{
                            $appPathKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\$exeName"
                            if (Test-Path $appPathKey) {{
                                $fullPath = (Get-ItemProperty $appPathKey).'(default)'
                                if ($fullPath -and (Test-Path $fullPath)) {{
                                    if (-not $addedPaths.Contains($fullPath)) {{
                                        $list.Add(@{{ name = $exeName; path = $fullPath }})
                                        $addedPaths.Add($fullPath) | Out-Null
                                    }}
                                }}
                            }}
                        }}
                    }} catch {{}}
                }}
            }}
        }}

        $list | ConvertTo-Json -Depth 2 -Compress
    "#, ext);

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        let json_str = String::from_utf8_lossy(&output.stdout);
        if let Ok(apps) = serde_json::from_str::<Vec<AppInfo>>(&json_str) {
            return Ok(apps);
        }
    }

    Ok(Vec::new())
}

#[tauri::command]
pub fn get_system_default_app(content_type: String) -> AppResult<String> {
    let ext = match content_type.as_str() {
        "image" => ".png",
        "video" => ".mp4",
        "code" => ".txt", 
        "text" => ".txt", 
        "file" => ".txt",
        "link" | "url" => "http",
        _ => return Ok("系统默认".to_string()),
    };

    unsafe {
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ext_wide: Vec<u16> = std::ffi::OsStr::new(ext).encode_wide().chain(std::iter::once(0)).collect();
        let ext_pcwstr = PCWSTR(ext_wide.as_ptr());

        let res = AssocQueryStringW(
            ASSOCF_VERIFY,
            ASSOCSTR_FRIENDLYAPPNAME,
            ext_pcwstr,
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut size
        );

        if res.is_ok() {
            let len = (0..size as usize).position(|i| buffer[i] == 0).unwrap_or(size as usize);
            let name = String::from_utf16_lossy(&buffer[0..len]);
            if !name.trim().is_empty() {
                return Ok(name);
            }
        }
        
        // Fallback to executable name
        let mut size = buffer.len() as u32;
        let res = AssocQueryStringW(
            ASSOCF_VERIFY,
            ASSOCSTR_EXECUTABLE,
            ext_pcwstr,
            PCWSTR::null(),
            Some(PWSTR(buffer.as_mut_ptr())),
            &mut size
        );
         if res.is_ok() {
            let len = (0..size as usize).position(|i| buffer[i] == 0).unwrap_or(size as usize);
            let path_str = String::from_utf16_lossy(&buffer[0..len]);
            if let Some(name) = Path::new(&path_str).file_name() {
                return Ok(name.to_string_lossy().to_string());
            }
        }
    }

    Ok("系统默认".to_string())
}

use std::os::windows::ffi::OsStrExt;

// Moved from main.rs
pub async fn launch_uwp_with_file(app_id: &str, file_path: &str) -> AppResult<()> {
    
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        return Err(AppError::Validation(format!("File does not exist: {}", file_path)));
    }
    
    let family_name = app_id.split('!').next().unwrap_or(app_id);

    let ps_script = format!(
        r#"
        Add-Type -AssemblyName System.Runtime.WindowsRuntime
        $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | ? {{ $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' }})[0]
        
        $fileOp = [Windows.Storage.StorageFile,Windows.Storage,ContentType=WindowsRuntime]::GetFileFromPathAsync('{}')
        $fileTask = $asTask.MakeGenericMethod([Windows.Storage.StorageFile]).Invoke($null, @($fileOp))
        $file = $fileTask.GetAwaiter().GetResult()
        
        $options = New-Object Windows.System.LauncherOptions
        $options.TargetApplicationPackageFamilyName = '{}'
        
        $launchOp = [Windows.System.Launcher,Windows.System,ContentType=WindowsRuntime]::LaunchFileAsync($file, $options)
        $launchTask = $asTask.MakeGenericMethod([Boolean]).Invoke($null, @($launchOp))
        $result = $launchTask.GetAwaiter().GetResult()
        
        if ($result) {{ exit 0 }} else {{ exit 1 }}
        "#,
        file_path.replace("'", "''"),
        family_name
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("Starting PowerShell failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("WinRT Launch failed: {}", stderr.trim()).into())
    }
}
