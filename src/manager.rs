use crate::config::{self, VpsEntry};
use dialoguer::{Confirm, Input, Password};
use ssh2::Session;
use std::net::TcpStream;

pub fn add_vps(entries: &mut Vec<VpsEntry>) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== ➕ 添加新的 VPS 节点 ===");
    println!("💡 [操作指引] 在任何步骤中，输入 `!q` 并回车即可随时取消并返回主界面。");

    let name: String = Input::new().with_prompt("VPS 别名 (如 阿里云)").interact_text()?;
    if name == "!q" { return Err("操作已取消".into()); }

    let host: String = Input::new().with_prompt("IP 地址").interact_text()?;
    if host == "!q" { return Err("操作已取消".into()); }

    let port_str: String = Input::new().with_prompt("SSH 端口").default("22".to_string()).interact_text()?;
    if port_str == "!q" { return Err("操作已取消".into()); }
    let port = port_str.parse::<u16>().unwrap_or(22);

    let user: String = Input::new().with_prompt("登录用户名").default("root".to_string()).interact_text()?;
    if user == "!q" { return Err("操作已取消".into()); }
    
    let use_pwd = Confirm::new().with_prompt("是否使用密码登录？(选 No 则配置私钥)").default(true).interact()?;
    
    let (password, key_path) = if use_pwd {
        let pwd = Password::new().with_prompt("登录密码 (输入时不可见)").interact()?;
        if pwd == "!q" { return Err("操作已取消".into()); }
        (Some(pwd), None)
    } else {
        let default_key = dirs::home_dir().unwrap().join(".ssh/benssh_rsa").to_string_lossy().to_string();
        let k_path: String = Input::new().with_prompt("私钥完整路径").default(default_key).interact_text()?;
        if k_path == "!q" { return Err("操作已取消".into()); }
        (None, Some(k_path))
    };

    entries.push(VpsEntry { name, host, port, user, password, key_path, setup: None });
    config::save_config(entries)?;
    Ok(())
}

pub fn edit_vps(entries: &mut Vec<VpsEntry>, idx: usize) -> Result<(), Box<dyn std::error::Error>> {
    let entry = &entries[idx];
    println!("\n=== ✏️ 编辑节点: {} ===", entry.name);
    println!("💡 [操作指引] 直接回车表示保留原值；输入 `!q` 可以随时抛弃修改并返回主界面。");

    // 为防止改到一半退出导致脏数据，我们先用临时变量接收
    let name: String = Input::new().with_prompt("VPS 别名").default(entry.name.clone()).interact_text()?;
    if name == "!q" { return Err("编辑已取消，没有任何内容被修改".into()); }

    let host: String = Input::new().with_prompt("IP 地址").default(entry.host.clone()).interact_text()?;
    if host == "!q" { return Err("编辑已取消，没有任何内容被修改".into()); }

    let port_str: String = Input::new().with_prompt("SSH 端口").default(entry.port.to_string()).interact_text()?;
    if port_str == "!q" { return Err("编辑已取消，没有任何内容被修改".into()); }
    let port = port_str.parse::<u16>().unwrap_or(entry.port);

    let user: String = Input::new().with_prompt("登录用户名").default(entry.user.clone()).interact_text()?;
    if user == "!q" { return Err("编辑已取消，没有任何内容被修改".into()); }
    
    let mut new_pwd = entry.password.clone();
    let mut new_key = entry.key_path.clone();

    if Confirm::new().with_prompt("是否需要重新设置密码？").default(false).interact()? {
        let p = Password::new().with_prompt("新密码 (输入时不可见)").interact()?;
        if p == "!q" { return Err("编辑已取消，没有任何内容被修改".into()); }
        new_pwd = Some(p);
        new_key = None; // 换密码就清空秘钥，保证安全隔离
    }

    // 所有数据验证通过后，最后一步落盘
    let target = &mut entries[idx];
    target.name = name;
    target.host = host;
    target.port = port;
    target.user = user;
    target.password = new_pwd;
    target.key_path = new_key;

    config::save_config(entries)?;
    Ok(())
}

pub fn delete_vps(entries: &mut Vec<VpsEntry>, idx: usize) -> Result<String, Box<dyn std::error::Error>> {
    let name = entries[idx].name.clone();
    if Confirm::new().with_prompt(format!("🚨 确定要永久删除节点 [{}] 吗？", name)).default(false).interact()? {
        entries.remove(idx);
        config::save_config(entries)?;
        Ok(format!("节点 [{}] 已被彻底删除", name))
    } else {
        Ok("已取消删除操作".to_string())
    }
}

pub fn setup_ssh_key(entries: &mut Vec<VpsEntry>, idx: usize) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== 🔑 开始为节点配置底层免密证书登录 ===");
    let entry = &mut entries[idx];

    if entry.password.is_none() {
        return Err("必须配置了初始密码的节点才能下发公钥！".into());
    }

    let home = dirs::home_dir().ok_or("找不到 Home 目录")?;
    let pub_key_path = home.join(".ssh/benssh_rsa.pub");
    let priv_key_path = home.join(".ssh/benssh_rsa");

    if !pub_key_path.exists() {
        println!("正在你的电脑上自动生成高强度 RSA 2048 密钥对...");
        std::fs::create_dir_all(home.join(".ssh"))?;
        std::process::Command::new("ssh-keygen")
            .args(["-t", "rsa", "-b", "2048", "-m", "PEM", "-N", "", "-f", priv_key_path.to_str().unwrap()])
            .output()?;
        println!("✅ 密钥对生成成功！");
    }

    let pub_key_content = std::fs::read_to_string(&pub_key_path)?;

    println!("正在利用原始密码建立加密通道...");
    let tcp = TcpStream::connect(format!("{}:{}", entry.host, entry.port))?;
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;
    sess.userauth_password(&entry.user, entry.password.as_ref().unwrap())?;

    if !sess.authenticated() { return Err("凭证无效，登入目标机器失败".into()); }

    println!("正在向目标 Linux 内核注入公钥指纹...");
    let mut channel = sess.channel_session()?;
    let cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
        pub_key_content.trim()
    );
    channel.exec(&cmd)?;
    channel.wait_close()?;

    entry.password = None;
    entry.key_path = Some(priv_key_path.to_string_lossy().to_string());
    config::save_config(entries)?;
    
    println!("✅ 免密配置下发并注入成功！安全级别已升级。请按任意键返回...");
    std::io::stdin().read_line(&mut String::new())?;

    Ok(())
}
