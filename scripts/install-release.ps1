# 빌드 없이 Windows에 kairos를 설치한다. 최신 릴리스 묶음을 받아 SHA256을 확인하고 scripts\install.ps1로 넘긴다.
# 다시 실행하면 최신 판으로 올린다. 설정과 데이터는 그대로 둔다.
#
#   irm https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.ps1 | iex
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.ps1))) --yes --google <회사 메일> --slack <Slack 멤버 ID>   # 묻지 않음
#
# 인자는 묶음 안의 scripts\install.ps1에 그대로 넘긴다.
# 묶음은 x86_64 하나다(ARM64 Windows는 에뮬레이션으로 돈다). 미리 있어야 하는 것은 로그인한 gh다.
# KAIROS_RELEASE_URL을 주면 그 주소에서 받는다(사내 미러, 시험). 기본은 GitHub의 최신 릴리스다.
# SHA256 파일은 묶음과 같은 곳에서 받는다. 받는 중에 깨진 파일은 걸러내지만 올린 사람을 증명하지는 않는다.
# iex로 돌면 사용자의 창 안에서 돈다. exit를 쓰지 않고 돌아온다(창이 닫히지 않는다).
# 이 파일은 BOM 없는 UTF-8이다. irm이 UTF-8로 읽어 넘긴다.

# 명령 줄 한 칸으로 감싼다(CommandLineToArgvW 규칙). 따옴표 앞의 역슬래시는 두 배로 쓴다.
function ConvertTo-KairosArgument([string]$text) {
  if ($text -ne '' -and $text -notmatch '[\s"]') { return $text }
  $escaped = [regex]::Replace($text, '(\\*)"', '$1$1\"')
  $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
  return '"' + $escaped + '"'
}

# 한 파일을 받는다. https는 리디렉션 뒤의 마지막 주소도 https여야 한다. file://은 이 컴퓨터의 경로만 읽는다(공유 폴더는 받지 않는다).
function Get-KairosFile([string]$uri, [string]$out) {
  $parsed = [Uri]$uri
  if ($parsed.Scheme -eq 'file') {
    if ($parsed.IsUnc -or $parsed.Host) { throw "공유 폴더 주소는 받지 않습니다: $uri" }
    Copy-Item -LiteralPath $parsed.LocalPath -Destination $out
    return
  }
  $response = Invoke-WebRequest -UseBasicParsing -Uri $uri -OutFile $out -PassThru
  # Windows PowerShell은 ResponseUri, PowerShell 7은 RequestMessage.RequestUri에 마지막 주소가 있다.
  $base = $response.BaseResponse
  $final = if ($base.PSObject.Properties['ResponseUri']) { $base.ResponseUri } else { $base.RequestMessage.RequestUri }
  if ($null -eq $final -or $final.Scheme -ne 'https') {
    Remove-Item -LiteralPath $out -Force -ErrorAction SilentlyContinue
    throw "https가 아닌 주소로 넘어갔습니다: $final"
  }
}

function Install-KairosRelease {
  $ErrorActionPreference = 'Stop'
  Set-StrictMode -Version 3
  $base = if ($env:KAIROS_RELEASE_URL) { $env:KAIROS_RELEASE_URL.TrimEnd('/') } else { 'https://github.com/blan22/Kairos/releases/latest/download' }

  if ([Environment]::OSVersion.Platform -ne 'Win32NT') {
    Write-Host '멈춤: Windows에서만 설치합니다. macOS는 install-release.sh, Linux는 install-release-linux.sh를 쓰세요'
    return 1
  }
  # https만 받는다. 리디렉션도 https여야 한다. file:// 미러(시험)는 이 컴퓨터의 경로만 읽는다.
  if ($base -notmatch '^(https|file)://') {
    Write-Host "멈춤: 받을 주소는 https:// 또는 file://이어야 합니다: $base"
    return 1
  }
  $asset = 'kairos-x86_64-pc-windows-msvc.zip'
  # Windows PowerShell 5.1의 기본 TLS가 낮을 수 있다. 1.2를 켠다.
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
  $work = Join-Path ([IO.Path]::GetTempPath()) ('kairos-release-' + [Guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $work | Out-Null
  try {
    Write-Host '== kairos 릴리스 받기'
    $zip = Join-Path $work $asset
    $sumFile = "$zip.sha256"
    try {
      Get-KairosFile "$base/$asset" $zip
      Get-KairosFile "$base/$asset.sha256" $sumFile
    } catch {
      Write-Host "멈춤: 받지 못했습니다: $base/$asset ($($_.Exception.Message))"
      return 1
    }
    $expected = ([IO.File]::ReadAllText($sumFile)).Trim()
    if ($expected.Length -ge 64) { $expected = $expected.Substring(0, 64) }
    if ($expected -cnotmatch '^[0-9a-f]{64}$') {
      Write-Host '멈춤: SHA256 파일의 모양이 맞지 않습니다'
      return 1
    }
    $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -cne $expected) {
      Write-Host '멈춤: SHA256이 다릅니다. 받은 파일을 쓰지 않습니다'
      return 1
    }
    Write-Host "  SHA256 확인 $actual"

    $bundle = Join-Path $work 'bundle'
    Expand-Archive -LiteralPath $zip -DestinationPath $bundle
    $roots = @(Get-ChildItem -LiteralPath $bundle -Directory -Filter 'kairos-*')
    if ($roots.Count -ne 1) {
      Write-Host '멈춤: 묶음의 모양이 맞지 않습니다'
      return 1
    }
    $root = $roots[0].FullName
    $installer = Join-Path $root 'scripts\install.ps1'
    if (-not (Test-Path -LiteralPath (Join-Path $root 'bin\kairos.exe') -PathType Leaf) -or
        -not (Test-Path -LiteralPath $installer -PathType Leaf)) {
      Write-Host '멈춤: 묶음에 실행 파일이나 설치 스크립트가 없습니다'
      return 1
    }
    Write-Host "  kairos $((Get-Content -LiteralPath (Join-Path $root 'VERSION') -TotalCount 1).Trim())"

    # 설치 스크립트는 따로 띄우고 이 창의 콘솔을 그대로 준다. 그 안의 exit가 이 창을 닫지 않고 setup이 물을 수 있다.
    $line = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $installer) + @($args) |
      ForEach-Object { ConvertTo-KairosArgument ([string]$_) }
    $process = Start-Process -FilePath 'powershell.exe' -ArgumentList ($line -join ' ') -NoNewWindow -PassThru
    # 핸들을 먼저 잡아야 끝난 뒤 종료 코드를 읽는다.
    $null = $process.Handle
    $process.WaitForExit()
    return $process.ExitCode
  } finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
  }
}

$global:LASTEXITCODE = Install-KairosRelease @args
