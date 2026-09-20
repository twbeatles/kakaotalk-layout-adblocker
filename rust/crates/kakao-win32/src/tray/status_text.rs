use super::state::StatusSnapshot;

/// Single-line summary for the notification-area tooltip (NIF_TIP, 127 wchars).
pub fn status_tooltip(snapshot: &StatusSnapshot) -> String {
    let mut text = format!(
        "KakaoTalk Layout AdBlocker\n차단 {} · 공격 {} · 메인윈도우 {}",
        on_off(snapshot.enabled),
        on_off(snapshot.aggressive),
        snapshot.main_windows
    );
    if snapshot.restore_failures > 0 {
        text.push_str(&format!("\n복원 실패 {}건", snapshot.restore_failures));
    }
    truncate_utf16(&text, 127)
}

/// Grayed header lines shown above the tray menu items.
pub fn status_menu_lines(snapshot: &StatusSnapshot) -> Vec<String> {
    let mut lines = vec![
        "KakaoTalk Layout AdBlocker".to_string(),
        format!(
            "차단 {} · 공격 모드 {} · 메인윈도우 {}",
            on_off(snapshot.enabled),
            on_off(snapshot.aggressive),
            snapshot.main_windows
        ),
        format!(
            "누적 숨김 {} · 누적 닫힘 {} · 누적 리사이즈 {}",
            snapshot.hidden_windows, snapshot.closed_windows, snapshot.resized_windows
        ),
    ];
    if snapshot.restore_failures > 0 {
        lines.push(format!(
            "복원 실패 {}건 (초기화 가능)",
            snapshot.restore_failures
        ));
    }
    let error = snapshot.last_error.trim();
    if !error.is_empty() {
        lines.push(format!("오류: {}", truncate_utf16(error, 60)));
    }
    lines
}

fn on_off(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

fn truncate_utf16(text: &str, max_units: usize) -> String {
    if text.encode_utf16().count() <= max_units {
        return text.to_string();
    }
    let mut out = String::new();
    let mut units = 0usize;
    for ch in text.chars() {
        let len = ch.len_utf16();
        if units + len > max_units {
            break;
        }
        units += len;
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_report_counters_and_failures() {
        let snapshot = StatusSnapshot {
            enabled: true,
            aggressive: false,
            main_windows: 2,
            hidden_windows: 7,
            closed_windows: 1,
            resized_windows: 12,
            restore_failures: 3,
            last_error: "restore show failed hwnd=42".into(),
        };
        let lines = status_menu_lines(&snapshot);
        assert!(lines.iter().any(|l| l.contains("차단 ON")));
        assert!(lines.iter().any(|l| l.contains("공격 모드 OFF")));
        assert!(lines.iter().any(|l| l.contains("메인윈도우 2")));
        assert!(lines.iter().any(|l| l.contains("누적 숨김 7")));
        assert!(lines.iter().any(|l| l.contains("누적 닫힘 1")));
        assert!(lines.iter().any(|l| l.contains("누적 리사이즈 12")));
        assert!(lines.iter().any(|l| l.contains("복원 실패 3건")));
        assert!(lines.iter().any(|l| l.contains("hwnd=42")));
    }

    #[test]
    fn status_lines_omit_failure_rows_when_clean() {
        let snapshot = StatusSnapshot {
            enabled: true,
            aggressive: true,
            main_windows: 1,
            ..StatusSnapshot::default()
        };
        let lines = status_menu_lines(&snapshot);
        assert!(!lines.iter().any(|l| l.contains("복원 실패")));
        assert!(!lines.iter().any(|l| l.starts_with("오류:")));
    }

    #[test]
    fn tooltip_fits_the_win32_limit() {
        let snapshot = StatusSnapshot {
            enabled: true,
            aggressive: true,
            main_windows: u32::MAX,
            restore_failures: u32::MAX,
            last_error: "x".repeat(500),
            ..StatusSnapshot::default()
        };
        let tip = status_tooltip(&snapshot);
        assert!(tip.encode_utf16().count() <= 127);
        assert!(tip.starts_with("KakaoTalk Layout AdBlocker"));
    }
}
