# Windows 릴리스 묶음을 만든다. 받는 사람은 Rust 없이 설치한다(scripts\install-release.ps1). macOS는 scripts/package.sh, Linux는 scripts/package-linux.sh다.
#
#   pwsh -File scripts\package.ps1 [--no-build]
#
# 결과는 target\dist\kairos-x86_64-pc-windows-msvc.zip과 같은 이름의 .sha256이다.
# 묶음 안은 kairos-<판>\ 아래에 bin\, ops\windows\, plugins\, scripts\install.ps1, 문서가 놓인다.
# 실행 파일의 조상에 ops\와 plugins\가 있어 kairos가 설치 자료를 찾는다.
# --no-build는 이미 빌드한 target\release를 그대로 묶는다.

# 파일은 BOM이 있는 UTF-8이다. Windows PowerShell 5.1은 BOM이 없으면 한글을 시스템 코드 페이지로 읽는다.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

$repo = Split-Path -Parent $PSScriptRoot
$asset = 'kairos-x86_64-pc-windows-msvc.zip'
$dist = Join-Path $repo 'target\dist'

function Fail([string]$text) { [Console]::Error.WriteLine("멈춤: $text"); exit 1 }
function Step([string]$text) { Write-Host ''; Write-Host "== $text" }

$build = $true
switch ($args.Count) {
  0 { }
  1 { if ($args[0] -ceq '--no-build') { $build = $false } else { Fail '사용법: scripts\package.ps1 [--no-build]' } }
  default { Fail '사용법: scripts\package.ps1 [--no-build]' }
}

Set-Location $repo
if ([Environment]::OSVersion.Platform -ne 'Win32NT') { Fail 'Windows에서만 묶습니다' }
$version = (Select-String -LiteralPath 'Cargo.toml' -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if ($version -notmatch '^\d+\.\d+\.\d+') { Fail 'Cargo.toml에서 판 번호를 읽지 못했습니다' }
$name = "kairos-$version"
$binaries = 'kairos.exe', 'kairos-run.exe', 'kairos-launch.exe'

if ($build) {
  Step '빌드'
  & cargo build --release --locked -p kairos-cli
  if ($LASTEXITCODE -ne 0) { Fail '빌드하지 못했습니다' }
}

Step '확인'
foreach ($bin in $binaries) {
  if (-not (Test-Path -LiteralPath "target\release\$bin" -PathType Leaf)) { Fail "target\release\$bin 가 없습니다. --no-build 없이 다시 실행하세요" }
}
# 빈 데이터 폴더로 판만 묻는다. kairos는 %LOCALAPPDATA% 밖의 데이터 폴더를 거절하므로 그 아래 임시 폴더를 쓴다.
$probe = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) ('kairos-probe-' + [Guid]::NewGuid().ToString('N'))
$stage = Join-Path $dist 'stage'
try {
  $env:KAIROS_HOME = Join-Path $probe 'home'
  $built = (& "target\release\kairos.exe" --version | Out-String).Trim()
  Remove-Item Env:KAIROS_HOME
  if ($built -ne "kairos $version") { Fail "target\release\kairos.exe는 '$built'입니다. 지금 소스(kairos $version)로 다시 빌드하세요" }
  Write-Host "  $built"

  Step '묶기'
  if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
  $top = Join-Path $stage $name
  New-Item -ItemType Directory -Force -Path (Join-Path $top 'bin'), (Join-Path $top 'ops\windows') | Out-Null
  foreach ($bin in $binaries) { Copy-Item -LiteralPath "target\release\$bin" -Destination (Join-Path $top 'bin') }
  Copy-Item -Path 'ops\windows\*.template' -Destination (Join-Path $top 'ops\windows')
  # 나머지는 git이 추적하는 파일만 넣는다. node_modules와 로컬 설정이 섞이지 않는다.
  $tracked = (& git ls-files -z -- plugins examples docs scripts/install.ps1 profiles/README.md README.md LICENSE SECURITY.md) -split "`0" | Where-Object { $_ }
  if ($LASTEXITCODE -ne 0) { Fail 'git ls-files가 실패했습니다' }
  foreach ($file in $tracked) {
    $target = Join-Path $top $file
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
    Copy-Item -LiteralPath $file -Destination $target
  }
  [IO.File]::WriteAllText((Join-Path $top 'VERSION'), "$version`n", (New-Object System.Text.UTF8Encoding $false))
  if (-not (Test-Path -LiteralPath (Join-Path $top 'scripts\install.ps1') -PathType Leaf)) { Fail 'scripts\install.ps1이 묶음에 없습니다' }

  New-Item -ItemType Directory -Force -Path $dist | Out-Null
  $zip = Join-Path $dist $asset
  if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
  Compress-Archive -Path $top -DestinationPath $zip
  $sum = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
  [IO.File]::WriteAllText("$zip.sha256", "$sum`n", (New-Object System.Text.UTF8Encoding $false))

  Write-Host "  $zip"
  Write-Host "  sha256 $sum"
  Write-Host "완료: $name"
} finally {
  Remove-Item -LiteralPath $probe, $stage -Recurse -Force -ErrorAction SilentlyContinue
}
