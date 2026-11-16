# Windows에 kairos를 설치한다. 처음 설치와 업그레이드가 같은 명령이다. macOS는 scripts/install.sh, Linux는 scripts/install-linux.sh다.
#
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1 [--org <파일> [--sha256 <값>]]
#   ... -File scripts\install.ps1 --yes --google <회사 메일> --slack <Slack 멤버 ID> [--name <이름>]   # 묻지 않음
#   ... -File scripts\install.ps1 --update [--yes]    # 저장소를 최신으로 당긴 뒤 다시 설치한다
#   ... -File scripts\install.ps1 --no-modify-path    # 사용자 PATH를 건드리지 않는다
#   ... -File scripts\install.ps1 --help
#
# --update, --no-modify-path, --help를 뺀 인자는 그대로 kairos setup에 넘긴다.
# 받은 저장소에서 돌리면 빌드하고, 릴리스 묶음(bin\kairos.exe가 들어 있다)에서 돌리면 빌드를 건너뛴다.
# 1. 도구를 확인한다. 빠진 것을 한 번에 모아 알려 주고 멈춘다.
# 2. kairos, kairos-run, kairos-launch를 빌드한다(릴리스 묶음은 건너뛴다).
# 3. kairos setup으로 넘긴다. 설치 단계는 작업 스케줄러에 작업을 등록한다.
# 4. 설치했으면 설치본의 bin(%LOCALAPPDATA%\kairos\current\bin)이 사용자 PATH에 없을 때 더할지 묻는다(--yes면 묻지 않고 더한다).
#    Windows에서는 세션(에이전트에 맡기기)을 열지 않는다. 수집, 패널, 알림은 macOS와 같다.
#    데이터 폴더(KAIROS_HOME)는 %LOCALAPPDATA% 아래 절대 경로여야 한다. 다른 곳이면 아무것도 바꾸기 전에 멈춘다.

# 파일은 BOM이 있는 UTF-8이다. Windows PowerShell 5.1은 BOM이 없으면 한글을 시스템 코드 페이지로 읽는다.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 3

$repo = Split-Path -Parent $PSScriptRoot
$localAppData = [Environment]::GetFolderPath('LocalApplicationData')
$data = if ($env:KAIROS_HOME) { $env:KAIROS_HOME } else { Join-Path $localAppData 'kairos' }

function Step([string]$text) { Write-Host ''; Write-Host "== $text" }
function Note([string]$text) { Write-Host "  $text" }
function Fail([string]$text) { [Console]::Error.WriteLine("멈춤: $text"); exit 1 }
function Have([string]$name) { [bool](Get-Command $name -CommandType Application -ErrorAction SilentlyContinue) }
# 출력을 버리고 성공만 본다. Windows PowerShell은 Stop일 때 넘긴 stderr를 오류로 던지므로 잠시 늦춘다.
function Succeeds([string]$file, [string[]]$arguments) {
  $saved = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try { & $file @arguments *> $null; return $LASTEXITCODE -eq 0 } catch { return $false } finally { $ErrorActionPreference = $saved }
}

# 이 스크립트의 옵션을 떼고 나머지를 setup에 넘긴다. 빈 인자도 자리를 지킨다.
$update = $false
$modifyPath = $true
$yes = $false
$setupArgs = New-Object System.Collections.Generic.List[string]
foreach ($arg in $args) {
  switch -CaseSensitive ([string]$arg) {
    { $_ -in '-h', '--help' } {
      # 머리말의 주석이 곧 사용법이다.
      foreach ($line in Get-Content -LiteralPath $PSCommandPath -Encoding UTF8) {
        if ($line -notmatch '^#') { break }
        Write-Host ($line -replace '^# ?', '')
      }
      exit 0
    }
    '--update' { $update = $true }
    '--no-modify-path' { $modifyPath = $false }
    '--yes' { $yes = $true; $setupArgs.Add($_) }
    default { $setupArgs.Add($_) }
  }
}

# 릴리스 묶음에는 빌드한 실행 파일이 들어 있고 Cargo.toml이 없다.
$prebuilt = (Test-Path -LiteralPath (Join-Path $repo 'bin\kairos.exe') -PathType Leaf) -and
  -not (Test-Path -LiteralPath (Join-Path $repo 'Cargo.toml') -PathType Leaf)

if ($update) {
  if ($prebuilt) { Fail '릴리스 설치는 설치 명령을 다시 실행하면 최신 판으로 올라갑니다(kairos update가 알려 줍니다)' }
  Step '업데이트: 저장소 당기기'
  if (-not (Have 'git')) { Fail 'git이 없습니다' }
  if (git -C $repo status --porcelain) { Fail '저장소에 커밋하지 않은 변경이 있습니다. 정리한 뒤 다시 실행하세요' }
  git -C $repo pull --ff-only
  if ($LASTEXITCODE -ne 0) { Fail '당기지 못했습니다. 위 메시지를 확인하세요' }
  # 당긴 뒤의 스크립트로 다시 시작한다. 지금 읽는 파일이 바뀌었을 수 있다. PATH는 건드리지 않는다.
  $again = @($setupArgs) + '--no-modify-path'
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $repo 'scripts\install.ps1') @again
  exit $LASTEXITCODE
}

Step '1/4 도구 확인'
if ([Environment]::OSVersion.Platform -ne 'Win32NT') { Fail 'Windows에서만 설치합니다. macOS는 scripts/install.sh, Linux는 scripts/install-linux.sh를 쓰세요' }
# kairos는 %LOCALAPPDATA% 밖의 데이터 폴더를 거절한다. PATH와 파일을 바꾸기 전에 같은 규칙으로 먼저 멈춘다.
if (-not [IO.Path]::IsPathRooted($data)) { Fail "KAIROS_HOME은 절대 경로여야 합니다: $data" }
$data = [IO.Path]::GetFullPath($data).TrimEnd('\')
$root = [IO.Path]::GetFullPath($localAppData).TrimEnd('\') + '\'
if (-not $data.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
  Fail "KAIROS_HOME은 $root 아래여야 합니다: $data"
}
$binDir = Join-Path (Join-Path $data 'current') 'bin'
$missing = New-Object System.Collections.Generic.List[string]
if (-not $prebuilt -and -not (Have 'cargo')) {
  $missing.Add('Rust: https://rustup.rs 의 rustup-init.exe로 설치하고 새 터미널을 여세요(Visual Studio Build Tools의 C++ 도구가 함께 필요합니다)')
}
if (-not (Have 'gh')) { $missing.Add('gh: winget install --id GitHub.cli 로 설치한 뒤 gh auth login') }
if ($missing.Count -gt 0) {
  [Console]::Error.WriteLine('멈춤: 아래를 갖춘 뒤 다시 실행하세요')
  foreach ($item in $missing) { [Console]::Error.WriteLine("  - $item") }
  exit 1
}
if (-not (Succeeds 'gh' @('auth', 'status'))) { Note 'gh에 로그인되어 있지 않습니다. GitHub를 쓰려면 gh auth login을 먼저 하세요' }
if (-not (Have 'orca')) { Note 'orca가 PATH에 없습니다. 패널 단계는 건너뜁니다' }
Note '도구 확인'

Step '2/4 빌드'
if ($prebuilt) {
  $kairos = Join-Path $repo 'bin\kairos.exe'
  Note '릴리스 묶음이라 빌드하지 않습니다'
} else {
  Push-Location $repo
  try {
    & cargo build --release -p kairos-cli
    if ($LASTEXITCODE -ne 0) { Fail '빌드하지 못했습니다. 위 메시지를 확인하세요' }
  } finally {
    Pop-Location
  }
  $kairos = Join-Path $repo 'target\release\kairos.exe'
}

# 설치 방식을 적어 둔다. kairos update가 읽고 맞는 명령을 알려 준다. BOM 없는 UTF-8 한 줄이다.
New-Item -ItemType Directory -Force -Path $data | Out-Null
$record = if ($prebuilt) { "release`n" } else { "source:$repo`n" }
$temp = Join-Path $data ('.install-source.' + [Guid]::NewGuid().ToString('N'))
[IO.File]::WriteAllText($temp, $record, (New-Object System.Text.UTF8Encoding $false))
Move-Item -LiteralPath $temp -Destination (Join-Path $data 'install-source') -Force

Step '3/4 kairos setup'
& $kairos setup @setupArgs
$code = $LASTEXITCODE

Step '4/4 kairos 명령'
# setup이 설치본을 놓았을 때만 PATH를 고친다. 설치하지 않았거나 실패했으면 PATH를 건드리지 않는다.
if (-not (Test-Path -LiteralPath (Join-Path $binDir 'kairos.exe') -PathType Leaf)) {
  Note '설치하지 않아 kairos 명령이 없습니다. PATH는 고치지 않았습니다'
  if ($prebuilt) {
    Note '받은 묶음은 끝나면 지워집니다. 설치하려면 설치 명령을 다시 실행하고 설치 질문에 y로 답하세요(--yes면 묻지 않습니다)'
  }
  exit $code
}
# Windows는 링크 대신 설치본의 bin(current는 junction이다)을 사용자 PATH에 둔다. 판이 바뀌어도 다시 고칠 필요가 없다.
# 레지스트리 값을 펼치지 않고 읽고 쓴다. %USERPROFILE% 같은 항목이 고정 경로로 바뀌지 않는다.
$envKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
$userPath = [string]$envKey.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
# 값의 종류(REG_SZ, REG_EXPAND_SZ)를 그대로 둔다. 없으면 펼치는 값으로 만든다.
$kind = if ($null -ne $envKey.GetValue('Path')) { $envKey.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
$entries = @($userPath -split ';' | Where-Object { $_ })
$expanded = @($entries | ForEach-Object { [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') })
if ($expanded -icontains $binDir.TrimEnd('\')) {
  Note "사용자 PATH에 $binDir 가 있습니다"
} elseif (-not $modifyPath) {
  Note "사용자 PATH에 $binDir 를 더하지 않았습니다(--no-modify-path)"
} else {
  $add = $yes
  if (-not $yes) {
    $answer = Read-Host "  사용자 PATH에 $binDir 를 더할까요? [Y/n]"
    $add = $answer -eq '' -or $answer -match '^[Yy]'
  }
  if ($add) {
    $joined = if ($userPath -eq '') { $binDir } else { $userPath.TrimEnd(';') + ';' + $binDir }
    $envKey.SetValue('Path', $joined, $kind)
    # 바뀐 환경을 다른 프로그램에 알린다(WM_SETTINGCHANGE). 빈 값을 썼다 지우면 .NET이 대신 알린다.
    [Environment]::SetEnvironmentVariable('KAIROS_PATH_REFRESH', '1', 'User')
    [Environment]::SetEnvironmentVariable('KAIROS_PATH_REFRESH', $null, 'User')
    Note "사용자 PATH에 $binDir 를 더했습니다. 새 터미널부터 kairos를 부를 수 있습니다"
  } else {
    Note "더하지 않았습니다. kairos는 $binDir\kairos.exe 로 부르세요"
  }
}
$envKey.Close()

exit $code
