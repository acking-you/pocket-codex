$ErrorActionPreference = 'Continue'
Set-Location -LiteralPath $PSScriptRoot

Write-Host ""
Write-Host "=== Pocket-Codex 子模块初始化 ===" -ForegroundColor Cyan
Write-Host "仓库目录: $PSScriptRoot"
Write-Host ""

# Check Git before modifying the local repository configuration.
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host "找不到 git，请先安装 Git for Windows: https://git-scm.com/download/win" -ForegroundColor Red
    exit 1
}
git --version

# Codex requires long-path support on Windows.
Write-Host ""
Write-Host "--> 为本仓库开启长路径支持" -ForegroundColor DarkGray
git config core.longpaths true

# First use the SSH URL recorded in .gitmodules.
Write-Host ""
Write-Host "--> 尝试 1/2: 使用 SSH 地址拉取子模块" -ForegroundColor Yellow
Write-Host "    (deps/codex 仓库较大, 可能要几分钟, 请耐心等待)" -ForegroundColor DarkGray
Write-Host ""
git submodule sync --recursive
git submodule update --init --recursive --progress
$ok = ($LASTEXITCODE -eq 0)

# Retry over HTTPS if SSH is unavailable.
if (-not $ok) {
    Write-Host ""
    Write-Host "--> SSH 方式失败, 尝试 2/2: 改用 HTTPS 重试" -ForegroundColor Yellow
    Write-Host ""
    git submodule sync --recursive
    git -c 'url.https://github.com/.insteadOf=git@github.com:' submodule update --init --recursive --progress
    $ok = ($LASTEXITCODE -eq 0)
    if ($ok) {
        Write-Host ""
        Write-Host "本次拉取已使用 HTTPS，未修改仓库的 SSH 地址。" -ForegroundColor DarkGray
    }
}

# Codex is the only submodule; pb-mapper now comes from the registry.
Write-Host ""
Write-Host "=== 结果 ===" -ForegroundColor Cyan
$allGood = $ok
foreach ($d in 'codex') {
    $p = Join-Path $PSScriptRoot "deps\$d"
    $n = 0
    if (Test-Path -LiteralPath $p) {
        $n = @(Get-ChildItem -LiteralPath $p -Force -ErrorAction SilentlyContinue).Count
    }
    if ($n -gt 0) {
        Write-Host ("  [ OK ]  deps/{0}  ({1} 项)" -f $d, $n) -ForegroundColor Green
    } else {
        Write-Host ("  [ 空 ]  deps/{0}" -f $d) -ForegroundColor Red
        $allGood = $false
    }
}

Write-Host ""
Write-Host "--- git submodule status ---" -ForegroundColor DarkGray
git submodule status --recursive

Write-Host ""
if ($allGood) {
    Write-Host "子模块已就绪。下一步可以跑:" -ForegroundColor Green
    Write-Host "    cargo build -p pocket_codex_bridge" -ForegroundColor Green
} else {
    Write-Host "子模块初始化失败。请查看上面的完整错误输出。" -ForegroundColor Red
}
Write-Host ""
if (-not $allGood) { exit 1 }
