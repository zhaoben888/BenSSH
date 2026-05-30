# BenSHH 一键极客安装脚本
# 右键点击本文件，选择 "使用 PowerShell 运行" 即可完成自动安装

$installPath = "$env:USERPROFILE\AppData\Local\Programs\benshh"
$exeName = "benshh.exe"
$sourceExe = ".\benshh.exe"

Write-Host "🚀 开始安装 Google Antigravity - BenSHH..." -ForegroundColor Cyan

# 1. 检查有没有拿到编译好的核心程序
if (-Not (Test-Path $sourceExe)) {
    Write-Host "❌ 错误: 找不到 benshh.exe ！" -ForegroundColor Red
    Write-Host "请确保你把本安装脚本和编译好的 benshh.exe 放在同一个文件夹下运行。" -ForegroundColor Yellow
    Pause
    exit
}

# 2. 复制到系统级安全目录
Write-Host "📁 正在向系统核心区域植入程序..." 
New-Item -ItemType Directory -Force -Path $installPath | Out-Null
Copy-Item $sourceExe -Destination "$installPath\$exeName" -Force

$sourceIco = ".\benshh.ico"
if (Test-Path $sourceIco) {
    Copy-Item $sourceIco -Destination "$installPath\benshh.ico" -Force
}

Write-Host "✅ 核心程序植入成功: $installPath\$exeName" -ForegroundColor Green

# 3. 注册全局环境变量 (让任何终端都能随叫随到)
$oldPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($oldPath -notmatch [regex]::Escape($installPath)) {
    Write-Host "🌍 正在注入系统全局环境变量 (PATH)..."
    $newPath = $oldPath + ";" + $installPath
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Host "✅ 环境变量注入完成！你现在可以在任意处输入 benshh 召唤本程序了。" -ForegroundColor Green
} else {
    Write-Host "ℹ️ 环境变量已存在，跳过注入。" -ForegroundColor DarkGray
}

# 4. 创建桌面快捷方式 (双击即用)
Write-Host "🖥️ 正在生成桌面传送门 (快捷方式)..."
$WshShell = New-Object -comObject WScript.Shell
$Shortcut = $WshShell.CreateShortcut("$env:USERPROFILE\Desktop\BenSHH.lnk")
$Shortcut.TargetPath = "$installPath\$exeName"
$Shortcut.WorkingDirectory = "$installPath"
if (Test-Path "$installPath\benshh.ico") {
    $Shortcut.IconLocation = "$installPath\benshh.ico"
} else {
    $Shortcut.IconLocation = "%SystemRoot%\System32\shell32.dll,27" 
}
$Shortcut.Save()
Write-Host "✅ 桌面快捷方式生成完毕！" -ForegroundColor Green

Write-Host "`n🎉 安装全流程结束！" -ForegroundColor Cyan
Write-Host "1. 你现在可以直接双击桌面上的 BenSHH 图标使用了！"
Write-Host "2. 如果你想在终端里使用，请【关闭当前所有黑框并重新打开】，然后直接输入 benshh 敲回车即可！"
Pause
