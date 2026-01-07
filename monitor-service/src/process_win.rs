use anyhow::{Context, Result};
use std::ptr;
use std::mem;

#[cfg(windows)]
use windows::{
    core::PWSTR,
    Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, BOOL},
    Win32::Security::{
        DuplicateTokenEx, SecurityIdentification, TokenPrimary, 
        SECURITY_ATTRIBUTES, TOKEN_ALL_ACCESS,
        TOKEN_DUPLICATE, TOKEN_QUERY,
    },
    Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, ProcessIdToSessionId},
    Win32::System::Threading::{
        CreateProcessAsUserW, WaitForSingleObject,
        PROCESS_INFORMATION, STARTUPINFOW, STARTF_USESTDHANDLES, INFINITE,
        CREATE_NO_WINDOW, NORMAL_PRIORITY_CLASS, OpenProcess, PROCESS_QUERY_INFORMATION,
        OpenProcessToken,
    },
    Win32::System::Pipes::CreatePipe,
    Win32::Storage::FileSystem::ReadFile,
    Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    },
};

#[cfg(not(windows))]
pub fn run_agent_in_active_session(_args: &[&str]) -> Result<String> {
    Ok("[]".to_string())
}

#[cfg(windows)]
fn get_session_user_token(session_id: u32) -> Result<(HANDLE, bool)> {
    unsafe {
        // Try to find a process in the session to steal token from
        // Priority: explorer.exe (User), winlogon.exe (System/Login)
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(anyhow::anyhow!("Failed to create process snapshot"));
        }

        let mut entry = PROCESSENTRY32W {
            dwSize: mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        let mut target_pid = 0;
        let mut found_winlogon = 0;
        let mut is_winlogon = false;

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                let mut proc_session_id = 0;
                
                if ProcessIdToSessionId(pid, &mut proc_session_id).is_ok() && proc_session_id == session_id {
                    let name = String::from_utf16_lossy(&entry.szExeFile)
                        .trim_matches(char::from(0))
                        .to_lowercase();
                    
                    if name == "explorer.exe" {
                        target_pid = pid;
                        is_winlogon = false;
                        break; // Found best candidate
                    } else if name == "winlogon.exe" {
                        found_winlogon = pid;
                    }
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);

        if target_pid == 0 {
            if found_winlogon != 0 {
                target_pid = found_winlogon;
                is_winlogon = true;
            } else {
                 return Err(anyhow::anyhow!("No suitable process found in session {}", session_id));
            }
        }

        let h_process = OpenProcess(PROCESS_QUERY_INFORMATION, BOOL(0), target_pid)?;
        let mut h_token = HANDLE::default();
        
        if OpenProcessToken(h_process, TOKEN_DUPLICATE | TOKEN_QUERY, &mut h_token).is_err() {
             let _ = CloseHandle(h_process);
             return Err(anyhow::anyhow!("Failed to open process token"));
        }
        let _ = CloseHandle(h_process);

        let mut primary_token = HANDLE::default();
        let dup_res = DuplicateTokenEx(
            h_token,
            TOKEN_ALL_ACCESS,
            None,
            SecurityIdentification,
            TokenPrimary,
            &mut primary_token,
        );
        let _ = CloseHandle(h_token);
        
        dup_res.context("Failed to duplicate token")?;
        
        Ok((primary_token, is_winlogon))
    }
}

#[cfg(windows)]
pub fn run_agent_in_active_session(args: &[&str]) -> Result<String> {
    unsafe {
        // 1. Get Active Console Session ID
        let session_id = WTSGetActiveConsoleSessionId();
        if session_id == 0xFFFFFFFF {
            return Err(anyhow::anyhow!("No active console session found"));
        }

        // 2. Get User Token (via Stealing)
        let (primary_token, is_winlogon) = get_session_user_token(session_id)?;

        // 3. Create Pipes for Stdout
        let mut h_read = HANDLE::default();
        let mut h_write = HANDLE::default();
        let sa = SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: BOOL(1), // True
        };

        if CreatePipe(&mut h_read, &mut h_write, Some(&sa), 0).is_err() {
             let _ = CloseHandle(primary_token);
             return Err(anyhow::anyhow!("Failed to create pipe"));
        }

        // 4. Setup Startup Info
        let mut si: STARTUPINFOW = mem::zeroed();
        si.cb = mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdOutput = h_write;
        si.hStdError = h_write; 
        
        // Explicitly set desktop for WinLogon session
        let desktop_name = if is_winlogon {
            "winsta0\\winlogon"
        } else {
            "winsta0\\default"
        };
        let mut desktop_u16: Vec<u16> = desktop_name.encode_utf16().chain(std::iter::once(0)).collect();
        si.lpDesktop = PWSTR(desktop_u16.as_mut_ptr());

        let mut pi: PROCESS_INFORMATION = mem::zeroed();

        // 5. Build Command Line
        let exe_path = std::env::current_exe()?;
        // Use the same executable but with "agent" subcommand
        let mut cmd_line = format!("\"{}\" agent", exe_path.to_string_lossy());
        for arg in args {
            cmd_line.push_str(" ");
            // Quote argument if it contains spaces and not already quoted
            // But here args are simple strings like "list" or "set" or "internal", no spaces expected usually.
            // If ID contains colons, it's fine.
            cmd_line.push_str(arg);
        }
        
        let mut cmd_line_u16: Vec<u16> = cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

        // 6. Create Process As User
        let result = CreateProcessAsUserW(
            primary_token,
            None,
            PWSTR(cmd_line_u16.as_mut_ptr()),
            None,
            None,
            BOOL(1), // Inherit handles
            CREATE_NO_WINDOW | NORMAL_PRIORITY_CLASS, 
            None,
            None, // Current directory
            &si,
            &mut pi,
        );

        let _ = CloseHandle(h_write);
        let _ = CloseHandle(primary_token);

        if result.is_err() {
             let _ = CloseHandle(h_read);
             return Err(anyhow::anyhow!("CreateProcessAsUserW failed: {:?}", result));
        }

        let _ = CloseHandle(pi.hThread);

        // 7. Read Output
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut bytes_read = 0;

        loop {
            let success = ReadFile(
                h_read,
                Some(&mut buffer),
                Some(&mut bytes_read),
                None,
            );

            if success.is_err() || bytes_read == 0 {
                break;
            }
            output.extend_from_slice(&buffer[0..bytes_read as usize]);
        }

        WaitForSingleObject(pi.hProcess, INFINITE);
        let _ = CloseHandle(pi.hProcess);
        let _ = CloseHandle(h_read);

        let output_str = String::from_utf8_lossy(&output).to_string();
        Ok(output_str)
    }
}
