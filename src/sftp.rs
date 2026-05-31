use crate::config::VpsEntry;
use ssh2::Session;
use std::net::TcpStream;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

// ==== 架构级重构：统一凭证拦截器 ====
// 支持密码和秘钥双模认证，解决打包前可能遇到的免密节点 SFTP 连接失败 Bug
fn authenticate_session(sess: &mut Session, entry: &VpsEntry) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(ref key_path) = entry.key_path {
        let pub_path = format!("{}.pub", key_path);
        let pubkey = if Path::new(&pub_path).exists() { Some(Path::new(&pub_path)) } else { None };
        sess.userauth_pubkey_file(&entry.user, pubkey, Path::new(key_path), None)?;
    } else if let Some(ref pwd) = entry.password {
        sess.userauth_password(&entry.user, pwd)?;
    } else {
        return Err("节点未配置任何有效凭证（密码或私钥），拒绝访问".into());
    }
    
    if !sess.authenticated() {
        return Err("SFTP 拒绝访问：凭证失效，请检查密钥或密码是否被吊销".into());
    }
    Ok(())
}

pub fn list_directory(entry: &VpsEntry, path: &str) -> Result<Vec<RemoteFile>, Box<dyn std::error::Error>> {
    let tcp = TcpStream::connect(format!("{}:{}", entry.host, entry.port))?;
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;

    // 调用统一认证
    authenticate_session(&mut sess, entry)?;

    let sftp = sess.sftp()?;
    let stat = sftp.readdir(Path::new(path))?;

    let mut files = Vec::new();
    if path != "/" {
        files.push(RemoteFile { name: "..".to_string(), is_dir: true, size: 0 });
    }

    for (path_buf, stat) in stat {
        if let Some(name) = path_buf.file_name() {
            let name_str = name.to_string_lossy().to_string();
            if name_str == "." || name_str == ".." { continue; }
            files.push(RemoteFile { name: name_str, is_dir: stat.is_dir(), size: stat.size.unwrap_or(0) });
        }
    }
    
    files.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(files)
}

pub fn download_file(entry: &VpsEntry, remote_file: &str, local_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let tcp = TcpStream::connect(format!("{}:{}", entry.host, entry.port))?;
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;

    authenticate_session(&mut sess, entry)?;
    
    let sftp = sess.sftp()?;
    let mut remote = sftp.open(Path::new(remote_file))?;
    
    let mut local = std::fs::File::create(Path::new(local_file))?;
    let mut buffer = vec![0; 64 * 1024]; 
    loop {
        let n = std::io::Read::read(&mut remote, &mut buffer)?;
        if n == 0 { break; }
        std::io::Write::write_all(&mut local, &buffer[..n])?;
    }
    Ok(())
}

pub fn upload_file(entry: &VpsEntry, local_file: &str, remote_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let tcp = TcpStream::connect(format!("{}:{}", entry.host, entry.port))?;
    let mut sess = Session::new()?;
    sess.set_tcp_stream(tcp);
    sess.handshake()?;

    authenticate_session(&mut sess, entry)?;
    
    let sftp = sess.sftp()?;
    let file_name = Path::new(local_file).file_name().unwrap().to_str().unwrap();
    let remote_path = if remote_dir == "/" { format!("/{}", file_name) } else { format!("{}/{}", remote_dir, file_name) };

    let mut local = std::fs::File::open(local_file)?;
    let mut remote = sftp.create(Path::new(&remote_path))?;
    
    let mut buffer = vec![0; 64 * 1024]; 
    loop {
        let n = std::io::Read::read(&mut local, &mut buffer)?;
        if n == 0 { break; }
        std::io::Write::write_all(&mut remote, &buffer[..n])?;
    }
    Ok(())
}
