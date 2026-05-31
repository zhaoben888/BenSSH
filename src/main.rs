mod config;
mod ssh; 
mod sftp;
mod manager;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
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
use std::{error::Error, io};

#[derive(PartialEq)]
enum AppMode {
    ServerList,
    FileBrowser,
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage((100 - percent_y) / 2), Constraint::Percentage(percent_y), Constraint::Percentage((100 - percent_y) / 2)].as_ref())
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage((100 - percent_x) / 2), Constraint::Percentage(percent_x), Constraint::Percentage((100 - percent_x) / 2)].as_ref())
        .split(popup_layout[1])[1]
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut entries = config::load_config()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut list_state = ListState::default();
    if !entries.is_empty() { list_state.select(Some(0)); }

    let mut app_mode = AppMode::ServerList;
    let mut remote_files: Vec<sftp::RemoteFile> = Vec::new();
    let mut file_list_state = ListState::default();
    let mut current_path = String::from("/root");
    let mut feedback_msg: Option<String> = None;

    loop {
        terminal.draw(|f| {
            let size = f.size();
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
██████╗ ███████╗███╗   ██╗███████╗██╗  ██╗██╗  ██╗\n\
██╔══██╗██╔════╝████╗  ██║██╔════╝██║  ██║██║  ██║\n\
██████╔╝█████╗  ██╔██╗ ██║███████╗███████║███████║\n\
██╔══██╗██╔══╝  ██║╚██╗██║╚════██║██╔══██║██╔══██║\n\
██████╔╝███████╗██║ ╚████║███████║██║  ██║██║  ██║\n\
╚═════╝ ╚══════╝╚═╝  ╚═══╝╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝";
            
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
                " ⌨️ 快捷键映射:  [Enter]连接终端   [f]文件管理   [a]新增   [e]编辑   [x]删除   [s]安全免密   [q]退出程序"
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

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if key.code == KeyCode::Char('q') { break; }
                    
                    if feedback_msg.is_some() {
                        feedback_msg = None;
                        continue;
                    }

                    match app_mode {
                        AppMode::ServerList => {
                            match key.code {
                                KeyCode::Enter => {
                                    if let Some(i) = list_state.selected() {
                                        let _ = ssh::launch_interactive_shell(&entries[i]);
                                    }
                                }
                                KeyCode::Char('f') => {
                                    if let Some(i) = list_state.selected() {
                                        current_path = String::from("/root");
                                        match sftp::list_directory(&entries[i], &current_path) {
                                            Ok(files) => {
                                                remote_files = files;
                                                app_mode = AppMode::FileBrowser;
                                                if !remote_files.is_empty() { file_list_state.select(Some(0)); }
                                            }
                                            Err(e) => {
                                                feedback_msg = Some(format!("❌ SFTP 连接失败: {}", e));
                                            }
                                        }
                                    }
                                }
                                KeyCode::Char('a') | KeyCode::Char('e') | KeyCode::Char('x') | KeyCode::Char('s') => {
                                    disable_raw_mode().unwrap(); execute!(terminal.backend_mut(), LeaveAlternateScreen).unwrap(); terminal.show_cursor().unwrap();
                                    
                                    match key.code {
                                        KeyCode::Char('a') => {
                                            if let Ok(_) = manager::add_vps(&mut entries) {
                                                feedback_msg = Some("✅ 新增节点成功！配置已固化到系统。".into());
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
                                                    if i >= entries.len() && i > 0 { list_state.select(Some(i - 1)); }
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

                                    enable_raw_mode().unwrap(); execute!(terminal.backend_mut(), EnterAlternateScreen).unwrap(); terminal.clear().unwrap();
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let i = match list_state.selected() {
                                        Some(i) => if i >= entries.len().saturating_sub(1) { 0 } else { i + 1 },
                                        None => 0,
                                    };
                                    list_state.select(Some(i));
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let i = match list_state.selected() {
                                        Some(i) => if i == 0 { entries.len().saturating_sub(1) } else { i - 1 },
                                        None => 0,
                                    };
                                    list_state.select(Some(i));
                                }
                                _ => {}
                            }
                        }
                        AppMode::FileBrowser => {
                            match key.code {
                                KeyCode::Char('b') | KeyCode::Esc => { app_mode = AppMode::ServerList; }
                                KeyCode::Enter => {
                                    if let Some(f_idx) = file_list_state.selected() {
                                        if let Some(s_idx) = list_state.selected() {
                                            let selected_file = &remote_files[f_idx];
                                            if selected_file.is_dir {
                                                if selected_file.name == ".." {
                                                    let mut parts: Vec<&str> = current_path.split('/').filter(|s| !s.is_empty()).collect();
                                                    if !parts.is_empty() { parts.pop(); }
                                                    current_path = if parts.is_empty() { String::from("/") } else { format!("/{}", parts.join("/")) };
                                                } else {
                                                    if current_path == "/" {
                                                        current_path = format!("/{}", selected_file.name);
                                                    } else {
                                                        current_path = format!("{}/{}", current_path, selected_file.name);
                                                    }
                                                }
                                                if let Ok(files) = sftp::list_directory(&entries[s_idx], &current_path) {
                                                    remote_files = files;
                                                    if !remote_files.is_empty() { file_list_state.select(Some(0)); }
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
                                                let remote_path = if current_path == "/" { format!("/{}", file.name) } else { format!("{}/{}", current_path, file.name) };
                                                
                                                if let Some(file_path) = rfd::FileDialog::new()
                                                    .set_title("请选择下载保存的位置")
                                                    .set_file_name(&file.name)
                                                    .save_file() 
                                                {
                                                    let local_str = file_path.to_str().unwrap();
                                                    if let Ok(_) = sftp::download_file(&entries[s_idx], &remote_path, local_str) {
                                                        feedback_msg = Some(format!("✅ 下载成功！\n  文件已保存至：\n  {}", local_str));
                                                    } else {
                                                        feedback_msg = Some("❌ 下载失败！网络异常或无权限写入".to_string());
                                                    }
                                                }
                                            } else {
                                                feedback_msg = Some("❌ 目前暂不支持打包下载整个文件夹".to_string());
                                            }
                                        }
                                    }
                                }
                                KeyCode::Char('u') => {
                                    if let Some(s_idx) = list_state.selected() {
                                        if let Some(file_path) = rfd::FileDialog::new().set_title("请选择你要上传到远端的本地文件").pick_file() {
                                            if let Ok(_) = sftp::upload_file(&entries[s_idx], file_path.to_str().unwrap(), &current_path) {
                                                feedback_msg = Some("✅ 上传成功！\n  远端目录已刷新。".to_string());
                                                if let Ok(files) = sftp::list_directory(&entries[s_idx], &current_path) {
                                                    remote_files = files;
                                                }
                                            } else {
                                                feedback_msg = Some("❌ 上传失败！目标目录无写入权限或断网".to_string());
                                            }
                                        }
                                    }
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let i = match file_list_state.selected() {
                                        Some(i) => if i >= remote_files.len().saturating_sub(1) { 0 } else { i + 1 },
                                        None => 0,
                                    };
                                    file_list_state.select(Some(i));
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let i = match file_list_state.selected() {
                                        Some(i) => if i == 0 { remote_files.len().saturating_sub(1) } else { i - 1 },
                                        None => 0,
                                    };
                                    file_list_state.select(Some(i));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
