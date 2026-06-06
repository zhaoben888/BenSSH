mod ai;
mod ai_ssh;
mod config;
mod manager;
mod sftp;
mod ssh;

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::{
    error::Error,
    io,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

const AI_TERMINAL_SCROLLBACK_ROWS: usize = 5000;

#[derive(PartialEq)]
enum AppMode {
    ServerList,
    FileBrowser,
    AiSsh,
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}

struct TerminalBuffer {
    parser: vt100::Parser,
    plain_log: String,
    application_cursor_mode: bool,
    mode_scan_tail: String,
    history_offset: usize,
    rows: u16,
    cols: u16,
}

impl TerminalBuffer {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, AI_TERMINAL_SCROLLBACK_ROWS),
            plain_log: String::new(),
            application_cursor_mode: false,
            mode_scan_tail: String::new(),
            history_offset: 0,
            rows,
            cols,
        }
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.rows = rows;
        self.cols = cols;
        self.parser.set_size(rows, cols);
    }

    fn clear(&mut self) {
        self.parser = vt100::Parser::new(32, 120, AI_TERMINAL_SCROLLBACK_ROWS);
        self.plain_log.clear();
        self.application_cursor_mode = false;
        self.mode_scan_tail.clear();
        self.history_offset = 0;
        self.rows = 32;
        self.cols = 120;
    }

    fn push_output(&mut self, text: &str) {
        self.update_cursor_key_mode(text);
        self.parser.process(text.as_bytes());
        append_terminal_text_to_log(&mut self.plain_log, text);
        let ends_with_newline = self.plain_log.ends_with('\n');
        let lines: Vec<&str> = self.plain_log.lines().rev().take(300).collect();
        self.plain_log = lines.into_iter().rev().collect::<Vec<_>>().join("\n");
        if ends_with_newline && !self.plain_log.is_empty() {
            self.plain_log.push('\n');
        }
    }

    fn display_text(&self, cursor_visible: bool) -> String {
        if self.history_offset > 0 {
            return history_display_text(&self.plain_log, self.rows as usize, self.history_offset);
        }

        terminal_display_text(
            &self.parser.screen().contents(),
            self.parser.screen().cursor_position(),
            cursor_visible,
        )
    }

    fn recent_context(&self) -> String {
        recent_context(&self.plain_log)
    }

    fn application_cursor_mode(&self) -> bool {
        self.application_cursor_mode
    }

    fn update_cursor_key_mode(&mut self, text: &str) {
        let scan_text = format!("{}{}", self.mode_scan_tail, text);
        self.mode_scan_tail = scan_text
            .chars()
            .rev()
            .take(16)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let enable = scan_text.rfind("\x1b[?1h");
        let disable = scan_text.rfind("\x1b[?1l");
        match (enable, disable) {
            (Some(enable_at), Some(disable_at)) => {
                self.application_cursor_mode = enable_at > disable_at;
            }
            (Some(_), None) => self.application_cursor_mode = true,
            (None, Some(_)) => self.application_cursor_mode = false,
            (None, None) => {}
        }
    }

    fn scrollback_offset(&self) -> usize {
        self.history_offset
    }

    fn scrollback_up(&mut self, rows: usize) {
        let max_offset = self.max_history_offset();
        self.history_offset = self.history_offset.saturating_add(rows).min(max_offset);
    }

    fn scrollback_down(&mut self, rows: usize) {
        self.history_offset = self.history_offset.saturating_sub(rows);
    }

    fn scrollback_top(&mut self) {
        self.history_offset = self.max_history_offset();
    }

    fn scrollback_bottom(&mut self) {
        self.history_offset = 0;
    }

    fn max_history_offset(&self) -> usize {
        self.plain_log
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count()
            .saturating_sub(self.rows as usize)
    }
}

fn sanitize_terminal_output(text: &str) -> String {
    let mut output = String::new();
    append_terminal_text_to_log(&mut output, text);
    output
}

fn append_terminal_text_to_log(output: &mut String, text: &str) {
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for code in chars.by_ref() {
                        if ('@'..='~').contains(&code) {
                            break;
                        }
                    }
                } else if chars.peek() == Some(&']') {
                    chars.next();
                    while let Some(code) = chars.next() {
                        if code == '\x07' {
                            break;
                        }
                        if code == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
            }
            '\r' => {}
            '\x08' | '\x7f' => {
                output.pop();
            }
            _ => output.push(ch),
        }
    }
}

fn recent_context(buffer: &str) -> String {
    buffer
        .lines()
        .rev()
        .take(80)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn history_display_text(buffer: &str, rows: usize, offset_from_bottom: usize) -> String {
    let lines = buffer
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if rows == 0 || lines.is_empty() {
        return String::new();
    }

    let max_offset = lines.len().saturating_sub(rows);
    let offset = offset_from_bottom.min(max_offset);
    let end = lines.len().saturating_sub(offset);
    let start = end.saturating_sub(rows);
    lines[start..end].join("\n")
}

fn terminal_display_text(
    output: &str,
    cursor_position: (u16, u16),
    cursor_visible: bool,
) -> String {
    if !cursor_visible {
        return output.to_string();
    }

    let (row, col) = cursor_position;
    let row = row as usize;
    let col = col as usize;
    let mut lines = output.split('\n').map(str::to_string).collect::<Vec<_>>();

    while lines.len() <= row {
        lines.push(String::new());
    }

    let line = &mut lines[row];
    while display_width(line) < col {
        line.push(' ');
    }

    let byte_idx = cell_col_to_byte_idx(line, col);
    if byte_idx < line.len() {
        let next_idx = line[byte_idx..]
            .chars()
            .next()
            .map(|ch| byte_idx + ch.len_utf8())
            .unwrap_or(byte_idx);
        line.replace_range(byte_idx..next_idx, "█");
    } else {
        line.push('█');
    }

    lines.join("\n")
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

fn cell_col_to_byte_idx(text: &str, target_col: usize) -> usize {
    let mut current_col = 0;
    for (idx, ch) in text.char_indices() {
        if current_col >= target_col {
            return idx;
        }
        current_col += UnicodeWidthChar::width(ch).unwrap_or(0);
    }

    text.len()
}

fn compact_advice_text(text: &str, expanded: bool) -> String {
    if expanded {
        return text.to_string();
    }

    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            line.starts_with("警告:")
                || line.starts_with("问题:")
                || line.starts_with("命令:")
                || line.starts_with("正在")
                || line.starts_with("模型分析失败:")
                || line.starts_with("未配置")
        })
        .take(3)
        .collect::<Vec<_>>()
        .join("\n")
}

fn shell_key_sequence(code: KeyCode, application_cursor_mode: bool) -> Option<&'static str> {
    match code {
        KeyCode::Up if application_cursor_mode => Some("\x1bOA"),
        KeyCode::Down if application_cursor_mode => Some("\x1bOB"),
        KeyCode::Right if application_cursor_mode => Some("\x1bOC"),
        KeyCode::Left if application_cursor_mode => Some("\x1bOD"),
        KeyCode::Home if application_cursor_mode => Some("\x1bOH"),
        KeyCode::End if application_cursor_mode => Some("\x1bOF"),
        KeyCode::Up => Some("\x1b[A"),
        KeyCode::Down => Some("\x1b[B"),
        KeyCode::Right => Some("\x1b[C"),
        KeyCode::Left => Some("\x1b[D"),
        KeyCode::Home => Some("\x1b[H"),
        KeyCode::End => Some("\x1b[F"),
        KeyCode::Delete => Some("\x1b[3~"),
        _ => None,
    }
}

fn should_quit_app(code: KeyCode, _modifiers: KeyModifiers, mode: &AppMode) -> bool {
    match mode {
        AppMode::AiSsh => false,
        _ => code == KeyCode::Char('q'),
    }
}

fn should_exit_ai_ssh(code: KeyCode, modifiers: KeyModifiers) -> bool {
    code == KeyCode::Char('q') && modifiers.contains(KeyModifiers::CONTROL)
}

fn should_interrupt_ai_ssh(code: KeyCode, modifiers: KeyModifiers) -> bool {
    let raw_ctrl_c = matches!(code, KeyCode::Char('\u{3}'));
    let modified_ctrl_c = matches!(code, KeyCode::Char(ch) if ch.eq_ignore_ascii_case(&'c'))
        && modifiers.contains(KeyModifiers::CONTROL);
    raw_ctrl_c || modified_ctrl_c
}

fn bracketed_paste_sequence(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    format!("\x1b[200~{}\x1b[201~", normalized)
}

fn control_char_for(c: char) -> Option<char> {
    let lower = c.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        return Some((lower as u8 - b'a' + 1) as char);
    }

    match c {
        '[' => Some('\x1b'),
        '\\' => Some('\x1c'),
        ']' => Some('\x1d'),
        '^' => Some('\x1e'),
        '_' => Some('\x1f'),
        '?' => Some('\x7f'),
        _ => None,
    }
}

fn analysis_fingerprint(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(240)
        .collect()
}

fn should_trigger_ai_analysis(
    new_output: &str,
    last_fingerprint: &str,
    elapsed_since_last: Duration,
) -> Option<String> {
    let cleaned = sanitize_terminal_output(new_output);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }

    let short_single_fragment = trimmed.len() < 8 && !cleaned.contains('\n');
    let complete_output_frame = cleaned.contains('\n') || trimmed.len() >= 40;
    if short_single_fragment || !complete_output_frame {
        return None;
    }

    let fingerprint = analysis_fingerprint(trimmed);
    if fingerprint == last_fingerprint {
        return None;
    }

    if elapsed_since_last < Duration::from_secs(20) {
        return None;
    }

    Some(fingerprint)
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test]
    fn sanitize_terminal_output_removes_ansi_escape_sequences() {
        let text = "\x1b[01;32mroot\x1b[0m@\x1b[01;34mhost\x1b[0m:~# ls\r\n";
        assert_eq!(sanitize_terminal_output(text), "root@host:~# ls\n");
    }

    #[test]
    fn sanitize_terminal_output_handles_backspace() {
        assert_eq!(sanitize_terminal_output("abc\x08d"), "abd");
    }

    #[test]
    fn sanitize_terminal_output_does_not_turn_carriage_return_into_newline() {
        assert_eq!(
            sanitize_terminal_output("root@TX:~# l\rs\r\n"),
            "root@TX:~# ls\n"
        );
    }

    #[test]
    fn push_output_preserves_partial_line_across_reads() {
        let mut buffer = TerminalBuffer::new(10, 40);
        buffer.push_output("l");
        buffer.push_output("s");
        assert!(buffer.recent_context().contains("ls"));
    }

    #[test]
    fn terminal_buffer_uses_remote_echo_as_screen_source() {
        let mut buffer = TerminalBuffer::new(10, 40);
        buffer.push_output("root@TX:~# ls\r\nfile.txt\r\nroot@TX:~# ");

        let context = buffer.recent_context();
        assert!(context.contains("root@TX:~# ls\nfile.txt"));
    }

    #[test]
    fn terminal_buffer_interprets_ansi_clear_line() {
        let mut buffer = TerminalBuffer::new(3, 20);
        buffer.push_output("Downloading 10%");
        buffer.push_output("\r\x1b[2KDone\n");
        let display = buffer.display_text(true);
        assert!(display.contains("Done"));
        assert!(!display.contains("Downloading 10%"));
    }

    #[test]
    fn terminal_display_text_adds_visible_cursor() {
        assert_eq!(
            terminal_display_text("root@TX:~# ", (0, 11), true),
            "root@TX:~# █"
        );
        assert_eq!(terminal_display_text("", (0, 0), true), "█");
        assert_eq!(
            terminal_display_text("root@TX:~# ", (0, 11), false),
            "root@TX:~# "
        );
    }

    #[test]
    fn terminal_display_text_uses_real_cursor_position() {
        assert_eq!(
            terminal_display_text("root@TX:~# reb", (0, 12), true),
            "root@TX:~# r█b"
        );
    }

    #[test]
    fn terminal_display_text_uses_cell_width_for_chinese_cursor() {
        let line = "# 2. 清理所有m存";
        let cursor_col = display_width(line);

        assert_eq!(
            terminal_display_text(line, (0, cursor_col as u16), true),
            "# 2. 清理所有m存█"
        );
    }

    #[test]
    fn cell_col_to_byte_idx_accounts_for_wide_chars() {
        let line = "# 2. 清理所有m存";
        assert_eq!(&line[cell_col_to_byte_idx(line, 13)..], "m存");
        assert_eq!(cell_col_to_byte_idx(line, display_width(line)), line.len());
    }

    #[test]
    fn ai_analysis_does_not_trigger_on_partial_typing() {
        assert_eq!(
            should_trigger_ai_analysis("reb", "", Duration::from_secs(60)),
            None
        );
    }

    #[test]
    fn ai_analysis_triggers_on_complete_remote_output() {
        let output = "README.md\nsrc\ntarget\n";
        assert!(should_trigger_ai_analysis(output, "", Duration::from_secs(60)).is_some());
    }

    #[test]
    fn ai_analysis_deduplicates_same_output_once() {
        let output = "README.md\nsrc\ntarget\n";
        let fingerprint = should_trigger_ai_analysis(output, "", Duration::from_secs(60)).unwrap();
        assert_eq!(
            should_trigger_ai_analysis(output, &fingerprint, Duration::from_secs(60)),
            None
        );
    }

    #[test]
    fn ai_analysis_respects_cooldown() {
        let output = "README.md\nsrc\ntarget\n";
        assert_eq!(
            should_trigger_ai_analysis(output, "", Duration::from_secs(5)),
            None
        );
    }

    #[test]
    fn compact_advice_text_hides_details_by_default() {
        let text = "警告: 高\n问题: 有安全更新\n命令: dnf upgrade-minimal --security\n来源: 大模型\n原因:\n- 很长";
        let compact = compact_advice_text(text, false);
        assert!(compact.contains("警告: 高"));
        assert!(compact.contains("问题: 有安全更新"));
        assert!(compact.contains("命令: dnf upgrade-minimal --security"));
        assert!(!compact.contains("来源:"));
        assert!(!compact.contains("原因:"));
        assert!(!compact.contains("[Tab]"));
    }

    #[test]
    fn shell_key_sequence_maps_arrows_for_remote_history() {
        assert_eq!(shell_key_sequence(KeyCode::Up, false), Some("\x1b[A"));
        assert_eq!(shell_key_sequence(KeyCode::Down, false), Some("\x1b[B"));
        assert_eq!(shell_key_sequence(KeyCode::Left, false), Some("\x1b[D"));
        assert_eq!(shell_key_sequence(KeyCode::Right, false), Some("\x1b[C"));
    }

    #[test]
    fn shell_key_sequence_uses_application_cursor_mode_for_vim() {
        assert_eq!(shell_key_sequence(KeyCode::Up, true), Some("\x1bOA"));
        assert_eq!(shell_key_sequence(KeyCode::Down, true), Some("\x1bOB"));
        assert_eq!(shell_key_sequence(KeyCode::Left, true), Some("\x1bOD"));
        assert_eq!(shell_key_sequence(KeyCode::Right, true), Some("\x1bOC"));
    }

    #[test]
    fn terminal_buffer_tracks_application_cursor_mode() {
        let mut buffer = TerminalBuffer::new(10, 40);
        assert!(!buffer.application_cursor_mode());

        buffer.push_output("\x1b[?1h");
        assert!(buffer.application_cursor_mode());

        buffer.push_output("\x1b[?1l");
        assert!(!buffer.application_cursor_mode());
    }

    #[test]
    fn terminal_buffer_tracks_split_application_cursor_sequence() {
        let mut buffer = TerminalBuffer::new(10, 40);
        buffer.push_output("\x1b[?");
        buffer.push_output("1h");
        assert!(buffer.application_cursor_mode());
    }

    #[test]
    fn terminal_buffer_can_scroll_back_to_previous_output() {
        let mut buffer = TerminalBuffer::new(3, 20);
        for i in 1..=8 {
            buffer.push_output(&format!("line{}\r\n", i));
        }

        buffer.scrollback_up(2);
        let historical = buffer.display_text(false);

        assert!(buffer.scrollback_offset() > 0);
        assert!(historical.contains("line4"));
        assert!(historical.contains("line5"));
    }

    #[test]
    fn terminal_buffer_scrollback_bottom_returns_to_live_screen() {
        let mut buffer = TerminalBuffer::new(3, 20);
        for i in 1..=8 {
            buffer.push_output(&format!("line{}\r\n", i));
        }
        let bottom = buffer.display_text(false);

        buffer.scrollback_up(2);
        buffer.scrollback_bottom();

        assert_eq!(buffer.scrollback_offset(), 0);
        assert_eq!(buffer.display_text(false), bottom);
    }

    #[test]
    fn history_display_text_slices_from_bottom_safely() {
        let history = "1\n2\n3\n4\n5\n6\n";
        assert_eq!(history_display_text(history, 3, 0), "4\n5\n6");
        assert_eq!(history_display_text(history, 3, 2), "2\n3\n4");
        assert_eq!(history_display_text(history, 3, 99), "1\n2\n3");
    }

    #[test]
    fn history_display_text_ignores_blank_padding_when_scrolled() {
        let history = "\n\nLast login\n[root@ALIYUN ~]# ps -ef\nUID PID\nroot 1\n\n\nroot 2\n";

        assert_eq!(
            history_display_text(history, 4, 3),
            "Last login\n[root@ALIYUN ~]# ps -ef\nUID PID\nroot 1"
        );
    }

    #[test]
    fn ai_ssh_keeps_letters_available_for_commands() {
        assert!(!should_exit_ai_ssh(
            KeyCode::Char('q'),
            KeyModifiers::empty()
        ));
        assert!(!should_exit_ai_ssh(KeyCode::Esc, KeyModifiers::empty()));
        assert!(should_exit_ai_ssh(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL
        ));
        assert!(!should_interrupt_ai_ssh(
            KeyCode::Char('c'),
            KeyModifiers::empty()
        ));
        assert!(should_interrupt_ai_ssh(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        ));
        assert!(should_interrupt_ai_ssh(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL
        ));
        assert!(should_interrupt_ai_ssh(
            KeyCode::Char('\u{3}'),
            KeyModifiers::empty()
        ));
        assert!(!should_quit_app(
            KeyCode::Char('q'),
            KeyModifiers::empty(),
            &AppMode::AiSsh
        ));
        assert!(!should_quit_app(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
            &AppMode::AiSsh
        ));
    }

    #[test]
    fn control_char_for_maps_shell_control_shortcuts() {
        assert_eq!(control_char_for('d'), Some('\x04'));
        assert_eq!(control_char_for('l'), Some('\x0c'));
        assert_eq!(control_char_for('['), Some('\x1b'));
        assert_eq!(control_char_for('?'), Some('\x7f'));
    }

    #[test]
    fn bracketed_paste_sequence_wraps_and_normalizes_pasted_text() {
        assert_eq!(
            bracketed_paste_sequence("a\r\nb\rc"),
            "\x1b[200~a\nb\nc\x1b[201~"
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut entries = config::load_config()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _ = execute!(stdout, EnableBracketedPaste);
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut list_state = ListState::default();
    if !entries.is_empty() {
        list_state.select(Some(0));
    }

    let mut app_mode = AppMode::ServerList;
    let mut remote_files: Vec<sftp::RemoteFile> = Vec::new();
    let mut file_list_state = ListState::default();
    let mut current_path = String::from("/root");
    let mut feedback_msg: Option<String> = None;
    let mut ai_session: Option<ai_ssh::AiSshSession> = None;
    let mut ai_pty_size: Option<(u16, u16)> = None;
    let mut ai_output = TerminalBuffer::new(32, 120);
    let mut ai_advice = "按 i 进入 AI SSH 后，我会在这里分析 Linux 错误输出。".to_string();
    let mut ai_node_name = String::new();
    let mut ai_user = String::new();
    let mut last_analysis_at = Instant::now() - Duration::from_secs(60);
    let mut last_analysis_fingerprint = String::new();
    let mut last_model_advice = String::new();
    let mut mouse_copy_mode = false;
    let cursor_started_at = Instant::now();
    let ai_config = ai::AiConfig::from_env();
    let (advice_tx, mut advice_rx) = tokio::sync::mpsc::unbounded_channel::<(String, String)>();

    loop {
        while let Ok((fingerprint, advice)) = advice_rx.try_recv() {
            if fingerprint == last_analysis_fingerprint {
                last_model_advice = advice.clone();
                ai_advice = advice;
            }
        }

        if app_mode == AppMode::AiSsh {
            if let Some(session) = ai_session.as_mut() {
                match session.read_available() {
                    Ok(output) if !output.is_empty() => {
                        ai_output.push_output(&output);
                        let trigger = should_trigger_ai_analysis(
                            &output,
                            &last_analysis_fingerprint,
                            last_analysis_at.elapsed(),
                        );

                        if let Some(fingerprint) = trigger {
                            let pending_fingerprint = fingerprint.clone();
                            let context = ai_output.recent_context();
                            last_analysis_at = Instant::now();
                            last_analysis_fingerprint = fingerprint;

                            if ai_config.api_key.is_some() {
                                ai_advice =
                                    "警告: 判断中\n问题: 正在请求大模型分析\n命令: 无".to_string();
                                let tx = advice_tx.clone();
                                let config = ai_config.clone();
                                let node_name = ai_node_name.clone();
                                let user = ai_user.clone();
                                let previous_advice = last_model_advice.clone();
                                tokio::spawn(async move {
                                    let text = match ai::analyze_with_model(
                                        &config,
                                        &node_name,
                                        &user,
                                        &context,
                                        &previous_advice,
                                    )
                                    .await
                                    {
                                        Ok(analysis) => analysis.to_panel_text(),
                                        Err(err) => {
                                            format!(
                                                "警告: 高\n问题: 大模型分析失败: {}\n命令: 无",
                                                err
                                            )
                                        }
                                    };
                                    let _ = tx.send((pending_fingerprint, text));
                                });
                            } else {
                                ai_advice =
                                    "警告: 高\n问题: 未配置 API Key，无法使用大模型判断\n命令: 无"
                                        .to_string();
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => {
                        ai_advice = format!("AI SSH 读取失败: {}", err);
                    }
                }
            }
        }

        if app_mode == AppMode::AiSsh {
            if let (Ok(size), Some(session)) = (terminal.size(), ai_session.as_mut()) {
                let rows = size.height.saturating_sub(12).max(1);
                let cols = size.width.saturating_sub(4).max(20);
                if ai_pty_size != Some((rows, cols)) {
                    let _ = session.resize(rows, cols);
                    ai_pty_size = Some((rows, cols));
                }
            }
        }

        terminal.draw(|f| {
            let size = f.size();

            if app_mode == AppMode::AiSsh {
                let advice_height = 7;
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints(
                        [
                            Constraint::Min(5),
                            Constraint::Length(advice_height),
                            Constraint::Length(1),
                        ]
                        .as_ref(),
                    )
                    .split(size);

                let copy_mode_title = if mouse_copy_mode { " | 复制模式" } else { "" };
                let terminal_title = if ai_output.scrollback_offset() > 0 {
                    format!(
                        " AI SSH: {}@{} | 历史 +{}{} ",
                        ai_user,
                        ai_node_name,
                        ai_output.scrollback_offset(),
                        copy_mode_title
                    )
                } else {
                    format!(" AI SSH: {}@{}{} ", ai_user, ai_node_name, copy_mode_title)
                };
                let terminal_rows = chunks[0].height.saturating_sub(2).max(1);
                let terminal_cols = chunks[0].width.saturating_sub(2).max(20);
                ai_output.resize(terminal_rows, terminal_cols);
                let cursor_visible = (cursor_started_at.elapsed().as_millis() / 500) % 2 == 0;
                let display_output = ai_output.display_text(cursor_visible);
                let terminal_panel = Paragraph::new(display_output)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().title(terminal_title).borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
                f.render_widget(terminal_panel, chunks[0]);

                let advice_title = " AI 告警 ";
                let advice_text = compact_advice_text(&ai_advice, false);
                let advice_panel = Paragraph::new(advice_text)
                    .wrap(Wrap { trim: true })
                    .block(Block::default().title(advice_title).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
                f.render_widget(advice_panel, chunks[1]);

                let help_bar = Paragraph::new(" AI SSH: [滚轮/PageUp]翻输出   [F2]复制模式   [Ctrl+C]中断   [Ctrl+Q]返回")
                    .style(Style::default().fg(Color::DarkGray));
                f.render_widget(help_bar, chunks[2]);

                if let Some(ref msg) = feedback_msg {
                    let area = centered_rect(60, 40, size);
                    let popup_block = Paragraph::new(format!("\n  {}\n\n  (按任意键关闭此弹窗)", msg))
                        .wrap(Wrap { trim: true })
                        .block(Block::default().title(" 🔔 系统通知 ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
                    f.render_widget(Clear, area);
                    f.render_widget(popup_block, area);
                }

                return;
            }

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(8), // 为装甲级巨型字体腾出空间
                    Constraint::Min(2),
                    Constraint::Length(1)
                ].as_ref())
                .split(size);

            // ==== 架构级美学：全实心装甲风格 (ANSI Shadow) ====
            // 这是极客圈最顶级的 3D 实心方块字，绝对够大且不空洞！
            let ascii_logo = "\
██████╗ ███████╗███╗   ██╗███████╗███████╗██╗  ██╗\n\
██╔══██╗██╔════╝████╗  ██║██╔════╝██╔════╝██║  ██║\n\
██████╔╝█████╗  ██╔██╗ ██║███████╗███████╗███████║\n\
██╔══██╗██╔══╝  ██║╚██╗██║╚════██║╚════██║██╔══██║\n\
██████╔╝███████╗██║ ╚████║███████║███████║██║  ██║\n\
╚═════╝ ╚══════╝╚═╝  ╚═══╝╚══════╝╚══════╝╚═╝  ╚═╝";

            let header = Paragraph::new(ascii_logo)
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
            f.render_widget(header, chunks[0]);

            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
                .split(chunks[1]);

            let (list_border_color, file_border_color) = if app_mode == AppMode::ServerList {
                (Color::Cyan, Color::DarkGray)
            } else {
                (Color::DarkGray, Color::Cyan)
            };

            let items: Vec<ListItem> = entries.iter().map(|e| {
                let sec_icon = if e.key_path.is_some() { "🔒" } else { "🔑" };
                ListItem::new(format!(" {} [{}] {}", sec_icon, e.name, e.host))
            }).collect();

            let list = List::new(items)
                .block(Block::default().title(" 🖥️ 节点列表 ").borders(Borders::ALL).border_style(Style::default().fg(list_border_color)))
                .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD))
                .highlight_symbol(">>");
            f.render_stateful_widget(list, main_chunks[0], &mut list_state);

            let file_items: Vec<ListItem> = remote_files.iter().map(|file| {
                let icon = if file.is_dir { "📁" } else { "📄" };
                let size_str = if file.is_dir { "".to_string() } else { format!("({} Bytes)", file.size) };
                ListItem::new(format!(" {} {} {}", icon, file.name, size_str))
            }).collect();

            let file_title = format!(" 📂 目录: {} ", current_path);
            let file_list = List::new(file_items)
                .block(Block::default().title(file_title).borders(Borders::ALL).border_style(Style::default().fg(file_border_color)))
                .highlight_style(Style::default().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD))
                .highlight_symbol(">>");
            f.render_stateful_widget(file_list, main_chunks[1], &mut file_list_state);

            let help_msg = if app_mode == AppMode::ServerList {
                " ⌨️ 快捷键映射:  [Enter]连接终端   [i]AI SSH   [f]文件管理   [a]新增   [e]编辑   [x]删除   [s]安全免密   [q]退出程序"
            } else {
                " ⌨️ 快捷键映射:  [Enter]进入目录   [d]下载此文件   [u]上传至当前目录   [b]返回服务器列表   [q]退出程序"
            };
            let help_bar = Paragraph::new(help_msg).style(Style::default().fg(Color::DarkGray));
            f.render_widget(help_bar, chunks[2]);

            if let Some(ref msg) = feedback_msg {
                let area = centered_rect(60, 40, size);
                let popup_block = Paragraph::new(format!("\n  {}\n\n  (按任意键关闭此弹窗)", msg))
                    .wrap(Wrap { trim: true })
                    .block(Block::default().title(" 🔔 系统通知 ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
                f.render_widget(Clear, area);
                f.render_widget(popup_block, area);
            }
        })?;

        if event::poll(std::time::Duration::from_millis(16))? {
            match event::read()? {
                Event::Paste(paste) => {
                    if feedback_msg.is_some() {
                        feedback_msg = None;
                        continue;
                    }

                    if app_mode == AppMode::AiSsh {
                        if let Some(session) = ai_session.as_mut() {
                            let _ = session.send_text(&bracketed_paste_sequence(&paste));
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if app_mode == AppMode::AiSsh && !mouse_copy_mode {
                        let wheel_rows = 3;

                        match mouse.kind {
                            MouseEventKind::ScrollUp => ai_output.scrollback_up(wheel_rows),
                            MouseEventKind::ScrollDown => ai_output.scrollback_down(wheel_rows),
                            _ => {}
                        }
                    }
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if should_quit_app(key.code, key.modifiers, &app_mode) {
                        break;
                    }

                    if feedback_msg.is_some() {
                        feedback_msg = None;
                        continue;
                    }

                    match app_mode {
                        AppMode::ServerList => match key.code {
                            KeyCode::Enter => {
                                if let Some(i) = list_state.selected() {
                                    let _ = ssh::launch_interactive_shell(&entries[i]);
                                }
                            }
                            KeyCode::Char('i') => {
                                if let Some(i) = list_state.selected() {
                                    if entries[i].key_path.is_none()
                                        && entries[i].password.is_some()
                                    {
                                        match manager::ensure_ssh_key(&mut entries, i) {
                                            Ok(_) => {}
                                            Err(e) => {
                                                feedback_msg = Some(format!(
                                                    "❌ AI SSH 自动免密配置失败: {}",
                                                    e
                                                ));
                                                continue;
                                            }
                                        }
                                    }
                                    let entry = entries[i].clone();
                                    ai_output.clear();
                                    ai_node_name = entry.name.clone();
                                    ai_user = entry.user.clone();
                                    last_model_advice.clear();
                                    match ai_ssh::AiSshSession::connect(&entry) {
                                        Ok(session) => {
                                            ai_session = Some(session);
                                            ai_pty_size = None;
                                            mouse_copy_mode = false;
                                            let _ = execute!(
                                                terminal.backend_mut(),
                                                EnableMouseCapture
                                            );
                                            app_mode = AppMode::AiSsh;
                                            ai_advice = if ai_config.api_key.is_some() {
                                                "AI SSH 已连接。检测到错误时会请求大模型给出告警和命令。".to_string()
                                            } else {
                                                "AI SSH 已连接。未配置 API Key，无法使用大模型分析。".to_string()
                                            };
                                        }
                                        Err(e) => {
                                            let err_text = format!("AI SSH 连接失败: {}", e);
                                            ai_advice =
                                                format!("警告: 高\n问题: {}\n命令: 无", err_text);
                                            let api_hint = if ai_config.api_key.is_some() {
                                                "API Key 已配置；本次失败发生在 SSH 登录前，尚未进入 AI 分析阶段。"
                                            } else {
                                                "未配置 API Key；这只影响大模型分析，不影响 SSH 登录。可在启动前设置 $env:DEEPSEEK_API_KEY=\"你的key\"。"
                                            };
                                            feedback_msg = Some(format!(
                                                "❌ {}\n\n{}\n\n{}",
                                                err_text, ai_advice, api_hint
                                            ));
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('f') => {
                                if let Some(i) = list_state.selected() {
                                    current_path = String::from("/root");
                                    match sftp::list_directory(&entries[i], &current_path) {
                                        Ok(files) => {
                                            remote_files = files;
                                            app_mode = AppMode::FileBrowser;
                                            if !remote_files.is_empty() {
                                                file_list_state.select(Some(0));
                                            }
                                        }
                                        Err(e) => {
                                            feedback_msg = Some(format!("❌ SFTP 连接失败: {}", e));
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('a')
                            | KeyCode::Char('e')
                            | KeyCode::Char('x')
                            | KeyCode::Char('s') => {
                                disable_raw_mode().unwrap();
                                let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
                                execute!(terminal.backend_mut(), LeaveAlternateScreen).unwrap();
                                terminal.show_cursor().unwrap();

                                match key.code {
                                    KeyCode::Char('a') => {
                                        if let Ok(_) = manager::add_vps(&mut entries) {
                                            feedback_msg =
                                                Some("✅ 新增节点成功！配置已固化到系统。".into());
                                            list_state.select(Some(entries.len() - 1));
                                        }
                                    }
                                    KeyCode::Char('e') => {
                                        if let Some(i) = list_state.selected() {
                                            if let Ok(_) = manager::edit_vps(&mut entries, i) {
                                                feedback_msg = Some("✅ 节点修改成功！".into());
                                            }
                                        }
                                    }
                                    KeyCode::Char('x') => {
                                        if let Some(i) = list_state.selected() {
                                            if let Ok(msg) = manager::delete_vps(&mut entries, i) {
                                                feedback_msg = Some(format!("ℹ️ {}", msg));
                                                if i >= entries.len() && i > 0 {
                                                    list_state.select(Some(i - 1));
                                                }
                                            }
                                        }
                                    }
                                    KeyCode::Char('s') => {
                                        if let Some(i) = list_state.selected() {
                                            match manager::setup_ssh_key(&mut entries, i) {
                                                    Ok(_) => feedback_msg = Some("✅ 证书下发成功！密码已抹除，该节点现在拥有最高安全级别。".into()),
                                                    Err(e) => feedback_msg = Some(format!("❌ 配置失败: {}", e)),
                                                }
                                        }
                                    }
                                    _ => {}
                                }

                                enable_raw_mode().unwrap();
                                execute!(terminal.backend_mut(), EnterAlternateScreen).unwrap();
                                let _ = execute!(terminal.backend_mut(), EnableBracketedPaste);
                                terminal.clear().unwrap();
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let i = match list_state.selected() {
                                    Some(i) => {
                                        if i >= entries.len().saturating_sub(1) {
                                            0
                                        } else {
                                            i + 1
                                        }
                                    }
                                    None => 0,
                                };
                                list_state.select(Some(i));
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                let i = match list_state.selected() {
                                    Some(i) => {
                                        if i == 0 {
                                            entries.len().saturating_sub(1)
                                        } else {
                                            i - 1
                                        }
                                    }
                                    None => 0,
                                };
                                list_state.select(Some(i));
                            }
                            _ => {}
                        },
                        AppMode::FileBrowser => match key.code {
                            KeyCode::Char('b') | KeyCode::Esc => {
                                app_mode = AppMode::ServerList;
                            }
                            KeyCode::Enter => {
                                if let Some(f_idx) = file_list_state.selected() {
                                    if let Some(s_idx) = list_state.selected() {
                                        let selected_file = &remote_files[f_idx];
                                        if selected_file.is_dir {
                                            if selected_file.name == ".." {
                                                let mut parts: Vec<&str> = current_path
                                                    .split('/')
                                                    .filter(|s| !s.is_empty())
                                                    .collect();
                                                if !parts.is_empty() {
                                                    parts.pop();
                                                }
                                                current_path = if parts.is_empty() {
                                                    String::from("/")
                                                } else {
                                                    format!("/{}", parts.join("/"))
                                                };
                                            } else {
                                                if current_path == "/" {
                                                    current_path =
                                                        format!("/{}", selected_file.name);
                                                } else {
                                                    current_path = format!(
                                                        "{}/{}",
                                                        current_path, selected_file.name
                                                    );
                                                }
                                            }
                                            if let Ok(files) =
                                                sftp::list_directory(&entries[s_idx], &current_path)
                                            {
                                                remote_files = files;
                                                if !remote_files.is_empty() {
                                                    file_list_state.select(Some(0));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('d') => {
                                if let Some(f_idx) = file_list_state.selected() {
                                    if let Some(s_idx) = list_state.selected() {
                                        let file = &remote_files[f_idx];
                                        if !file.is_dir {
                                            let remote_path = if current_path == "/" {
                                                format!("/{}", file.name)
                                            } else {
                                                format!("{}/{}", current_path, file.name)
                                            };

                                            if let Some(file_path) = rfd::FileDialog::new()
                                                .set_title("请选择下载保存的位置")
                                                .set_file_name(&file.name)
                                                .save_file()
                                            {
                                                let local_str = file_path.to_str().unwrap();
                                                if let Ok(_) = sftp::download_file(
                                                    &entries[s_idx],
                                                    &remote_path,
                                                    local_str,
                                                ) {
                                                    feedback_msg = Some(format!(
                                                        "✅ 下载成功！\n  文件已保存至：\n  {}",
                                                        local_str
                                                    ));
                                                } else {
                                                    feedback_msg = Some(
                                                        "❌ 下载失败！网络异常或无权限写入"
                                                            .to_string(),
                                                    );
                                                }
                                            }
                                        } else {
                                            feedback_msg = Some(
                                                "❌ 目前暂不支持打包下载整个文件夹".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('u') => {
                                if let Some(s_idx) = list_state.selected() {
                                    if let Some(file_path) = rfd::FileDialog::new()
                                        .set_title("请选择你要上传到远端的本地文件")
                                        .pick_file()
                                    {
                                        if let Ok(_) = sftp::upload_file(
                                            &entries[s_idx],
                                            file_path.to_str().unwrap(),
                                            &current_path,
                                        ) {
                                            feedback_msg = Some(
                                                "✅ 上传成功！\n  远端目录已刷新。".to_string(),
                                            );
                                            if let Ok(files) =
                                                sftp::list_directory(&entries[s_idx], &current_path)
                                            {
                                                remote_files = files;
                                            }
                                        } else {
                                            feedback_msg = Some(
                                                "❌ 上传失败！目标目录无写入权限或断网".to_string(),
                                            );
                                        }
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let i = match file_list_state.selected() {
                                    Some(i) => {
                                        if i >= remote_files.len().saturating_sub(1) {
                                            0
                                        } else {
                                            i + 1
                                        }
                                    }
                                    None => 0,
                                };
                                file_list_state.select(Some(i));
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                let i = match file_list_state.selected() {
                                    Some(i) => {
                                        if i == 0 {
                                            remote_files.len().saturating_sub(1)
                                        } else {
                                            i - 1
                                        }
                                    }
                                    None => 0,
                                };
                                file_list_state.select(Some(i));
                            }
                            _ => {}
                        },
                        AppMode::AiSsh => {
                            if let Some(session) = ai_session.as_mut() {
                                let terminal_rows_for_page = terminal
                                    .size()
                                    .map(|size| size.height.saturating_sub(12).max(1) as usize)
                                    .unwrap_or(20);

                                match key.code {
                                    KeyCode::F(2) => {
                                        mouse_copy_mode = !mouse_copy_mode;
                                        if mouse_copy_mode {
                                            let _ = execute!(
                                                terminal.backend_mut(),
                                                DisableMouseCapture
                                            );
                                        } else {
                                            let _ = execute!(
                                                terminal.backend_mut(),
                                                EnableMouseCapture
                                            );
                                        }
                                    }
                                    code if should_exit_ai_ssh(code, key.modifiers) => {
                                        session.close();
                                        ai_session = None;
                                        ai_pty_size = None;
                                        mouse_copy_mode = false;
                                        let _ =
                                            execute!(terminal.backend_mut(), DisableMouseCapture);
                                        app_mode = AppMode::ServerList;
                                    }
                                    KeyCode::PageUp => {
                                        ai_output.scrollback_up(terminal_rows_for_page);
                                    }
                                    KeyCode::PageDown => {
                                        ai_output.scrollback_down(terminal_rows_for_page);
                                    }
                                    KeyCode::Home
                                        if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                    {
                                        ai_output.scrollback_top();
                                    }
                                    KeyCode::End
                                        if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                    {
                                        ai_output.scrollback_bottom();
                                    }
                                    code if should_interrupt_ai_ssh(code, key.modifiers) => {
                                        ai_output.scrollback_bottom();
                                        let _ = session.send_interrupt();
                                    }
                                    KeyCode::Enter => {
                                        ai_output.scrollback_bottom();
                                        let _ = session.send_text("\n");
                                    }
                                    KeyCode::Backspace => {
                                        ai_output.scrollback_bottom();
                                        let _ = session.send_text("\x7f");
                                    }
                                    KeyCode::Tab => {
                                        ai_output.scrollback_bottom();
                                        let _ = session.send_text("\t");
                                    }
                                    KeyCode::Esc => {
                                        ai_output.scrollback_bottom();
                                        let _ = session.send_text("\x1b");
                                    }
                                    code @ (KeyCode::Up
                                    | KeyCode::Down
                                    | KeyCode::Right
                                    | KeyCode::Left
                                    | KeyCode::Home
                                    | KeyCode::End
                                    | KeyCode::Delete) => {
                                        if let Some(sequence) = shell_key_sequence(
                                            code,
                                            ai_output.application_cursor_mode(),
                                        ) {
                                            ai_output.scrollback_bottom();
                                            let _ = session.send_text(sequence);
                                        }
                                    }
                                    KeyCode::Char(ch) => {
                                        ai_output.scrollback_bottom();
                                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                                            if let Some(control) = control_char_for(ch) {
                                                let text = control.to_string();
                                                let _ = session.send_text(&text);
                                            }
                                        } else {
                                            let text = ch.to_string();
                                            let _ = session.send_text(&text);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
