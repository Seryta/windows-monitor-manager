use crate::{MonitorInfo, TopologyMode};

#[cfg(target_os = "windows")]
use windows::{
    Win32::Devices::Display::{
        SetDisplayConfig, SDC_APPLY, SDC_TOPOLOGY_INTERNAL, SDC_TOPOLOGY_EXTERNAL,
        SDC_TOPOLOGY_EXTEND, SDC_TOPOLOGY_CLONE, SDC_USE_SUPPLIED_DISPLAY_CONFIG,
        GetDisplayConfigBufferSizes, QueryDisplayConfig, DisplayConfigGetDeviceInfo,
        DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_MODE_INFO,
        DISPLAYCONFIG_TARGET_DEVICE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
        QDC_ALL_PATHS,
    },
};
#[cfg(target_os = "windows")]
use std::mem;

const DISPLAYCONFIG_PATH_ACTIVE: u32 = 0x00000001;

pub fn list_monitors() -> std::result::Result<Vec<MonitorInfo>, String> {
    #[cfg(target_os = "windows")]
    {
        // Use QueryDisplayConfig (CCD) for enumeration to match SetDisplayConfig requirements
        let mut num_path_elements = 0;
        let mut num_mode_elements = 0;
        let _ = unsafe { GetDisplayConfigBufferSizes(QDC_ALL_PATHS, &mut num_path_elements, &mut num_mode_elements) };
        
        let mut path_info_array = vec![unsafe { mem::zeroed::<DISPLAYCONFIG_PATH_INFO>() }; num_path_elements as usize];
        let mut mode_info_array = vec![unsafe { mem::zeroed::<DISPLAYCONFIG_MODE_INFO>() }; num_mode_elements as usize];
        
        let _ = unsafe {
             QueryDisplayConfig(
                QDC_ALL_PATHS,
                &mut num_path_elements,
                path_info_array.as_mut_ptr(),
                &mut num_mode_elements,
                mode_info_array.as_mut_ptr(),
                None,
            )
        };

        let mut monitors: Vec<MonitorInfo> = Vec::new();
        let mut seen_targets = std::collections::HashSet::new();

        for i in 0..num_path_elements as usize {
            let path = path_info_array[i];
            let adapter_id_low = path.targetInfo.adapterId.LowPart;
            let adapter_id_high = path.targetInfo.adapterId.HighPart as u32; // Assuming high part fits or cast safe
            let target_id = path.targetInfo.id;
            
            // Deduplicate by target
            let key = (adapter_id_low, adapter_id_high, target_id);
            if seen_targets.contains(&key) {
                continue;
            }
            seen_targets.insert(key);

            // Get Friendly Name
            let mut name = format!("Monitor {}", monitors.len() + 1);
            let mut device_string = "Generic Monitor".to_string();

            let mut target_name = unsafe { mem::zeroed::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() };
            target_name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target_name.header.size = mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target_name.header.adapterId = path.targetInfo.adapterId;
            target_name.header.id = path.targetInfo.id;

            if unsafe { DisplayConfigGetDeviceInfo(&mut target_name.header as *mut _ as *mut _) } == 0 {
                 let raw_name = String::from_utf16_lossy(&target_name.monitorFriendlyDeviceName)
                    .trim_matches(char::from(0))
                    .to_string();
                 if !raw_name.is_empty() {
                     name = raw_name.clone();
                     device_string = raw_name;
                 }
            }

            // Filter out "Generic Monitor" if user wants to hide them
            // Or better: filter out monitors that don't have a valid friendly name AND aren't active?
            // User says: "Q27G2SG4B+/HD TO USB" are valid. "Generic Monitor" are not.
            // Let's filter out if name is "Generic Monitor" (unless it is active, maybe?)
            
            // Actually, many real monitors might report as Generic PnP Monitor if drivers are missing.
            // But the user specifically sees "Generic Monitor" (our default fallback) for phantom displays.
            // Let's check if we successfully got a name.
            // If device_string is still "Generic Monitor" AND it is NOT active, we can probably skip it.
            
            let is_active = (path.flags & DISPLAYCONFIG_PATH_ACTIVE) != 0;

            if device_string == "Generic Monitor" && !is_active {
                continue;
            }
            
            // Also filter if name is just "Monitor N" and not active?
            // Let's rely on the fact that real monitors usually return a name from DisplayConfigGetDeviceInfo.
            
            // To determine primary/attached, we'd need more logic, simplified here:
            let is_primary = path.sourceInfo.id == 0; // Simplified assumption
            let is_attached = true; // If it's in QDC_ALL_PATHS it's attached

            monitors.push(MonitorInfo {
                id: monitors.len() as u32,
                name,
                device_string,
                is_active,
                is_attached,
                is_primary,
                adapter_id_low,
                adapter_id_high,
                target_id,
            });
        }

        Ok(monitors)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(vec![])
    }
}

pub fn set_topology(mode: TopologyMode) -> std::result::Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        match mode {
            TopologyMode::Single(id_str) => {
                // Parse "low:high:id"
                let parts: Vec<&str> = id_str.split(':').collect();
                if parts.len() != 3 {
                    return Err("Invalid ID format".to_string());
                }
                let adapter_low = parts[0].parse::<u32>().map_err(|_| "Invalid adapter low")?;
                let adapter_high = parts[1].parse::<i32>().map_err(|_| "Invalid adapter high")?; // HighPart is i32 in windows-rs usually
                let target_id = parts[2].parse::<u32>().map_err(|_| "Invalid target id")?;

                enable_single_monitor_by_id(adapter_low, adapter_high, target_id)
            }
            _ => {
                // Standard modes
                let flags = match mode {
                    TopologyMode::Internal => SDC_TOPOLOGY_INTERNAL,
                    TopologyMode::External => SDC_TOPOLOGY_EXTERNAL,
                    TopologyMode::Extend => SDC_TOPOLOGY_EXTEND,
                    TopologyMode::Clone => SDC_TOPOLOGY_CLONE,
                    _ => SDC_TOPOLOGY_INTERNAL,
                };

                unsafe {
                    let ret = SetDisplayConfig(None, None, SDC_APPLY | flags);
                    if ret == 0 {
                        Ok(())
                    } else {
                        Err(format!("SetDisplayConfig failed with error code: {}", ret))
                    }
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        println!("Mock setting topology to {:?}", mode);
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn enable_single_monitor_by_id(adapter_low: u32, adapter_high: i32, target_id: u32) -> std::result::Result<(), String> {
    unsafe {
        // 1. Get Buffer Sizes
        let mut num_path_elements = 0;
        let mut num_mode_elements = 0;
        let _ = GetDisplayConfigBufferSizes(QDC_ALL_PATHS, &mut num_path_elements, &mut num_mode_elements);

        // 2. Query All Paths
        let mut path_info_array = vec![mem::zeroed::<DISPLAYCONFIG_PATH_INFO>(); num_path_elements as usize];
        let mut mode_info_array = vec![mem::zeroed::<DISPLAYCONFIG_MODE_INFO>(); num_mode_elements as usize];

        let ret = QueryDisplayConfig(
            QDC_ALL_PATHS,
            &mut num_path_elements,
            path_info_array.as_mut_ptr(),
            &mut num_mode_elements,
            mode_info_array.as_mut_ptr(),
            None,
        );
        if ret.is_err() {
            return Err(format!("QueryDisplayConfig failed: {:?}", ret));
        }

        // 3. Find matching path
        let mut found_path_idx = None;
        for i in 0..num_path_elements as usize {
            let path = &path_info_array[i];
            if path.targetInfo.adapterId.LowPart == adapter_low && 
               path.targetInfo.adapterId.HighPart == adapter_high &&
               path.targetInfo.id == target_id {
                found_path_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = found_path_idx {
            let mut new_path = path_info_array[idx];
            new_path.flags |= DISPLAYCONFIG_PATH_ACTIVE;
            let new_paths = vec![new_path];
            
            let ret = SetDisplayConfig(
                Some(&new_paths),
                Some(&mode_info_array), 
                SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG,
            );
            if ret == 0 {
                Ok(())
            } else {
                Err(format!("SetDisplayConfig (Single) failed: {}", ret))
            }
        } else {
            Err("Target monitor path not found".to_string())
        }
    }
}
