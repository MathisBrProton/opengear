use anyhow::Result;

#[derive(Debug, Clone)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

/// Parse a canonical Windows GUID string (`xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`, optional `{}`).
pub(crate) fn parse_guid_parts(input: &str) -> Result<(u32, u16, u16, [u8; 8])> {
    let s = input.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5
        || parts[0].len() != 8
        || parts[1].len() != 4
        || parts[2].len() != 4
        || parts[3].len() != 4
        || parts[4].len() != 12
        || !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        anyhow::bail!("GUID de mode d'alimentation invalide : {input:?}");
    }
    let d1 = u32::from_str_radix(parts[0], 16)?;
    let d2 = u16::from_str_radix(parts[1], 16)?;
    let d3 = u16::from_str_radix(parts[2], 16)?;
    let mut d4 = [0u8; 8];
    for i in 0..2 {
        d4[i] = u8::from_str_radix(&parts[3][i * 2..i * 2 + 2], 16)?;
    }
    for i in 0..6 {
        d4[2 + i] = u8::from_str_radix(&parts[4][i * 2..i * 2 + 2], 16)?;
    }
    Ok((d1, d2, d3, d4))
}

#[cfg(windows)]
mod win_power {
    use super::*;
    use windows::Win32::Foundation::{
        ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HLOCAL, LocalFree,
    };
    use windows::Win32::System::Power::{
        ACCESS_SCHEME, PowerEnumerate, PowerGetActiveScheme, PowerReadFriendlyName,
        PowerSetActiveScheme,
    };
    use windows::core::GUID;

    fn to_win_guid(s: &str) -> Result<GUID> {
        let (d1, d2, d3, d4) = parse_guid_parts(s)?;
        Ok(GUID::from_values(d1, d2, d3, d4))
    }

    fn format_win_guid(g: &GUID) -> String {
        format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            g.data1,
            g.data2,
            g.data3,
            g.data4[0],
            g.data4[1],
            g.data4[2],
            g.data4[3],
            g.data4[4],
            g.data4[5],
            g.data4[6],
            g.data4[7],
        )
    }

    fn read_friendly_name(scheme: &GUID) -> String {
        let mut size: u32 = 0;
        let rc = unsafe {
            PowerReadFriendlyName(None, Some(scheme), None, None, None, &mut size)
        };
        if (rc != ERROR_SUCCESS && rc != ERROR_MORE_DATA) || size == 0 {
            return format_win_guid(scheme);
        }
        let mut buf = vec![0u8; size as usize];
        let rc2 = unsafe {
            PowerReadFriendlyName(
                None,
                Some(scheme),
                None,
                None,
                Some(buf.as_mut_ptr()),
                &mut size,
            )
        };
        if rc2 != ERROR_SUCCESS {
            return format_win_guid(scheme);
        }
        let units: Vec<u16> = buf[..(size as usize).min(buf.len())]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        let name = String::from_utf16_lossy(&units).trim().to_string();
        if name.is_empty() {
            format_win_guid(scheme)
        } else {
            name
        }
    }

    pub fn list_power_plans() -> Vec<PowerPlan> {
        let mut active_ptr: *mut GUID = std::ptr::null_mut();
        let active_guid = unsafe {
            if PowerGetActiveScheme(None, &mut active_ptr) == ERROR_SUCCESS && !active_ptr.is_null()
            {
                let g = *active_ptr;
                let _ = LocalFree(Some(HLOCAL(active_ptr as *mut _)));
                Some(g)
            } else {
                None
            }
        };

        let mut plans = Vec::new();
        for idx in 0..64u32 {
            let mut g = GUID::zeroed();
            let mut sz = std::mem::size_of::<GUID>() as u32;
            let rc = unsafe {
                PowerEnumerate(
                    None,
                    None,
                    None,
                    ACCESS_SCHEME,
                    idx,
                    Some(&mut g as *mut GUID as *mut u8),
                    &mut sz,
                )
            };
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS {
                continue;
            }
            plans.push(PowerPlan {
                guid: format_win_guid(&g),
                name: read_friendly_name(&g),
                active: active_guid == Some(g),
            });
        }
        plans
    }

    pub fn set_active_power_plan(guid: &str) -> Result<()> {
        let g = to_win_guid(guid)?;
        let rc = unsafe { PowerSetActiveScheme(None, Some(&g)) };
        if rc != ERROR_SUCCESS {
            anyhow::bail!("PowerSetActiveScheme a échoué (code Win32 {})", rc.0);
        }
        Ok(())
    }
}

/// List Windows power plans via native `powrprof.dll` (works without admin rights, full Unicode).
pub fn list_power_plans() -> Vec<PowerPlan> {
    #[cfg(windows)]
    {
        win_power::list_power_plans()
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// Switch active Windows power plan (Silent / Balanced / High Performance / Bitsum).
pub fn set_active_power_plan(guid: &str) -> Result<()> {
    #[cfg(windows)]
    {
        win_power::set_active_power_plan(guid)
    }
    #[cfg(not(windows))]
    {
        let _ = parse_guid_parts(guid)?;
        anyhow::bail!("Non supporté hors Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::parse_guid_parts;

    #[test]
    fn valid_guid_parses_and_rejects_malformed() {
        let parsed = parse_guid_parts("{381b4222-f694-41f0-9685-ff5bb260df2e}").unwrap();
        assert_eq!(parsed.0, 0x381b4222);
        assert_eq!(parsed.1, 0xf694);
        assert_eq!(parsed.2, 0x41f0);
        assert!(parse_guid_parts("not-a-guid").is_err());
        assert!(parse_guid_parts("381b4222-f694-41f0-9685-ff5bb260df2g").is_err());
        assert!(parse_guid_parts("381b4222-f694-41f0-9685-ff5bb260df2e & calc").is_err());
    }
}
