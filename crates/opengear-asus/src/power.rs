use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

/// List Windows power plans via `powercfg /list` (works without admin rights).
pub fn list_power_plans() -> Vec<PowerPlan> {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("powercfg")
            .arg("/list")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };
    #[cfg(not(windows))]
    let output = Command::new("powercfg").arg("/list").output();

    let Ok(out) = output else {
        return Vec::new();
    };
    let text = decode_oem_cp850(&out.stdout);
    let mut plans = Vec::new();

    for line in text.lines() {
        // Match lines containing a GUID like 381b4222-f694-41f0-9685-ff5bb260df2e  (Name) *
        if let Some(colon) = line.find(':') {
            let rest = line[colon + 1..].trim();
            let mut parts = rest.splitn(2, char::is_whitespace);
            if let (Some(guid), Some(tail)) = (parts.next(), parts.next()) {
                if guid.len() == 36 && guid.chars().filter(|&c| c == '-').count() == 4 {
                    let active = tail.trim_end().ends_with('*');
                    let name = tail
                        .trim()
                        .trim_end_matches('*')
                        .trim()
                        .trim_start_matches('(')
                        .trim_end_matches(')')
                        .to_string();
                    plans.push(PowerPlan {
                        guid: guid.to_string(),
                        name,
                        active,
                    });
                }
            }
        }
    }
    plans
}

/// Switch active Windows power plan (Silent / Balanced / High Performance / Bitsum).
pub fn set_active_power_plan(guid: &str) -> Result<()> {
    #[cfg(windows)]
    let status = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("powercfg")
            .args(["/setactive", guid])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .context("Impossible d'exécuter powercfg /setactive")?
    };
    #[cfg(not(windows))]
    let status = Command::new("powercfg")
        .args(["/setactive", guid])
        .status()?;

    if !status.success() {
        anyhow::bail!("powercfg /setactive a échoué (code {:?})", status.code());
    }
    Ok(())
}

/// Decode common French Windows OEM CP850 bytes from `powercfg` output into clean UTF-8.
fn decode_oem_cp850(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        let ch = match b {
            0x00..=0x7F => b as char,
            0x82 => 'é',
            0x85 => 'à',
            0x87 => 'ç',
            0x88 => 'ê',
            0x8A => 'è',
            0x90 => 'É',
            0x93 => 'ô',
            0x96 => 'û',
            0x97 => 'ù',
            0xA0 => 'á',
            0xB5 => 'Á',
            0xB7 => 'À',
            0xD4 => 'È',
            _ => ' ',
        };
        out.push(ch);
    }
    out
}
