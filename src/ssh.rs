use crate::config::VpsEntry;
use std::process::Command;

/// 终极体验：在新标签页（Tab）中启动 SSH 会话
/// 利用 Windows 11 自带的 Windows Terminal (wt.exe) 特性
pub fn launch_interactive_shell(entry: &VpsEntry) -> Result<(), Box<dyn std::error::Error>> {
    let target = format!("{}@{}", entry.user, entry.host);
    let port = entry.port.to_string();

    // 呼出 Windows Terminal (wt.exe)
    let mut cmd = Command::new("wt");
    
    // -w 0: 代表强制在“当前”你正在用的这个终端窗口中操作，而不是弹出一个全新的软件窗口
    cmd.arg("-w").arg("0");
    
    // new-tab: 新建标签页
    cmd.arg("new-tab");
    
    // --title: 把新建出来的标签页名字，改成你配置的 VPS 名称（极度优雅）
    cmd.arg("--title").arg(&entry.name);
    // ⚠️ 核心修复：强制锁定标题，禁止远端 Linux 的 Bash/Zsh 篡改标签名
    cmd.arg("--suppressApplicationTitle");
    
    // 接下来的所有参数，就是让那个新标签去执行真正的 ssh 命令
    cmd.arg("ssh");
    cmd.arg("-p").arg(&port);

    if let Some(ref key_path) = entry.key_path {
        cmd.arg("-i").arg(key_path);
    }
    
    cmd.arg(&target);

    // 重点：我们只负责“触发”，不需要 wait()。
    // 这意味着弹出一个新标签后，Rust 的主界面完全不卡顿，你可以立刻继续选中下一台机器继续敲回车！
    cmd.spawn()?;

    Ok(())
}
